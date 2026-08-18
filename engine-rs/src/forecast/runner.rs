//! 预测三操作（runner.ts，RFC #342 v1）：create / get / select。
//!
//! 全部工件都在 `story/runtime/narrative-forecasts/<forecastId>/` 之下——
//! 本文件任何操作不得写 `story/state/*.json`、`story/*.md` 控制文档或
//! `chapters/`。

use std::path::{Path, PathBuf};

use crate::utils::utc_time::utc_now_iso;

use super::agent::{generate_branches, ForecastChat, ForecastGenerationInput};
use super::context_builder::{build_forecast_context, render_forecast_context_markdown};
use super::render::{render_forecast_comparison_markdown, render_selected_branch_plan_markdown};
use super::schema::{
    ForecastBranch, ForecastStatus, NarrativeForecast, FORECAST_DEFAULT_BRANCHES,
    FORECAST_DEFAULT_HORIZON, FORECAST_MAX_BRANCHES, FORECAST_MAX_HORIZON, FORECAST_MIN_BRANCHES,
    FORECAST_MIN_HORIZON,
};
use super::store::ForecastStore;

pub struct CreateNarrativeForecastOptions<'a> {
    pub project_root: &'a Path,
    pub book_id: &'a str,
    pub divergence: &'a str,
    pub branch_count: Option<u32>,
    pub horizon: Option<u32>,
}

#[derive(Debug)]
pub struct NarrativeForecastCreateResult {
    pub forecast: NarrativeForecast,
    pub forecast_json_path: String,
    pub comparison_path: String,
}

pub async fn create_narrative_forecast(
    chat: &dyn ForecastChat,
    options: &CreateNarrativeForecastOptions<'_>,
) -> Result<NarrativeForecastCreateResult, String> {
    let book_id = crate::interaction::session::is_safe_book_id(options.book_id)
        .then_some(options.book_id.to_string())
        .ok_or_else(|| format!("Invalid forecast.bookId: \"{}\"", options.book_id))?;
    let divergence = options.divergence.trim();
    if divergence.is_empty() {
        return Err("divergence is required: describe the decision point the forecast should branch on.".to_string());
    }
    let branch_count = bounded_integer(
        options.branch_count,
        FORECAST_DEFAULT_BRANCHES as u32,
        "branchCount",
        FORECAST_MIN_BRANCHES as u32,
        FORECAST_MAX_BRANCHES as u32,
    )?;
    let horizon = bounded_integer(
        options.horizon,
        FORECAST_DEFAULT_HORIZON,
        "horizon",
        FORECAST_MIN_HORIZON,
        FORECAST_MAX_HORIZON,
    )?;
    let book_dir = resolve_book_dir(options.project_root, &book_id).await?;

    let context = build_forecast_context(&book_dir, &book_id).await;
    let model_output = generate_branches(
        chat,
        &ForecastGenerationInput {
            context_markdown: &render_forecast_context_markdown(&context),
            divergence,
            branch_count: branch_count as usize,
            horizon,
            base_chapter: context.base_chapter,
            language: &context.language,
        },
    )
    .await?;

    let store = ForecastStore::new(&book_dir);
    let now_iso = utc_now_iso();
    let forecast = NarrativeForecast {
        version: 1,
        forecast_id: store.allocate_forecast_id(&now_iso).await?,
        book_id,
        created_at: now_iso,
        language: context.language,
        divergence: divergence.to_string(),
        horizon,
        base_chapter: context.base_chapter,
        context_fingerprint: context.context_fingerprint,
        status: ForecastStatus::Active,
        branches: model_output
            .branches
            .into_iter()
            .enumerate()
            .map(|(index, branch)| ForecastBranch {
                branch_id: format!("branch-{}", index + 1),
                title: branch.title,
                premise: branch.premise,
                beats: branch.beats,
                character_decisions: branch.character_decisions,
                projected_changes: branch.projected_changes,
                risks: branch.risks,
                uncertainties: branch.uncertainties,
                intent_alignment: branch.intent_alignment,
            })
            .collect(),
    };

    let (forecast_json_path, comparison_path) = store
        .save(&forecast, &render_forecast_comparison_markdown(&forecast))
        .await?;
    Ok(NarrativeForecastCreateResult { forecast, forecast_json_path, comparison_path })
}

#[derive(Debug)]
pub struct NarrativeForecastGetResult {
    pub forecast: NarrativeForecast,
    pub stale: bool,
    pub forecast_json_path: String,
    pub comparison_path: String,
}

