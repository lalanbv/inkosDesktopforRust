//! R11 场景节拍驱动写作（378 号契约批，三轮 P0）。
//!
//! TS 真源：`packages/core/src/utils/scene-beats.ts`；golden 唯一事实源：
//! `packages/core/src/__tests__/golden/scene-beats-vectors.json`（差分测试
//! `tests/golden_scene_beats_diff.rs` 读同一文件）。
//!
//! 章内 scene 粒度节拍：planner 产出节拍 JSON（title/description/exitHook，
//! clamp ≤6 场景），writer 按节拍顺序推进；开关 `writing.sceneBeats`
//! 默认关（关闭时行为与既有链路一致）。

use serde::Deserialize;

pub const SCENE_BEATS_MAX_SCENES: usize = 6;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneBeat {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_hook: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneBeatPlan {
    pub chapter: i64,
    pub scenes: Vec<SceneBeat>,
}

fn clamp_chars(value: &str, max: usize) -> String {
    if value.chars().count() > max {
        value.chars().take(max).collect()
    } else {
        value.to_string()
    }
}

/// 解析 planner 产出的场景节拍计划（围栏剥离 + 首个 JSON 对象；
/// scenes clamp ≤6；title/description 均空的行跳过；全部无效 → None）。
pub fn parse_scene_beat_plan(content: &str, chapter: i64) -> Option<SceneBeatPlan> {
    let trimmed = content.trim();
    let fenced = regex::Regex::new(r"(?i)```(?:json)?\s*([\s\S]*?)\s*```")
        .ok()
        .and_then(|re| re.captures(trimmed))
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().trim())
        .unwrap_or(trimmed);
    let start = fenced.find('{')?;
    let value: serde_json::Value = serde_json::from_str(&fenced[start..]).ok()?;
    let rows = value.get("scenes")?.as_array()?;
    let mut scenes: Vec<SceneBeat> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if scenes.len() >= SCENE_BEATS_MAX_SCENES {
            break;
        }
        let title = row
            .get("title")
            .and_then(serde_json::Value::as_str)
            .map(|value| clamp_chars(value.trim(), 60))
            .unwrap_or_default();
        let description = row
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(|value| clamp_chars(value.trim(), 200))
            .unwrap_or_default();
        if title.is_empty() && description.is_empty() {
            continue;
        }
        let exit_hook = row
            .get("exitHook")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| clamp_chars(value, 120));
        scenes.push(SceneBeat {
            id: row
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("s{}", index + 1)),
            title,
            description,
            exit_hook,
        });
    }
    if scenes.is_empty() {
        return None;
    }
    Some(SceneBeatPlan { chapter, scenes })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneBeatsLanguage {
    Zh,
    En,
}

/// planner 提示：产出场景节拍 JSON（zh/en 双语）。
pub fn build_scene_beats_prompt(
    goal: &str,
    outline_node: Option<&str>,
    scene_count: usize,
    language: SceneBeatsLanguage,
) -> String {
    let is_en = language == SceneBeatsLanguage::En;
    let outline = match outline_node {
        Some(node) if is_en => format!("\nOutline: {node}"),
        Some(node) => format!("\n大纲：{node}"),
        None => String::new(),
    };
    if is_en {
        format!(
            "Break this chapter into {scene_count} scene beats.\n\nChapter goal: {goal}{outline}\nOutput JSON: {{\"scenes\":[{{\"id\":\"s1\",\"title\":\"...\",\"description\":\"what happens in this scene (concrete, actionable)\",\"exitHook\":\"optional line that pushes into the next scene\"}}]}}"
        )
    } else {
        format!(
            "把本章拆分为 {scene_count} 个场景节拍。\n\n章节目标：{goal}{outline}\n输出 JSON：{{\"scenes\":[{{\"id\":\"s1\",\"title\":\"...\",\"description\":\"本场景发生什么（具体、可执行）\",\"exitHook\":\"推向下一场景的出口钩（可选）\"}}]}}"
        )
    }
}

/// writer 注入块：按节拍顺序推进的显式纪律（zh/en 双语）。
pub fn build_scene_beats_writer_block(
    plan: &SceneBeatPlan,
    language: SceneBeatsLanguage,
) -> String {
    let is_en = language == SceneBeatsLanguage::En;
    let header = if is_en {
        "## Scene beats (write in this order)"
    } else {
        "## 场景节拍（按此顺序推进）"
    };
    let mut lines: Vec<String> = Vec::new();
    for scene in &plan.scenes {
        lines.push(format!("- {}: {}", scene.title, scene.description));
        if let Some(exit_hook) = &scene.exit_hook {
            let label = if is_en { "exit" } else { "出口" };
            lines.push(format!("  {label}: {exit_hook}"));
        }
    }
    let rule = if is_en {
        "Each beat must land before the next begins; do not skip or merge beats."
    } else {
        "每个节拍必须落实后才进入下一拍；不得跳过或合并节拍。"
    };
    lines.push(rule.to_string());
    let mut parts = vec![header.to_string()];
    parts.extend(lines);
    parts.join("\n")
}
