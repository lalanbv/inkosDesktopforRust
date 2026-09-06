//! 系列书回填数据面（184 号 C3-b）。对标 Sudowrite Story Bible 的
//! 「系列书从既有正文回填设定」，交互按 179 号分解文档默认「预览 + 逐项勾选」
//! ——用户不确认则零写入。
//!
//! - 抽取：读源书 canon（story_bible / story_frame / roles/**），经 LLM 摘要为
//!   结构化 items 落盘 `books/{目标}/story/series_backfill_draft.json`；
//! - 回填：勾选的 items 渲染为 `books/{目标}/story/series_backfill.md`
//!   （独立文件，不碰既有真相文件——用户可自行合并）。

use serde::{Deserialize, Serialize};

/// 回填条目类别（宽松 String：LLM 输出未知值不致整文档失败，UI 分组展示）。
pub const BACKFILL_CATEGORIES: &[&str] = &["worldview", "character", "plot", "style"];

/// 单条可回填设定。对齐 TS `SeriesBackfillItem`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SeriesBackfillItem {
    pub id: String,
    pub category: String,
    pub title: String,
    pub content: String,
}

/// 抽取草稿（落盘 `story/series_backfill_draft.json`）。对齐 TS 同名 schema。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SeriesBackfillDraft {
    pub version: u32,
    pub book_id: String,
    pub source_book_id: String,
    pub updated_at: String,
    pub items: Vec<SeriesBackfillItem>,
}

impl SeriesBackfillDraft {
    pub const VERSION: u32 = 1;

    /// 从 LLM 文本宽松提取 items JSON：截取首个 `{` 到末个 `}` 再解析
    /// （LLM 常见 ```json 围栏/前后缀说明）。
    pub fn parse_items_text(book_id: &str, source_book_id: &str, text: &str) -> Result<Self, String> {
        let Some(start) = text.find('{') else {
            return Err("LLM output contains no JSON object".into());
        };
        let Some(end) = text.rfind('}') else {
            return Err("LLM output contains no JSON object".into());
        };
        let json_text = &text[start..=end];
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawItems {
            items: Vec<SeriesBackfillItem>,
        }
        let raw: RawItems = serde_json::from_str(json_text)
            .map_err(|e| format!("LLM items JSON 不可解析: {e}"))?;
        if raw.items.is_empty() {
            return Err("LLM returned zero backfill items".into());
        }
        Ok(Self {
            version: Self::VERSION,
            book_id: book_id.to_string(),
            source_book_id: source_book_id.to_string(),
            // 192 号：ISO 8601（与 Node extract 的 toISOString 对齐）。此前为
            // utc_now_ms()——epoch 毫秒会原样渲染进 series_backfill.md 头部的
            // 「抽取于 …」，用户不可读。
            updated_at: crate::utils::utc_time::utc_now_iso(),
            items: raw.items,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_roundtrip_and_defaults() {
        let json_text = r#"{
            "version": 1,
            "bookId": "target",
            "sourceBookId": "source",
            "updatedAt": "2026-09-07T00:00:00.000Z",
            "items": [
                { "id": "it-1", "category": "worldview", "title": "元气体系", "content": "……" }
            ]
        }"#;
        let draft: SeriesBackfillDraft = serde_json::from_str(json_text).unwrap();
        assert_eq!(draft.version, 1);
        assert_eq!(draft.items[0].category, "worldview");
        let out = serde_json::to_value(&draft).unwrap();
        assert_eq!(out["items"][0]["id"], "it-1");
    }

    #[test]
    fn parse_items_text_extracts_json_from_fenced_output() {
        let text = "以下是抽取结果：\n```json\n{\"items\": [{\"id\": \"a\", \"category\": \"plot\", \"title\": \"主线冲突\", \"content\": \"宗门之争\"}]}\n```\n请审阅。";
        let draft = SeriesBackfillDraft::parse_items_text("target", "source", text).unwrap();
        assert_eq!(draft.items.len(), 1);
        assert_eq!(draft.items[0].id, "a");
        assert_eq!(draft.book_id, "target");
        assert_eq!(draft.source_book_id, "source");
        // 192 号：updatedAt 必须是 ISO 8601（此前为 epoch 毫秒字符串，
        // 会原样渲染进 apply 文件头部的「抽取于 …」）。
        assert!(
            draft.updated_at.ends_with('Z')
                && draft.updated_at.contains('T')
                && draft.updated_at.starts_with("20"),
            "updatedAt 应为 ISO 8601，实际: {}",
            draft.updated_at
        );
    }

    #[test]
    fn parse_items_text_rejects_empty_and_garbage() {
        assert!(SeriesBackfillDraft::parse_items_text("t", "s", "没有 JSON").is_err());
        assert!(SeriesBackfillDraft::parse_items_text("t", "s", "{\"items\": []}").is_err());
    }
}
