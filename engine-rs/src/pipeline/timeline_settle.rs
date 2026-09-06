//! 时间线节拍自动沉淀编排（189 号）。write-next 落盘后调用：
//! 读 `story/timeline.json` → 既有线条名册 → LLM 提取本章节拍 → 原位合并 → 落盘。
//! 无时间线 / 无线条 / 提取失败均为非致命（Ok 或 Err 交调用方告警），不影响章节产物。

use std::path::Path;

use crate::agents::timeline_settler::{BeatsRequest, TimelineBeatsChat};
use crate::models::timeline::{merge_chapter_beats, Timeline};
use crate::utils::language::WritingLanguage;

/// 沉淀结果：`None` = 无时间线可沉淀（静默跳过）；`Some(n)` = 落格 n 条线条。
pub async fn settle_beats_for_chapter(
    book_dir: &Path,
    chat: &dyn TimelineBeatsChat,
    chapter_number: u32,
    chapter_title: &str,
    chapter_summary: &str,
    language: WritingLanguage,
) -> Result<Option<usize>, String> {
    let path = book_dir.join("story").join("timeline.json");
    let Ok(raw) = tokio::fs::read_to_string(&path).await else {
        return Ok(None);
    };
    let mut timeline: Timeline = match serde_json::from_str(&raw) {
        Ok(timeline) => timeline,
        Err(error) => return Err(format!("timeline.json 不可解析，跳过节拍沉淀: {error}")),
    };
    if timeline.plotlines.is_empty() {
        return Ok(None);
    }
    let roster: Vec<(String, String)> = timeline
        .plotlines
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    let beats = chat
        .beats(BeatsRequest {
            chapter_number,
            chapter_title,
            chapter_summary,
            plotlines: &roster,
            language,
        })
        .await?;
    let applied = merge_chapter_beats(&mut timeline, chapter_number, &beats);
    if applied == 0 {
        return Ok(Some(0));
    }
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let mut serialized = serde_json::to_string_pretty(&timeline).map_err(|e| e.to_string())?;
    serialized.push('\n');
    tokio::fs::write(&path, serialized).await.map_err(|e| format!("timeline.json 写入失败: {e}"))?;
    Ok(Some(applied))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::timeline::PlotlineBeat;
    use std::sync::Mutex;

    type BeatRequests = Mutex<Vec<(u32, Vec<(String, String)>)>>;

    struct MockBeats {
        result: Result<Vec<PlotlineBeat>, String>,
        requests: BeatRequests,
    }

    #[async_trait::async_trait]
    impl TimelineBeatsChat for MockBeats {
        async fn beats(&self, req: BeatsRequest<'_>) -> Result<Vec<PlotlineBeat>, String> {
            self.requests.lock().unwrap().push((
                req.chapter_number,
                req.plotlines.to_vec(),
            ));
            self.result.clone()
        }
    }

    fn beat(id: &str, title: &str) -> PlotlineBeat {
        PlotlineBeat { plotline_id: id.into(), title: Some(title.into()), note: Some("节拍".into()) }
    }

    async fn write_timeline(dir: &Path, body: &str) {
        let story = dir.join("story");
        tokio::fs::create_dir_all(&story).await.unwrap();
        tokio::fs::write(story.join("timeline.json"), body).await.unwrap();
    }

    fn timeline_json() -> String {
        format!(
            r#"{{"version":1,"bookId":"b1","updatedAt":"{}","plotlines":[{{"id":"main","name":"主线","cells":[]}}]}}"#,
            crate::utils::utc_time::utc_now_iso()
        )
    }

    #[tokio::test]
    async fn settles_beats_into_existing_timeline() {
        let dir = tempfile::tempdir().unwrap();
        write_timeline(dir.path(), &timeline_json()).await;
        let mock = MockBeats {
            result: Ok(vec![beat("main", "风起")]),
            requests: Mutex::new(Vec::new()),
        };
        let applied = settle_beats_for_chapter(
            dir.path(),
            &mock,
            1,
            "风起",
            "主角入场",
            WritingLanguage::Zh,
        )
        .await
        .unwrap();
        assert_eq!(applied, Some(1));
        // 名册透传给端口（测试传参 = 生产传参：id+name 全量）。
        assert_eq!(mock.requests.lock().unwrap()[0].1, vec![("main".into(), "主线".into())]);
        let raw = tokio::fs::read_to_string(dir.path().join("story/timeline.json")).await.unwrap();
        let timeline: Timeline = serde_json::from_str(&raw).unwrap();
        assert_eq!(timeline.plotlines[0].cells[0].title.as_deref(), Some("风起"));
    }

    #[tokio::test]
    async fn missing_timeline_is_silent_skip() {
        let dir = tempfile::tempdir().unwrap();
        let mock = MockBeats { result: Ok(vec![]), requests: Mutex::new(Vec::new()) };
        let applied = settle_beats_for_chapter(dir.path(), &mock, 1, "t", "s", WritingLanguage::Zh)
            .await
            .unwrap();
        assert_eq!(applied, None);
        assert!(mock.requests.lock().unwrap().is_empty()); // 未发起 LLM 调用
    }

    #[tokio::test]
    async fn empty_plotlines_is_silent_skip() {
        let dir = tempfile::tempdir().unwrap();
        write_timeline(
            dir.path(),
            r#"{"version":1,"bookId":"b1","updatedAt":"t","plotlines":[]}"#,
        )
        .await;
        let mock = MockBeats { result: Ok(vec![]), requests: Mutex::new(Vec::new()) };
        let applied = settle_beats_for_chapter(dir.path(), &mock, 1, "t", "s", WritingLanguage::Zh)
            .await
            .unwrap();
        assert_eq!(applied, None);
    }

    #[tokio::test]
    async fn unparseable_timeline_is_error_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        write_timeline(dir.path(), "{ broken").await;
        let mock = MockBeats { result: Ok(vec![]), requests: Mutex::new(Vec::new()) };
        let result = settle_beats_for_chapter(dir.path(), &mock, 1, "t", "s", WritingLanguage::Zh).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn chat_failure_propagates_for_caller_warning() {
        let dir = tempfile::tempdir().unwrap();
        write_timeline(dir.path(), &timeline_json()).await;
        let mock = MockBeats { result: Err("llm down".into()), requests: Mutex::new(Vec::new()) };
        let result = settle_beats_for_chapter(dir.path(), &mock, 1, "t", "s", WritingLanguage::Zh).await;
        assert_eq!(result.unwrap_err(), "llm down");
        // 失败不改写原文件（仍为空 cells）。
        let raw = tokio::fs::read_to_string(dir.path().join("story/timeline.json")).await.unwrap();
        let timeline: Timeline = serde_json::from_str(&raw).unwrap();
        assert!(timeline.plotlines[0].cells.is_empty());
    }
}
