//! 自动导演契约（G6/352 号，Phase C 末件首批）。
//!
//! TS 真源：`packages/core/src/models/director.ts`；共享向量：
//! `packages/core/src/__tests__/golden/director-vectors.json`
//! （差分测试 `tests/golden_director_diff.rs`）。
//!
//! 灵感卡 / 方向候选批量生成（prompt 构建+容错解析+标题组重做剔除）/
//! 三运行模式执行计划 / 驾驶舱阶段推进决策表，语义详见 TS 模块 doc。

use serde::Deserialize;
use serde::Serialize;

pub const DIRECTOR_RUN_MODES: [&str; 3] = ["ready-stop", "range", "full-book"];

pub const DIRECTOR_STAGES: [&str; 6] = [
    "inspiration",
    "directions",
    "outline",
    "writing",
    "paused",
    "done",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DirectorRunMode {
    ReadyStop,
    Range,
    FullBook,
}

impl DirectorRunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            DirectorRunMode::ReadyStop => "ready-stop",
            DirectorRunMode::Range => "range",
            DirectorRunMode::FullBook => "full-book",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DirectorStage {
    Inspiration,
    Directions,
    Outline,
    Writing,
    Paused,
    Done,
}

impl DirectorStage {
    pub fn as_str(self) -> &'static str {
        match self {
            DirectorStage::Inspiration => "inspiration",
            DirectorStage::Directions => "directions",
            DirectorStage::Outline => "outline",
            DirectorStage::Writing => "writing",
            DirectorStage::Paused => "paused",
            DirectorStage::Done => "done",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspirationCard {
    pub premise: String,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub tone: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectionCandidate {
    pub id: String,
    pub title: String,
    pub hook: String,
    pub genre: String,
    pub synopsis: String,
    pub differentiator: String,
    pub confidence: f64,
}

/// 方向候选批量生成 prompt（count 默认 3；exclude_titles = 标题组重做剔除）。
pub fn build_direction_candidates_prompt(
    inspiration: &InspirationCard,
    count: usize,
    exclude_titles: &[String],
    language: Option<&str>,
) -> String {
    let is_en = language == Some("en");
    let i = inspiration;
    let mut optional = String::new();
    if let Some(genre) = &i.genre {
        optional.push_str(&if is_en {
            format!("\n- Genre: {genre}")
        } else {
            format!("\n- 题材：{genre}")
        });
    }
    if let Some(platform) = &i.platform {
        optional.push_str(&if is_en {
            format!("\n- Platform: {platform}")
        } else {
            format!("\n- 平台：{platform}")
        });
    }
    if let Some(tone) = &i.tone {
        optional.push_str(&if is_en {
            format!("\n- Tone: {tone}")
        } else {
            format!("\n- 基调：{tone}")
        });
    }
    if !i.keywords.is_empty() {
        optional.push_str(&if is_en {
            format!("\n- Keywords: {}", i.keywords.join(", "))
        } else {
            format!("\n- 关键词：{}", i.keywords.join("、"))
        });
    }
    let exclude_block = if !exclude_titles.is_empty() {
        if is_en {
            format!("\nExcluded titles (do not reuse): {}\n", exclude_titles.join(", "))
        } else {
            format!("\n已排除标题（不得复用）：{}\n", exclude_titles.join("、"))
        }
    } else {
        String::new()
    };
    if is_en {
        format!(
            "You are the story director. Generate {count} alternative book directions from the same inspiration.\n\n## Inspiration\n- Premise: {}{optional}{exclude_block}\nOutput JSON: {{\"directions\":[{{\"id\":\"d1\",\"title\":\"...\",\"hook\":\"one-line hook\",\"genre\":\"...\",\"synopsis\":\"2-3 sentences\",\"differentiator\":\"how it avoids sameness\",\"confidence\":0.0-1.0}}]}}",
            i.premise
        )
    } else {
        format!(
            "你是故事导演。基于同一份灵感，生成 {count} 套并列的开书方向。\n\n## 灵感卡\n- 灵感：{}{optional}{exclude_block}\n输出 JSON：{{\"directions\":[{{\"id\":\"d1\",\"title\":\"书名\",\"hook\":\"一句话钩子\",\"genre\":\"题材\",\"synopsis\":\"两三句简介\",\"differentiator\":\"差异化（如何避开同质化）\",\"confidence\":0.0-1.0}}]}}",
            i.premise
        )
    }
}

/// 解析方向候选（容错：首个 JSON 块），剔除 excludeTitles 重叠标题（标题组重做），
/// confidence 降序；缺失/空 → 空数组不抛错。
pub fn parse_direction_candidates(
    content: &str,
    exclude_titles: &[String],
) -> Vec<DirectionCandidate> {
    let start = match content.find('{') {
        Some(index) => index,
        None => return Vec::new(),
    };
    let end = match content.rfind('}') {
        Some(index) => index,
        None => return Vec::new(),
    };
    if end <= start {
        return Vec::new();
    }
    let parsed: Value = match serde_json::from_str(&content[start..=end]) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let rows = match parsed.get("directions").and_then(Value::as_array) {
        Some(rows) => rows.clone(),
        None => return Vec::new(),
    };
    let excluded: Vec<String> = exclude_titles
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let mut out: Vec<DirectionCandidate> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let title = row.get("title").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if title.is_empty() {
            continue;
        }
        if excluded.contains(&title.to_lowercase()) {
            continue;
        }
        let default_id = format!("d{}", index + 1);
        out.push(DirectionCandidate {
            id: row
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .unwrap_or(&default_id)
                .to_string(),
            title,
            hook: row.get("hook").and_then(Value::as_str).unwrap_or("").to_string(),
            genre: row.get("genre").and_then(Value::as_str).unwrap_or("").to_string(),
            synopsis: row.get("synopsis").and_then(Value::as_str).unwrap_or("").to_string(),
            differentiator: row
                .get("differentiator")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            confidence: row.get("confidence").and_then(Value::as_f64).unwrap_or(0.5),
        });
    }
    out.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.title.cmp(&b.title))
    });
    out
}

