//! 预测工件存储（store.ts）。
//!
//! 所有工件都在 `story/runtime/narrative-forecasts/<forecastId>/` 之下——
//! v1 安全边界：本存储永不写 `story/state/*.json`、`story/*.md` 控制文档
//! 或 `chapters/`。写入前先校验，无效 forecast 不落半写目录。

use std::path::PathBuf;

use regex::Regex;
use serde_json::json;
use std::sync::OnceLock;

use super::schema::{validate_narrative_forecast, NarrativeForecast};

/// `assertSafeForecastId`：`^[A-Za-z0-9][A-Za-z0-9_-]{0,79}$`。
pub fn assert_safe_forecast_id(value: &str) -> Result<String, String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,79}$").unwrap());
    if !re.is_match(value) {
        return Err(format!("Invalid forecast id: {}", serde_json::to_string(value).unwrap_or_default()));
    }
    Ok(value.to_string())
}

pub struct ForecastStore {
    book_dir: PathBuf,
}

impl ForecastStore {
    pub fn new(book_dir: impl Into<PathBuf>) -> Self {
        ForecastStore { book_dir: book_dir.into() }
    }

    pub fn forecasts_dir(&self) -> PathBuf {
        self.book_dir.join("story").join("runtime").join("narrative-forecasts")
    }

    pub fn forecast_dir(&self, forecast_id: &str) -> Result<PathBuf, String> {
        Ok(self.forecasts_dir().join(assert_safe_forecast_id(forecast_id)?))
    }

    pub fn forecast_json_path(&self, forecast_id: &str) -> Result<String, String> {
        Ok(self
            .forecast_dir(forecast_id)?
            .join("forecast.json")
            .to_string_lossy()
            .into_owned())
    }

    pub fn comparison_path(&self, forecast_id: &str) -> Result<String, String> {
        Ok(self
            .forecast_dir(forecast_id)?
            .join("comparison.md")
            .to_string_lossy()
            .into_owned())
    }

    pub fn selected_plan_path(&self, forecast_id: &str) -> Result<String, String> {
        Ok(self
            .forecast_dir(forecast_id)?
            .join("selected-branch-plan.md")
            .to_string_lossy()
            .into_owned())
    }

    /// 派生下一个预测 id：时间戳基名；目录已存在则追加 -2/-3…（重跑永不
    /// 覆盖早前预测）。
    pub async fn allocate_forecast_id(&self, now_iso: &str) -> Result<String, String> {
        let base = format!("fc-{}", format_timestamp(now_iso));
        let mut candidate = base.clone();
        let mut suffix = 2;
        while self.forecast_dir(&candidate).map(|d| d.is_dir()).unwrap_or(true) {
            candidate = format!("{base}-{suffix}");
            suffix += 1;
        }
        Ok(candidate)
    }

    /// 保存：校验 → mkdir → forecast.json（pretty + 尾换行）与
    /// comparison.md（trimEnd + 尾换行）。
    pub async fn save(
        &self,
        forecast: &NarrativeForecast,
        comparison_markdown: &str,
    ) -> Result<(String, String), String> {
        validate_narrative_forecast(forecast)?;
        let dir = self.forecast_dir(&forecast.forecast_id)?;
        tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
        let forecast_json_path = self.forecast_json_path(&forecast.forecast_id)?;
        let comparison_path = self.comparison_path(&forecast.forecast_id)?;
        let json = serde_json::to_string_pretty(forecast).map_err(|e| e.to_string())?;
        crate::utils::atomic_file_set::write_file_atomic(&dir.join("forecast.json"), &format!("{json}\n"))
            .await
            .map_err(|e| e.to_string())?;
        crate::utils::atomic_file_set::write_file_atomic(&dir.join("comparison.md"), &format!("{}\n", trim_end(comparison_markdown)))
            .await
            .map_err(|e| e.to_string())?;
        Ok((forecast_json_path, comparison_path))
    }