pub async fn get_narrative_forecast(
    project_root: &Path,
    book_id: &str,
    forecast_id: &str,
) -> Result<NarrativeForecastGetResult, String> {
    let book_id = crate::interaction::session::is_safe_book_id(book_id)
        .then_some(book_id.to_string())
        .ok_or_else(|| format!("Invalid forecast.bookId: \"{book_id}\""))?;
    let book_dir = resolve_book_dir(project_root, &book_id).await?;
    let store = ForecastStore::new(&book_dir);

    let mut forecast = store.load(forecast_id).await?;
    let stale = is_forecast_stale(&book_dir, &book_id, &forecast).await?;
    if stale && forecast.status == ForecastStatus::Active {
        // 持久化 stale 标记，后续读者无需重算。
        forecast = store.mark_stale(&forecast).await?;
    }

    Ok(NarrativeForecastGetResult {
        forecast_json_path: store.forecast_json_path(&forecast.forecast_id)?,
        comparison_path: store.comparison_path(&forecast.forecast_id)?,
        forecast,
        stale,
    })
}

#[derive(Debug)]
pub struct NarrativeForecastSelectResult {
    pub forecast: NarrativeForecast,
    pub branch: ForecastBranch,
    pub stale: bool,
    pub plan_path: String,
}

/// 选择一个分支：只写 selected-branch-plan.md。把计划应用到大纲/章节
/// 意图/正史状态是 v1 之外的独立用户确认操作。
pub async fn select_narrative_branch(
    project_root: &Path,
    book_id: &str,
    forecast_id: &str,
    branch_id: &str,
) -> Result<NarrativeForecastSelectResult, String> {
    let book_id = crate::interaction::session::is_safe_book_id(book_id)
        .then_some(book_id.to_string())
        .ok_or_else(|| format!("Invalid forecast.bookId: \"{book_id}\""))?;
    let book_dir = resolve_book_dir(project_root, &book_id).await?;
    let store = ForecastStore::new(&book_dir);

    let forecast = store.load(forecast_id).await?;
    let branch = forecast
        .branches
        .iter()
        .find(|candidate| candidate.branch_id == branch_id)
        .cloned()
        .ok_or_else(|| {
            format!(
                "Branch \"{branch_id}\" not found in forecast \"{}\". Available branches: {}",
                forecast.forecast_id,
                forecast
                    .branches
                    .iter()
                    .map(|candidate| candidate.branch_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

    let stale = is_forecast_stale(&book_dir, &book_id, &forecast).await?;
    let plan_path = store
        .write_selected_plan(
            &forecast.forecast_id,
            &render_selected_branch_plan_markdown(&forecast, &branch, &utc_now_iso(), stale),
        )
        .await?;
    Ok(NarrativeForecastSelectResult { forecast, branch, stale, plan_path })
}

async fn is_forecast_stale(book_dir: &Path, book_id: &str, forecast: &NarrativeForecast) -> Result<bool, String> {
    if forecast.status == ForecastStatus::Stale {
        return Ok(true);
    }
    let context = build_forecast_context(book_dir, book_id).await;
    Ok(context.context_fingerprint != forecast.context_fingerprint)
}

async fn resolve_book_dir(project_root: &Path, book_id: &str) -> Result<PathBuf, String> {
    let book_dir = project_root.join("books").join(book_id);
    if !book_dir.join("book.json").is_file() {
        return Err(format!(
            "Book \"{book_id}\" not found under {}.",
            project_root.join("books").to_string_lossy()
        ));
    }
    Ok(book_dir)
}

fn bounded_integer(value: Option<u32>, fallback: u32, name: &str, min: u32, max: u32) -> Result<u32, String> {
    let parsed = value.unwrap_or(fallback);
    if parsed < min || parsed > max {
        return Err(format!("{name} must be an integer between {min} and {max}."));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forecast::agent::ForecastChat;
    use crate::llm::provider::LLMMessage;

    struct ScriptChat;

    #[async_trait::async_trait]
    impl ForecastChat for ScriptChat {
        async fn chat(
            &self,
            _messages: Vec<LLMMessage>,
            _temperature: f64,
            _max_tokens: Option<u32>,
        ) -> Result<String, String> {
            Ok(r#"{"branches":[
                {"title":"合作","premise":"接受提议","beats":[{"chapter":2,"summary":"结盟"}],
                 "characterDecisions":[{"character":"主角","decision":"接受"}],
                 "projectedChanges":{"characters":["地位提升"],"relationships":[],"world":[],"hooks":["H01 推进"]},
                 "risks":[{"kind":"causality","description":"动机偏快"}],"uncertainties":["信任边界"],
                 "intentAlignment":{"score":88,"rationale":"贴合意图"}},
                {"title":"对抗","premise":"拒绝提议","beats":[{"chapter":2,"summary":"翻脸"}],
                 "characterDecisions":[],
                 "projectedChanges":{"characters":[],"relationships":[],"world":[],"hooks":[]},
                 "risks":[],"uncertainties":[],
                 "intentAlignment":{"score":72,"rationale":"冲突更强"}}
            ]}"#
                .to_string())
        }
    }

    async fn fixture(root: &Path) {
        let book = root.join("books").join("b1");
        tokio::fs::create_dir_all(book.join("chapters")).await.unwrap();
        tokio::fs::create_dir_all(book.join("story")).await.unwrap();
        tokio::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"测试书","language":"zh"}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(book.join("chapters").join("0001_风起.md"), "# 第1章").await.unwrap();
        tokio::fs::write(book.join("story").join("current_focus.md"), "聚焦：主线推进").await.unwrap();
    }

    #[tokio::test]
    async fn create_get_select_full_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root).await;

        // create：守卫（空分歧/越界分支数）。
        let empty = create_narrative_forecast(&ScriptChat, &CreateNarrativeForecastOptions {
            project_root: &root, book_id: "b1", divergence: "  ", branch_count: None, horizon: None,
        })
        .await
        .unwrap_err();
        assert!(empty.starts_with("divergence is required"));
        let bad_count = create_narrative_forecast(&ScriptChat, &CreateNarrativeForecastOptions {
            project_root: &root, book_id: "b1", divergence: "分歧", branch_count: Some(6), horizon: None,
        })
        .await
        .unwrap_err();
        assert_eq!(bad_count, "branchCount must be an integer between 2 and 5.");
        let missing_book = create_narrative_forecast(&ScriptChat, &CreateNarrativeForecastOptions {
            project_root: &root, book_id: "nope", divergence: "分歧", branch_count: None, horizon: None,
        })
        .await
        .unwrap_err();
        assert!(missing_book.starts_with("Book \"nope\" not found"));

        let created = create_narrative_forecast(&ScriptChat, &CreateNarrativeForecastOptions {
            project_root: &root, book_id: "b1", divergence: "合作还是对抗", branch_count: Some(2), horizon: Some(5),
        })
        .await
        .unwrap();
        let forecast_id = created.forecast.forecast_id.clone();
        assert!(forecast_id.starts_with("fc-"));
        assert_eq!(created.forecast.branches.len(), 2);
        assert_eq!(created.forecast.branches[0].branch_id, "branch-1");
        assert_eq!(created.forecast.base_chapter, 1);
        // 工件落盘。
        let forecast_dir = root.join("books/b1/story/runtime/narrative-forecasts").join(&forecast_id);
        assert!(forecast_dir.join("forecast.json").is_file());
        let comparison = tokio::fs::read_to_string(forecast_dir.join("comparison.md")).await.unwrap();
        assert!(comparison.starts_with("# 叙事推演对比：合作还是对抗"));
        assert!(comparison.contains("| branch-1 | 合作 | 88 | 1 | 接受提议 |"));

        // get：正史未变 → 非 stale。
        let got = get_narrative_forecast(&root, "b1", &forecast_id).await.unwrap();
        assert!(!got.stale);

        // select：写 selected-branch-plan.md。
        let selected = select_narrative_branch(&root, "b1", &forecast_id, "branch-2").await.unwrap();
        assert_eq!(selected.branch.branch_id, "branch-2");
        assert!(!selected.stale);
        let plan = tokio::fs::read_to_string(forecast_dir.join("selected-branch-plan.md")).await.unwrap();
        assert!(plan.starts_with("# 已选分支计划：对抗"));
        assert!(plan.contains("- 分支：branch-2"));
        let branch_error = select_narrative_branch(&root, "b1", &forecast_id, "branch-9").await.unwrap_err();
        assert_eq!(
            branch_error,
            format!(
                "Branch \"branch-9\" not found in forecast \"{forecast_id}\". Available branches: branch-1, branch-2"
            )
        );

        // 正史变化（新章）→ get 检出 stale 并持久化标记。
        tokio::fs::write(root.join("books/b1/chapters").join("0002_夜行.md"), "# 第2章").await.unwrap();
        let stale = get_narrative_forecast(&root, "b1", &forecast_id).await.unwrap();
        assert!(stale.stale);
        assert!(matches!(stale.forecast.status, crate::forecast::schema::ForecastStatus::Stale));
        // stale 后 select 带警告行。
        let selected = select_narrative_branch(&root, "b1", &forecast_id, "branch-1").await.unwrap();
        assert!(selected.stale);
        let plan = tokio::fs::read_to_string(forecast_dir.join("selected-branch-plan.md")).await.unwrap();
        assert!(plan.contains("⚠️ 该推演已过期"));
    }
}