use serde_json::Value;

/// 运行模式 → 执行计划。
pub fn resolve_director_run_plan(
    mode: DirectorRunMode,
    from_chapter: Option<i64>,
    to_chapter: Option<i64>,
    target_chapters: i64,
) -> (DirectorRunMode, i64, Option<i64>, bool) {
    let from = from_chapter.unwrap_or(1).max(1);
    match mode {
        DirectorRunMode::ReadyStop => (mode, from, None, true),
        DirectorRunMode::Range => {
            let to = to_chapter.unwrap_or(target_chapters).clamp(from, target_chapters);
            (mode, from, Some(to), false)
        }
        DirectorRunMode::FullBook => {
            let to = target_chapters.max(from);
            (mode, from, Some(to), false)
        }
    }
}

/// 驾驶舱阶段推进决策表。
pub fn next_director_stage(
    current: DirectorStage,
    mode: DirectorRunMode,
    written_chapters: i64,
    to_chapter: i64,
) -> DirectorStage {
    match current {
        DirectorStage::Inspiration => DirectorStage::Directions,
        DirectorStage::Directions => {
            if mode == DirectorRunMode::ReadyStop {
                DirectorStage::Paused
            } else {
                DirectorStage::Outline
            }
        }
        DirectorStage::Outline => DirectorStage::Writing,
        DirectorStage::Writing => {
            if written_chapters >= to_chapter {
                if mode == DirectorRunMode::FullBook {
                    DirectorStage::Done
                } else {
                    DirectorStage::Paused
                }
            } else {
                DirectorStage::Writing
            }
        }
        other => other,
    }
}

/// 机器可读契约（双端 golden 锁形状）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorContract {
    pub run_modes: Vec<String>,
    pub stages: Vec<String>,
    pub ready_stop_stops_after_directions: bool,
    pub manual_edit_never_auto_overwritten: bool,
}

pub fn director_contract() -> DirectorContract {
    DirectorContract {
        run_modes: DIRECTOR_RUN_MODES.iter().map(|mode| mode.to_string()).collect(),
        stages: DIRECTOR_STAGES.iter().map(|stage| stage.to_string()).collect(),
        ready_stop_stops_after_directions: true,
        manual_edit_never_auto_overwritten: true,
    }
}
