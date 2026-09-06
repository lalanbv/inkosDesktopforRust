//! 时间线节拍自动沉淀（189 号）。write-next 落盘后按既有情节线为本章补节拍：
//! 书籍级设置 `writing.autoTimelineBeats`（默认关）；LLM 失败/解析失败仅告警，
//! 不影响章节产物（章节已落盘，沉淀属事后增强）。

use std::collections::HashSet;
use std::sync::Arc;

use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::timeline::PlotlineBeat;
use crate::utils::language::WritingLanguage;

/// 节拍提取入参（章节元信息 + 既有线条名册）。
pub struct BeatsRequest<'a> {
    pub chapter_number: u32,
    pub chapter_title: &'a str,
    pub chapter_summary: &'a str,
    /// 既有情节线（id, name）——模型只允许在其中挑选归属。
    pub plotlines: &'a [(String, String)],
    pub language: WritingLanguage,
}

/// 节拍提取端口（生产 = AgentRouter 泛化 chat；测试 = mock）。
#[async_trait::async_trait]
pub trait TimelineBeatsChat: Send + Sync {
    async fn beats(&self, req: BeatsRequest<'_>) -> Result<Vec<PlotlineBeat>, String>;
}

/// 构造（system, user）双消息 prompt。
pub fn build_beats_prompt(req: &BeatsRequest<'_>) -> (String, String) {
    let system = if req.language == WritingLanguage::En {
        "You are a novel story-grid editor. Output pure JSON only — no markdown fences, no commentary.".to_string()
    } else {
        "你是网文时间线编辑。只输出纯 JSON，不要 markdown 围栏或任何解释文字。".to_string()
    };
    let mut roster = String::new();
    for (id, name) in req.plotlines {
        roster.push_str(&format!("- {id}: {name}\n"));
    }
    let user = if req.language == WritingLanguage::En {
        format!(
            "Chapter {} \"{}\" was just written. Summary:\n{}\n\nPlotlines:\n{roster}\n\
             For each plotline, decide whether this chapter advances it. Output pure JSON:\n\
             {{\"beats\":[{{\"plotlineId\":\"<id from roster>\",\"title\":\"<=12 chars beat title\",\"note\":\"one-sentence beat note\"}}]}}\n\
             Only include plotlines this chapter actually touches; omit the rest.",
            req.chapter_number, req.chapter_title, req.chapter_summary
        )
    } else {
        format!(
            "第 {} 章《{}》刚完成写作。章节梗概：\n{}\n\n情节线名册：\n{roster}\n\
             请为每条情节线判断本章是否推进了它，并输出纯 JSON：\n\
             {{\"beats\":[{{\"plotlineId\":\"<名册中的 id>\",\"title\":\"12字以内的节拍标题\",\"note\":\"一句话节拍说明\"}}]}}\n\
             只输出本章真正推进的情节线；未推进的不要输出。",
            req.chapter_number, req.chapter_title, req.chapter_summary
        )
    };
    (system, user)
}

/// 宽松解析模型输出：剥 markdown 围栏、容错引号包裹的 JSON、
/// 过滤未知 plotline id 与空节拍（title/note 皆空视为无效）。
pub fn parse_beats_json(text: &str, valid_ids: &HashSet<String>) -> Result<Vec<PlotlineBeat>, String> {
    let trimmed = text.trim();
    let stripped = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .strip_suffix("```")
        .unwrap_or(trimmed)
        .trim();
    let value: serde_json::Value = serde_json::from_str(stripped).map_err(|e| format!("beats JSON parse failed: {e}"))?;
    let Some(items) = value.get("beats").and_then(serde_json::Value::as_array) else {
        return Err("beats JSON missing `beats` array".to_string());
    };
    let mut beats = Vec::new();
    for item in items {
        let Some(id) = item.get("plotlineId").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !valid_ids.contains(id) {
            continue;
        }
        let title = item.get("title").and_then(serde_json::Value::as_str).map(str::to_string);
        let note = item.get("note").and_then(serde_json::Value::as_str).map(str::to_string);
        let is_blank = |s: Option<&str>| s.is_none_or(|v| v.trim().is_empty());
        if is_blank(title.as_deref()) && is_blank(note.as_deref()) {
            continue;
        }
        beats.push(PlotlineBeat {
            plotline_id: id.to_string(),
            title,
            note,
        });
    }
    Ok(beats)
}

/// 生产适配器：AgentRouter 泛化 chat（"inspiration" agent 档位，低温度小输出）。
pub struct RouterTimelineBeatsChat {
    pub router: Arc<crate::llm::agent_router::AgentRouter>,
}

#[async_trait::async_trait]
impl TimelineBeatsChat for RouterTimelineBeatsChat {
    async fn beats(&self, req: BeatsRequest<'_>) -> Result<Vec<PlotlineBeat>, String> {
        let (system, user) = build_beats_prompt(&req);
        let valid_ids: HashSet<String> = req.plotlines.iter().map(|(id, _)| id.clone()).collect();
        let outcome = self
            .router
            .chat(
                "inspiration",
                vec![
                    LLMMessage { role: LLMRole::System, content: system, tool_calls: None, tool_call_id: None },
                    LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
                ],
                0.3,
                Some(1_500),
            )
            .await
            .map_err(|e| e.to_string())?;
        parse_beats_json(&outcome.content, &valid_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Vec<(String, String)> {
        vec![("main".to_string(), "主线".to_string()), ("sub".to_string(), "副线".to_string())]
    }

    fn ids() -> HashSet<String> {
        roster().into_iter().map(|(id, _)| id).collect()
    }

    #[test]
    fn prompt_lists_roster_and_chapter() {
        let req = BeatsRequest {
            chapter_number: 3,
            chapter_title: "风起",
            chapter_summary: "主角入场",
            plotlines: &roster(),
            language: WritingLanguage::Zh,
        };
        let (system, user) = build_beats_prompt(&req);
        assert!(system.contains("纯 JSON"));
        assert!(user.contains("main: 主线"));
        assert!(user.contains("第 3 章"));
        assert!(user.contains("plotlineId"));
    }

    #[test]
    fn prompt_has_english_variant() {
        let req = BeatsRequest {
            chapter_number: 2,
            chapter_title: "Storm",
            chapter_summary: "The hero arrives.",
            plotlines: &roster(),
            language: WritingLanguage::En,
        };
        let (system, user) = build_beats_prompt(&req);
        assert!(system.contains("pure JSON"));
        assert!(!user.contains("第 2 章"));
    }

    #[test]
    fn parse_accepts_plain_and_fenced_json() {
        let plain = r#"{"beats":[{"plotlineId":"main","title":"风起","note":"主角入场"}]}"#;
        assert_eq!(parse_beats_json(plain, &ids()).unwrap().len(), 1);

        let fenced = format!("```json\n{plain}\n```");
        assert_eq!(parse_beats_json(&fenced, &ids()).unwrap().len(), 1);
    }

    #[test]
    fn parse_filters_unknown_ids_and_empty_beats() {
        let text = r#"{"beats":[
            {"plotlineId":"ghost","title":"幽灵","note":"未知线条"},
            {"plotlineId":"main","title":"","note":"  "},
            {"plotlineId":"sub","title":"副线推进","note":"配角入场"}
        ]}"#;
        let beats = parse_beats_json(text, &ids()).unwrap();
        assert_eq!(beats.len(), 1);
        assert_eq!(beats[0].plotline_id, "sub");
        assert_eq!(beats[0].title.as_deref(), Some("副线推进"));
    }

    #[test]
    fn parse_rejects_structural_garbage() {
        assert!(parse_beats_json("not json at all", &ids()).is_err());
        assert!(parse_beats_json(r#"{"items":[]}"#, &ids()).is_err());
    }
}
