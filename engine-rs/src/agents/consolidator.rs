//! consolidator —— 已完结卷的摘要归档（卷级 LLM 概括 + 原表归档）。
//!
//! 移植自 `packages/core/src/agents/consolidator.ts`（218 行）：
//! 晋级预处理（advancedCount ≥ 2 翻 promoted）→ 卷边界解析（大纲标题 +
//! 章节区间）→ 完结卷 LLM 概括（≤500 词，保名字/地点/情节点）→
//! volume_summaries.md 追加 + 明细归档 summaries_archive/ →
//! chapter_summaries.md 仅留当前卷。

use std::path::Path;

use async_trait::async_trait;
use regex::Regex;
use std::sync::OnceLock;

use crate::agents::continuity::ChatOutcome;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::runtime_state::HookRecord;
use crate::utils::hook_promotion::rerun_promotion_pass;
use crate::utils::language::WritingLanguage;
use crate::utils::story_markdown::{parse_pending_hooks_markdown, render_hook_snapshot};

/// agent 名。对齐 TS `ConsolidatorAgent.name`。
pub const CONSOLIDATOR_NAME: &str = "consolidator";

/// LLM 聊天端口（temperature 0.3）。
#[async_trait]
pub trait ConsolidatorChat: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String>;
}

/// 归档结果。对齐 TS `ConsolidationResult`。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsolidationResult {
    pub volume_summaries: String,
    pub archived_volumes: usize,
    pub retained_chapters: usize,
    pub promoted_hook_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ConsolidateError {
    #[error("LLM chat failed: {0}")]
    Chat(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// 卷边界。
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeBoundary {
    name: String,
    start_ch: u32,
    end_ch: u32,
}

/// 摘要表行。
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryRow {
    chapter: u32,
    raw: String,
}

/// 归档主入口。
pub async fn consolidate(
    chat: &dyn ConsolidatorChat,
    book_dir: &Path,
) -> Result<ConsolidationResult, ConsolidateError> {
    let story_dir = book_dir.join("story");
    let summaries_path = story_dir.join("chapter_summaries.md");
    let volume_summaries_path = story_dir.join("volume_summaries.md");

    let (summaries_raw, outline_raw) = tokio::join!(
        read_or_empty(&summaries_path),
        crate::utils::outline_paths::read_volume_map(book_dir, ""),
    );

    // Phase 7 hotfix 2：归档前晋级预处理（新书无完结卷也跑）。
    let promoted_hook_count = rerun_advanced_count_promotion(&story_dir).await;

    if summaries_raw.is_empty() || outline_raw.is_empty() {
        return Ok(ConsolidationResult {
            volume_summaries: String::new(),
            archived_volumes: 0,
            retained_chapters: 0,
            promoted_hook_count,
        });
    }

    let volume_boundaries = parse_volume_boundaries(&outline_raw);
    if volume_boundaries.is_empty() {
        return Ok(ConsolidationResult {
            volume_summaries: String::new(),
            archived_volumes: 0,
            retained_chapters: 0,
            promoted_hook_count,
        });
    }

    let (header, rows) = parse_summary_table(&summaries_raw);
    if rows.is_empty() {
        return Ok(ConsolidationResult {
            volume_summaries: String::new(),
            archived_volumes: 0,
            retained_chapters: 0,
            promoted_hook_count,
        });
    }

    let max_chapter = rows.iter().map(|r| r.chapter).max().unwrap_or(0);

    // 完结卷（endCh ≤ maxChapter 且有行）vs 当前卷（保留明细）。
    let mut completed: Vec<(&VolumeBoundary, Vec<&SummaryRow>)> = Vec::new();
    let mut current_rows: Vec<&SummaryRow> = Vec::new();

    for vol in &volume_boundaries {
        let vol_rows: Vec<&SummaryRow> = rows
            .iter()
            .filter(|r| r.chapter >= vol.start_ch && r.chapter <= vol.end_ch)
            .collect();
        if vol.end_ch <= max_chapter && !vol_rows.is_empty() {
            completed.push((vol, vol_rows));
        } else {
            current_rows.extend(vol_rows);
        }
    }

    // 边界外行保留。
    let covered: std::collections::HashSet<u32> = volume_boundaries
        .iter()
        .flat_map(|v| v.start_ch..=v.end_ch)
        .collect();
    for row in &rows {
        if !covered.contains(&row.chapter) {
            current_rows.push(row);
        }
    }

    if completed.is_empty() {
        return Ok(ConsolidationResult {
            volume_summaries: String::new(),
            archived_volumes: 0,
            retained_chapters: current_rows.len(),
            promoted_hook_count,
        });
    }

    // 每完结卷 LLM 概括。
    let existing = read_or_empty(&volume_summaries_path).await;
    let mut new_summaries: Vec<String> = if existing.trim().is_empty() {
        vec!["# Volume Summaries".to_string()]
    } else {
        vec![existing.trim().to_string()]
    };
    for (vol, vol_rows) in &completed {
        let vol_summary_rows = vol_rows.iter().map(|r| r.raw.clone()).collect::<Vec<_>>().join("\n");
        let response = chat
            .chat(
                vec![
                    LLMMessage {
                        role: LLMRole::System,
                        content: "You are a narrative summarizer. Compress chapter-by-chapter summaries into a single coherent paragraph (max 500 words) that captures the key events, character developments, and plot progression of this volume. Preserve specific names, locations, and plot points. Write in the same language as the input.".to_string(),
                        tool_calls: None, tool_call_id: None,
                    },
                    LLMMessage {
                        role: LLMRole::User,
                        content: format!(
                            "Volume: {} (Chapters {}-{})\n\nChapter summaries:\n{}\n{}",
                            vol.name, vol.start_ch, vol.end_ch, header, vol_summary_rows
                        ),
                        tool_calls: None, tool_call_id: None,
                    },
                ],
                0.3,
            )
            .await
            .map_err(ConsolidateError::Chat)?;
        new_summaries.push(format!(
            "\n## {} (Ch.{}-{})\n\n{}",
            vol.name,
            vol.start_ch,
            vol.end_ch,
            response.content.trim()
        ));
    }

    let combined = new_summaries.join("\n");
    tokio::fs::write(&volume_summaries_path, &combined).await?;

    // 明细归档。
    let archive_dir = story_dir.join("summaries_archive");
    tokio::fs::create_dir_all(&archive_dir).await?;
    for (vol, vol_rows) in &completed {
        let archive_path = archive_dir.join(format!("vol_{}-{}.md", vol.start_ch, vol.end_ch));
        let detail = vol_rows.iter().map(|r| r.raw.clone()).collect::<Vec<_>>().join("\n");
        tokio::fs::write(
            archive_path,
            format!("# {}\n\n{}\n{}", vol.name, header, detail),
        )
        .await?;
    }

    // chapter_summaries.md 仅留当前卷行。
    let retained = if !current_rows.is_empty() {
        format!(
            "{}\n{}\n",
            header,
            current_rows
                .iter()
                .map(|r| r.raw.clone())
                .collect::<Vec<_>>()
                .join("\n")
        )
    } else {
        format!("{header}\n")
    };
    tokio::fs::write(&summaries_path, retained).await?;

    Ok(ConsolidationResult {
        volume_summaries: combined,
        archived_volumes: completed.len(),
        retained_chapters: current_rows.len(),
        promoted_hook_count,
    })
}

/// 晋级预处理（advancedCount 跨阈翻 promoted；返回翻转数）。
async fn rerun_advanced_count_promotion(story_dir: &Path) -> usize {
    let ledger_path = story_dir.join("pending_hooks.md");
    let Ok(raw) = tokio::fs::read_to_string(&ledger_path).await else {
        return 0;
    };
    if raw.trim().is_empty() {
        return 0;
    }
    let hooks: Vec<HookRecord> = parse_pending_hooks_markdown(&raw);
    if hooks.is_empty() {
        return 0;
    }
    let contains_cjk = raw.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let language = if contains_cjk { WritingLanguage::Zh } else { WritingLanguage::En };
    let summaries_raw = read_or_empty(&story_dir.join("chapter_summaries.md")).await;

    let result = rerun_promotion_pass(&hooks, &summaries_raw);
    if !result.updated {
        return 0;
    }
    if tokio::fs::write(&ledger_path, render_hook_snapshot(&result.hooks, language))
        .await
        .is_err()
    {
        return 0;
    }
    result.flipped_count
}

/// 卷边界解析：`第N卷（第A-B章）` / `Volume N (Chapters A-B)` 形态。
pub fn parse_volume_boundaries(outline: &str) -> Vec<VolumeBoundary> {
    let mut volumes = Vec::new();
    for raw_line in outline.split('\n') {
        let line = heading_prefix_re().replace(raw_line, "").trim().to_string();
        if !volume_header_re().is_match(&line) {
            continue;
        }
        let Some(captures) = range_re().captures(&line) else {
            continue;
        };
        let start = captures
            .get(1)
            .or(captures.get(3))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(0);
        let end = captures
            .get(2)
            .or(captures.get(4))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(0);
        if start == 0 || end == 0 {
            continue;
        }
        // 名字 = 区间前的部分（去尾随开括号）。
        let range_index = captures.get(0).map(|m| m.start()).unwrap_or(line.len());
        let name = trailing_paren_re()
            .replace(&line[..range_index], "")
            .trim()
            .to_string();
        if !name.is_empty() {
            volumes.push(VolumeBoundary { name, start_ch: start, end_ch: end });
        }
    }
    volumes
}

/// 摘要表解析（表头 = 章节/Chapter/--- 行；数据行首列数字）。
pub fn parse_summary_table(raw: &str) -> (String, Vec<SummaryRow>) {
    let mut header_lines: Vec<&str> = Vec::new();
    let mut data_lines: Vec<&str> = Vec::new();
    for line in raw.split('\n') {
        if !line.starts_with('|') {
            continue;
        }
        if line.contains("章节") || line.contains("Chapter") || line.contains("---") {
            header_lines.push(line);
        } else {
            data_lines.push(line);
        }
    }
    let header = header_lines.join("\n");
    let rows = data_lines
        .iter()
        .filter_map(|line| {
            chapter_cell_re()
                .captures(line)
                .and_then(|c| c[1].parse::<u32>().ok())
                .filter(|chapter| *chapter > 0)
                .map(|chapter| SummaryRow { chapter, raw: (*line).to_string() })
        })
        .collect();
    (header, rows)
}

async fn read_or_empty(path: &Path) -> String {
    tokio::fs::read_to_string(path).await.unwrap_or_default()
}

fn heading_prefix_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^#+\s*").unwrap())
}

fn volume_header_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(第[一二三四五六七八九十百千万零〇\d]+卷|Volume\s+\d+)").unwrap()
    })
}

