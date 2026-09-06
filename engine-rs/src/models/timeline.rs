//! 书籍时间线数据面（181 号 C4-b）。对齐 TS `packages/core/src/models/timeline.ts`。
//!
//! - 落盘位置：`books/{id}/story/timeline.json`
//! - cells 稀疏存储：只为有 beats 的章建条目
//! - 生成端点默认关闭（待产品决策）；数据当前来自 PUT 端点（C4-c 编辑回写）

use serde::{Deserialize, Serialize};

/// 单章节拍。对齐 TS `TimelineCellSchema`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineCell {
    pub chapter: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 情节线。对齐 TS `TimelinePlotlineSchema`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePlotline {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub cells: Vec<TimelineCell>,
}

/// TS `z.literal(1)` 的 serde 等价物：反序列化只接受 1，序列化恒输出 1。
fn deserialize_v1<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    if value == 1 {
        Ok(1)
    } else {
        Err(serde::de::Error::custom("timeline.version must be 1"))
    }
}

/// 时间线文档（version 固定 1）。对齐 TS `TimelineSchema`（plotline id 唯一性
/// 由端点层校验，serde 不表达）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    #[serde(deserialize_with = "deserialize_v1")]
    pub version: u32,
    pub book_id: String,
    pub updated_at: String,
    #[serde(default)]
    pub plotlines: Vec<TimelinePlotline>,
}

impl Timeline {
    pub const VERSION: u32 = 1;

    /// plotline id 是否唯一（TS 侧 refine 的等价校验）。
    pub fn ids_unique(&self) -> bool {
        let ids: Vec<&str> = self.plotlines.iter().map(|p| p.id.as_str()).collect();
        ids.len() == ids.iter().collect::<std::collections::HashSet<_>>().len()
    }
}

/// 单条沉淀节拍（189 号：write-next 落盘后自动回写）。
#[derive(Debug, Clone, PartialEq)]
pub struct PlotlineBeat {
    pub plotline_id: String,
    pub title: Option<String>,
    pub note: Option<String>,
}

/// 把某章的节拍合并进时间线：命中既有 cell 则原位替换，否则按章号升序插入；
/// 未知 plotline id 忽略。返回实际落格的线条数。
pub fn merge_chapter_beats(timeline: &mut Timeline, chapter: u32, beats: &[PlotlineBeat]) -> usize {
    let mut applied = 0usize;
    for beat in beats {
        let Some(line) = timeline.plotlines.iter_mut().find(|p| p.id == beat.plotline_id) else {
            continue;
        };
        let cell = TimelineCell {
            chapter,
            title: beat.title.clone().filter(|t| !t.trim().is_empty()),
            note: beat.note.clone().filter(|n| !n.trim().is_empty()),
        };
        match line.cells.iter().position(|c| c.chapter == chapter) {
            Some(idx) => line.cells[idx] = cell,
            None => {
                let pos = line.cells.iter().take_while(|c| c.chapter < chapter).count();
                line.cells.insert(pos, cell);
            }
        }
        applied += 1;
    }
    if applied > 0 {
        timeline.updated_at = crate::utils::utc_time::utc_now_iso();
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_json() -> String {
        format!(
            r#"{{"version":1,"bookId":"b1","updatedAt":"{ts}","plotlines":[{{"id":"main","name":"主线","cells":[{{"chapter":1,"title":"风起","note":"主角入场"}}]}}]}}"#,
            ts = "2026-09-07T00:00:00.000Z"
        )
    }

    #[test]
    fn timeline_roundtrip_and_defaults() {
        let timeline: Timeline = serde_json::from_str(&base_json()).unwrap();
        assert_eq!(timeline.version, 1);
        assert_eq!(timeline.plotlines.len(), 1);
        assert_eq!(timeline.plotlines[0].cells[0].chapter, 1);
        assert_eq!(timeline.plotlines[0].cells[0].title.as_deref(), Some("风起"));
        assert!(timeline.plotlines[0].cells[0].note.is_some());

        // cells 缺省：省略 plotlines 内 cells → 反序列化为空。
        let minimal = r#"{"version":1,"bookId":"b1","updatedAt":"t","plotlines":[{"id":"a","name":"A"}]}"#;
        let timeline: Timeline = serde_json::from_str(minimal).unwrap();
        assert!(timeline.plotlines[0].cells.is_empty());

        // roundtrip：cells 为空数组时照常输出（与 TS default([]) 消费端兼容）。
        let json = serde_json::to_value(&timeline).unwrap();
        assert_eq!(json["plotlines"][0]["cells"], serde_json::json!([]));
    }

    #[test]
    fn ids_unique_detection() {
        let mut timeline: Timeline = serde_json::from_str(&base_json()).unwrap();
        assert!(timeline.ids_unique());
        timeline.plotlines.push(TimelinePlotline {
            id: "main".into(),
            name: "重复".into(),
            cells: vec![],
        });
        assert!(!timeline.ids_unique());
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let bad = base_json().replace("\"version\":1", "\"version\":2");
        assert!(serde_json::from_str::<Timeline>(&bad).is_err());
    }

    fn doc() -> Timeline {
        serde_json::from_str(&base_json()).unwrap()
    }

    fn beat(id: &str, title: Option<&str>, note: Option<&str>) -> PlotlineBeat {
        PlotlineBeat {
            plotline_id: id.to_string(),
            title: title.map(str::to_string),
            note: note.map(str::to_string),
        }
    }

    #[test]
    fn merge_beats_replaces_existing_cell() {
        let mut timeline = doc();
        let applied = merge_chapter_beats(
            &mut timeline,
            1,
            &[beat("main", Some("风起（改）"), Some("更新后的节拍"))],
        );
        assert_eq!(applied, 1);
        let cell = &timeline.plotlines[0].cells[0];
        assert_eq!(cell.chapter, 1);
        assert_eq!(cell.title.as_deref(), Some("风起（改）"));
        assert_eq!(timeline.plotlines[0].cells.len(), 1);
    }

    #[test]
    fn merge_beats_inserts_in_chapter_order() {
        let mut timeline = doc();
        let applied = merge_chapter_beats(
            &mut timeline,
            3,
            &[beat("main", Some("高潮"), Some("第三章"))],
        );
        assert_eq!(applied, 1);
        let applied2 = merge_chapter_beats(
            &mut timeline,
            2,
            &[beat("main", Some("推进"), None)],
        );
        assert_eq!(applied2, 1);
        let chapters: Vec<u32> = timeline.plotlines[0].cells.iter().map(|c| c.chapter).collect();
        assert_eq!(chapters, vec![1, 2, 3]);
        // 空标题/空 note 净化为 None。
        assert_eq!(timeline.plotlines[0].cells[1].note, None);
    }

    #[test]
    fn merge_beats_ignores_unknown_plotline_and_empty_beats() {
        let mut timeline = doc();
        let applied = merge_chapter_beats(
            &mut timeline,
            2,
            &[beat("ghost", Some("不存在"), None), beat("main", Some("第二章"), None)],
        );
        assert_eq!(applied, 1);
        assert!(timeline.plotlines.iter().all(|p| p.id != "ghost"));
        assert_eq!(merge_chapter_beats(&mut timeline, 4, &[]), 0);
    }
}