    /// 装载：缺文件 → 列可用 id 的逐字错误；JSON 损坏/schema 不过 → 逐字错误。
    pub async fn load(&self, forecast_id: &str) -> Result<NarrativeForecast, String> {
        let dir = self.forecast_dir(forecast_id)?;
        let raw = match tokio::fs::read_to_string(dir.join("forecast.json")).await {
            Ok(raw) => raw,
            Err(_) => {
                let available = self.list().await;
                return Err(format!(
                    "Narrative forecast \"{forecast_id}\" not found. Available forecasts: {}",
                    if available.is_empty() { "(none)".to_string() } else { available.join(", ") }
                ));
            }
        };
        // TS 两段错误面：JSON.parse 失败 = corrupted；zod 失败 = schema validation。
        let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
            format!("Narrative forecast \"{forecast_id}\" has corrupted forecast.json: {e}")
        })?;
        let parsed: NarrativeForecast = serde_json::from_value(value).map_err(|e| {
            format!("Narrative forecast \"{forecast_id}\" failed schema validation: {e}")
        })?;
        validate_narrative_forecast(&parsed)
            .map_err(|e| format!("Narrative forecast \"{forecast_id}\" failed schema validation: {e}"))?;
        Ok(parsed)
    }

    /// 列出含 forecast.json 的预测 id（排序）。
    pub async fn list(&self) -> Vec<String> {
        let mut ids = Vec::new();
        let Ok(mut entries) = tokio::fs::read_dir(self.forecasts_dir()).await else {
            return ids;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().join("forecast.json").is_file() {
                ids.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        ids.sort();
        ids
    }

    /// 持久化 stale 标记（后续读者无需重算）。
    pub async fn mark_stale(&self, forecast: &NarrativeForecast) -> Result<NarrativeForecast, String> {
        let mut stale = forecast.clone();
        stale.status = super::schema::ForecastStatus::Stale;
        validate_narrative_forecast(&stale)?;
        let dir = self.forecast_dir(&stale.forecast_id)?;
        let json = serde_json::to_string_pretty(&stale).map_err(|e| e.to_string())?;
        crate::utils::atomic_file_set::write_file_atomic(&dir.join("forecast.json"), &format!("{json}\n"))
            .await
            .map_err(|e| e.to_string())?;
        Ok(stale)
    }

    pub async fn write_selected_plan(&self, forecast_id: &str, markdown: &str) -> Result<String, String> {
        let path = self.selected_plan_path(forecast_id)?;
        let dir = self.forecast_dir(forecast_id)?;
        crate::utils::atomic_file_set::write_file_atomic(&dir.join("selected-branch-plan.md"), &format!("{}\n", trim_end(markdown)))
            .await
            .map_err(|e| e.to_string())?;
        Ok(path)
    }
}

/// TS `formatTimestamp`：ISO → `YYYYMMDD-HHMMSS`。
pub fn format_timestamp(iso: &str) -> String {
    // ISO 前缀恒为 YYYY-MM-DDTHH:MM:SS…（utc_now_iso 保证）。
    let date = &iso[0..10];
    let time = &iso[11..19];
    format!("{}-{}", date.replace('-', ""), time.replace(':', ""))
}

/// TS `String.trimEnd()`：剥尾部 Unicode 空白。
fn trim_end(value: &str) -> &str {
    value.trim_end_matches(|c: char| c.is_whitespace())
}

/// `NarrativeForecastSchema` 的 zod 约束手工等价（保存/装载双端校验）。
pub fn fingerprint_canonical_json(base_chapter: u32, files: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let entries: Vec<serde_json::Value> = sorted
        .iter()
        .map(|(path, content)| json!([path, content]))
        .collect();
    let canonical = json!({ "baseChapter": base_chapter, "files": entries });
    serde_json::to_string(&canonical).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_forecast(id: &str) -> NarrativeForecast {
        NarrativeForecast {
            version: 1,
            forecast_id: id.to_string(),
            book_id: "b1".to_string(),
            created_at: "2026-08-18T00:00:00.000Z".to_string(),
            language: "zh".to_string(),
            divergence: "合作还是对抗".to_string(),
            horizon: 5,
            base_chapter: 3,
            context_fingerprint: "abc".to_string(),
            status: super::super::schema::ForecastStatus::Active,
            branches: (1..=2)
                .map(|i| super::super::schema::ForecastBranch {
                    branch_id: format!("branch-{i}"),
                    title: format!("分支{i}"),
                    premise: "p".to_string(),
                    beats: vec![super::super::schema::ForecastBeat {
                        chapter: 4,
                        summary: "s".to_string(),
                    }],
                    character_decisions: Vec::new(),
                    projected_changes: Default::default(),
                    risks: Vec::new(),
                    uncertainties: Vec::new(),
                    intent_alignment: super::super::schema::ForecastIntentAlignment {
                        score: 80,
                        rationale: "r".to_string(),
                    },
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn save_load_list_and_stale_marker() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("books").join("b1");
        tokio::fs::create_dir_all(&book).await.unwrap();
        let store = ForecastStore::new(&book);

        let (json_path, comparison_path) = store.save(&sample_forecast("fc-test-1"), "# 对比\n").await.unwrap();
        assert!(json_path.ends_with("forecast.json"));
        assert!(comparison_path.ends_with("comparison.md"));
        // pretty JSON + 尾换行；comparison trimEnd + 尾换行。
        let raw = tokio::fs::read_to_string(&book.join("story/runtime/narrative-forecasts/fc-test-1/forecast.json"))
            .await
            .unwrap();
        assert!(raw.ends_with("}\n"));
        assert!(raw.contains("\"branchId\": \"branch-1\"") || raw.contains("\"branchId\":\"branch-1\""));
        let comparison = tokio::fs::read_to_string(&book.join("story/runtime/narrative-forecasts/fc-test-1/comparison.md"))
            .await
            .unwrap();
        assert_eq!(comparison, "# 对比\n");

        let loaded = store.load("fc-test-1").await.unwrap();
        assert_eq!(loaded.branches.len(), 2);
        assert_eq!(store.list().await, vec!["fc-test-1".to_string()]);

        let stale = store.mark_stale(&loaded).await.unwrap();
        assert!(matches!(stale.status, super::super::schema::ForecastStatus::Stale));
        assert!(matches!(store.load("fc-test-1").await.unwrap().status, super::super::schema::ForecastStatus::Stale));

        // 缺失 → 可用清单逐字。
        let err = store.load("fc-missing").await.unwrap_err();
        assert_eq!(err, "Narrative forecast \"fc-missing\" not found. Available forecasts: fc-test-1");

        // 选择计划：trimEnd + 尾换行。
        let plan = store.write_selected_plan("fc-test-1", "计划正文\n\n\n").await.unwrap();
        assert!(plan.ends_with("selected-branch-plan.md"));
        let saved = tokio::fs::read_to_string(&book.join("story/runtime/narrative-forecasts/fc-test-1/selected-branch-plan.md"))
            .await
            .unwrap();
        assert_eq!(saved, "计划正文\n");
    }

    #[tokio::test]
    async fn allocate_id_appends_suffix_on_collision() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("books").join("b1");
        let forecasts = book.join("story").join("runtime").join("narrative-forecasts");
        tokio::fs::create_dir_all(forecasts.join("fc-20260818-000000")).await.unwrap();
        let store = ForecastStore::new(&book);
        let first = store.allocate_forecast_id("2026-08-18T00:00:00.000Z").await.unwrap();
        assert_eq!(first, "fc-20260818-000000-2");
        // 未实际创建 -2 目录时再次分配仍得 -2（pathExists 语义，与 TS 一致）。
        let second = store.allocate_forecast_id("2026-08-18T00:00:00.000Z").await.unwrap();
        assert_eq!(second, "fc-20260818-000000-2");
        tokio::fs::create_dir_all(forecasts.join("fc-20260818-000000-2")).await.unwrap();
        let third = store.allocate_forecast_id("2026-08-18T00:00:00.000Z").await.unwrap();
        assert_eq!(third, "fc-20260818-000000-3");
    }

    #[test]
    fn timestamp_format_and_safe_id() {
        assert_eq!(format_timestamp("2026-08-18T09:05:07.123Z"), "20260818-090507");
        assert!(assert_safe_forecast_id("fc-x").is_ok());
        assert!(assert_safe_forecast_id("../escape").is_err());
        assert!(assert_safe_forecast_id("").is_err());
    }
}