/// 区间：括号式 `（第A-B章）` 或行内式 `第A-B章` / `Chapters A-B`。
fn range_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?i)[（(]\s*(?:第|[Cc]hapters?\s+)?(\d+)\s*[-–~～—]\s*(\d+)\s*(?:章)?\s*[）)]|(?:第|[Cc]hapters?\s+)(\d+)\s*[-–~～—]\s*(\d+)\s*(?:章)?",
        )
        .unwrap()
    })
}

fn trailing_paren_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[（(]\s*$").unwrap())
}

fn chapter_cell_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\|\s*(\d+)\s*\|").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_boundaries_parse_both_forms() {
        let outline = "## 第一卷（第1-30章）觉醒\n内容\n## Volume 2 (Chapters 31-60)\ncontent\n## 无区间卷\nx";
        let volumes = parse_volume_boundaries(outline);
        assert_eq!(volumes.len(), 2);
        assert_eq!(volumes[0].name, "第一卷");
        assert_eq!((volumes[0].start_ch, volumes[0].end_ch), (1, 30));
        assert_eq!(volumes[1].name, "Volume 2");
        assert_eq!((volumes[1].start_ch, volumes[1].end_ch), (31, 60));
    }

    #[test]
    fn summary_table_splits_header_and_rows() {
        let raw = "| 章节 | 标题 |\n| --- | --- |\n| 1 | a |\n| 2 | b |\n无关行";
        let (header, rows) = parse_summary_table(raw);
        assert!(header.contains("| 章节 |"));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].chapter, 1);
    }

    struct MockChat;

    #[async_trait]
    impl ConsolidatorChat for MockChat {
        async fn chat(&self, _m: Vec<LLMMessage>, _t: f64) -> Result<ChatOutcome, String> {
            Ok(ChatOutcome {
                content: "第一卷概括：林动觉醒祖符，进入宗门。".into(),
                usage: None,
            })
        }
    }

    #[tokio::test]
    async fn consolidate_archives_completed_volume() {
        let dir = tempfile::tempdir().unwrap();
        let story = dir.path().join("story");
        let outline = story.join("outline");
        tokio::fs::create_dir_all(&outline).await.unwrap();
        tokio::fs::write(
            outline.join("volume_map.md"),
            "## 第一卷（第1-2章）觉醒\n\n- 第 1 章：开端\n- 第 2 章：推进\n",
        )
        .await
        .unwrap();
        tokio::fs::write(
            story.join("chapter_summaries.md"),
            "| 章节 | 标题 |\n| --- | --- |\n| 1 | a |\n| 2 | b |\n| 3 | c |\n",
        )
        .await
        .unwrap();

        let result = consolidate(&MockChat, dir.path()).await.unwrap();
        assert_eq!(result.archived_volumes, 1);
        assert_eq!(result.retained_chapters, 1);
        assert!(result.volume_summaries.contains("## 第一卷 (Ch.1-2)"));
        assert!(result.volume_summaries.contains("林动觉醒祖符"));
        // 归档文件 + 留存表。
        assert!(story.join("summaries_archive").join("vol_1-2.md").exists());
        let retained = tokio::fs::read_to_string(story.join("chapter_summaries.md"))
            .await
            .unwrap();
        assert!(retained.contains("| 3 | c |"));
        assert!(!retained.contains("| 1 | a |"));
    }

    #[tokio::test]
    async fn empty_inputs_short_circuit() {
        let dir = tempfile::tempdir().unwrap();
        let result = consolidate(&MockChat, dir.path()).await.unwrap();
        assert_eq!(result.archived_volumes, 0);
        assert_eq!(result.volume_summaries, "");
    }
}
