//! 规划材料装配（planner 的种子材料 + 记忆选集聚合）。
//!
//! 移植自 `packages/core/src/utils/planning-materials.ts`（185 行）。
//! [`load_planning_seed_materials`] 读取 story/ 下的真相文件（Phase 5 优先
//! outline/ 新路径，legacy 文件透明回退）；[`gather_planning_materials`] 在
//! 种子之上叠加 [`retrieve_memory_selection`] 的记忆选集与 planner 输入清单。
//!
//! ## 移植纪律
//! - `readPreviousEndingExcerpt` 的 `body.slice(-320)` 是 **UTF-16 窗口**语义
//!   （emoji/代理对不得截半），与 TS `sanitize_filename` 的 slice(0,50) 同类
//! - `previousEndingHook` 只在 hookActivity 非空时 Some（TS `|| undefined` 的
//!   falsy 门控）
//! - `recentSummaries` 先按章节 DESC 取前 4 再还原 ASC（TS 两次 sort 的顺序）

use std::path::{Path, PathBuf};

use crate::models::runtime_state::HookRecord;
use crate::state::memory_db::StoredSummary;
use crate::utils::memory_retrieval::{retrieve_memory_selection, MemorySelection, RetrieveMemoryParams};
use crate::utils::outline_paths::{
    read_current_state_with_fallback, read_story_frame, read_volume_map,
};
use crate::utils::story_markdown::parse_chapter_summaries_markdown;

/// readFileOrDefault 的统一 fallback（对齐 TS 硬编码）。
const MISSING_FILE: &str = "(文件尚未创建)";

/// 规划种子材料（story/ 真相文件一次性快照）。对齐 TS `PlanningSeedMaterials`。
#[derive(Debug, Clone, Default)]
pub struct PlanningSeedMaterials {
    pub story_dir: PathBuf,
    pub author_intent: String,
    pub current_focus: String,
    pub story_bible: String,
    pub volume_outline: String,
    pub book_rules_raw: String,
    pub current_state: String,
    pub chapter_summaries_raw: String,
    pub brief: String,
    pub outline_node: Option<String>,
    /// 章节号 ASC 的最近 4 章摘要。
    pub recent_summaries: Vec<StoredSummary>,
    pub previous_ending_hook: Option<String>,
    pub previous_ending_excerpt: Option<String>,
}

/// 种子材料 + 记忆选集。对齐 TS `PlanningMaterials`。
#[derive(Debug, Clone, Default)]
pub struct PlanningMaterials {
    pub seed: PlanningSeedMaterials,
    pub outline_node: Option<String>,
    pub active_hooks: Vec<HookRecord>,
    pub memory_selection: MemorySelection,
    pub planner_inputs: Vec<String>,
}

/// 读取前章末屏节选：chapters/ 下 `NNNN*.md`，去首行取正文，截尾 320 UTF-16 单元。
pub async fn read_previous_ending_excerpt(
    book_dir: &Path,
    chapter_number: u32,
) -> Option<String> {
    let previous_chapter = chapter_number.checked_sub(1)?;
    if previous_chapter < 1 {
        return None;
    }

    let chapters_dir = book_dir.join("chapters");
    let padded = format!("{:04}", previous_chapter);
    let mut entries = tokio::fs::read_dir(&chapters_dir).await.ok()?;
    let mut matched: Option<String> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&padded) && name.ends_with(".md") {
            // TS files.find：取第一个匹配（readdir 顺序）。
            matched = Some(name);
            break;
        }
    }
    let file_name = matched?;
    let markdown = tokio::fs::read_to_string(chapters_dir.join(file_name))
        .await
        .ok()?;

    let body: String = markdown
        .split('\n')
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if body.is_empty() {
        return None;
    }
    Some(utf16_tail(&body, 320).trim().to_string())
}

/// 尾部 UTF-16 窗口切片（对齐 JS `String.prototype.slice(-n)`，代理对不截半）。
fn utf16_tail(value: &str, take: usize) -> String {
    let units: Vec<u16> = value.encode_utf16().collect();
    if units.len() <= take {
        return value.to_string();
    }
    String::from_utf16_lossy(&units[units.len() - take..])
}

/// 装配规划种子材料：author_intent / current_focus / 摘要 / 规则 / 状态 / brief。
pub async fn load_planning_seed_materials(
    book_dir: &Path,
    chapter_number: u32,
) -> PlanningSeedMaterials {
    let story_dir = book_dir.join("story");

    let author_intent_path = story_dir.join("author_intent.md");
    let current_focus_path = story_dir.join("current_focus.md");
    let chapter_summaries_path = story_dir.join("chapter_summaries.md");
    let book_rules_path = story_dir.join("book_rules.md");
    let brief_path = story_dir.join("brief.md");

    let (
        author_intent,
        current_focus,
        story_bible,
        volume_outline,
        chapter_summaries_raw,
        book_rules_raw,
        current_state,
        previous_ending_excerpt,
        brief,
    ) = tokio::join!(
        read_file_or_default(&author_intent_path),
        read_file_or_default(&current_focus_path),
        read_story_frame(book_dir, MISSING_FILE),
        read_volume_map(book_dir, MISSING_FILE),
        read_file_or_default(&chapter_summaries_path),
        read_file_or_default(&book_rules_path),
        // Phase 5 整合：current_state.md 还是架构师占位时，从 roles +
        // pending_hooks 种子行推导初始状态。
        read_current_state_with_fallback(book_dir, MISSING_FILE),
        read_previous_ending_excerpt(book_dir, chapter_number),
        read_brief_file(&brief_path),
    );

    // 章节号 DESC 过滤后取前 4，再还原 ASC。
    let mut chapter_summaries: Vec<StoredSummary> = parse_chapter_summaries_markdown(
        &chapter_summaries_raw,
    )
    .into_iter()
    .filter(|summary| summary.chapter < i64::from(chapter_number))
    .collect();
    chapter_summaries.sort_by(|left, right| right.chapter.cmp(&left.chapter));
    let previous_ending_hook = chapter_summaries
        .first()
        .map(|summary| summary.hook_activity.clone())
        .filter(|activity| !activity.is_empty());
    chapter_summaries.truncate(4);
    chapter_summaries.sort_by(|left, right| left.chapter.cmp(&right.chapter));

    PlanningSeedMaterials {
        story_dir,
        author_intent,
        current_focus,
        story_bible,
        volume_outline,
        book_rules_raw,
        current_state,
        chapter_summaries_raw,
        brief,
        outline_node: None,
        recent_summaries: chapter_summaries,
        previous_ending_hook,
        previous_ending_excerpt,
    }
}

/// 种子材料 + 记忆选集聚合（无 seed 时先读种子）。对齐 TS `gatherPlanningMaterials`。
pub async fn gather_planning_materials(
    book_dir: &Path,
    chapter_number: u32,
    goal: &str,
    outline_node: Option<&str>,
    must_keep: &[String],
    seed: Option<PlanningSeedMaterials>,
) -> PlanningMaterials {
    let seed = match seed {
        Some(seed) => seed,
        None => load_planning_seed_materials(book_dir, chapter_number).await,
    };

    let memory_selection = retrieve_memory_selection(&RetrieveMemoryParams {
        book_dir,
        chapter_number,
        goal,
        outline_node,
        must_keep,
        semantic_selector: None,
    })
    .await;

    let story_dir = &seed.story_dir;
    let mut planner_inputs: Vec<String> = vec![
        story_dir.join("author_intent.md").to_string_lossy().into_owned(),
        story_dir.join("current_focus.md").to_string_lossy().into_owned(),
        story_dir.join("outline").join("story_frame.md").to_string_lossy().into_owned(),
        story_dir.join("outline").join("volume_map.md").to_string_lossy().into_owned(),
        story_dir.join("chapter_summaries.md").to_string_lossy().into_owned(),
        story_dir.join("book_rules.md").to_string_lossy().into_owned(),
        story_dir.join("current_state.md").to_string_lossy().into_owned(),
        story_dir.join("pending_hooks.md").to_string_lossy().into_owned(),
    ];
    if let Some(db_path) = &memory_selection.db_path {
        planner_inputs.push(db_path.clone());
    }

    PlanningMaterials {
        outline_node: outline_node.map(|node| node.to_string()),
        active_hooks: memory_selection.active_hooks.clone(),
        memory_selection,
        planner_inputs,
        seed,
    }
}

async fn read_file_or_default(path: &Path) -> String {
    tokio::fs::read_to_string(path)
        .await
        .unwrap_or_else(|_| MISSING_FILE.to_string())
}

async fn read_brief_file(path: &Path) -> String {
    tokio::fs::read_to_string(path).await.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_tail_keeps_short_strings_and_surrogate_pairs() {
        assert_eq!(utf16_tail("短", 320), "短");
        assert_eq!(utf16_tail("ab", 5), "ab");
        // emoji 是代理对（2 个 UTF-16 单元）：窗口边界不截半。
        let emoji = "🎮x🎮";
        assert_eq!(utf16_tail(emoji, 3), "x🎮");
        assert_eq!(utf16_tail("abcdef", 3), "def");
    }

    #[tokio::test]
    async fn read_previous_ending_excerpt_picks_padded_file_and_tails() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        tokio::fs::create_dir_all(&chapters).await.unwrap();
        let body = format!("# 标题\n{}", "长".repeat(400));
        tokio::fs::write(chapters.join("0012-xxx.md"), body).await.unwrap();

        let excerpt = read_previous_ending_excerpt(dir.path(), 13).await.unwrap();
        assert_eq!(excerpt.encode_utf16().count(), 320);
        assert!(excerpt.starts_with('长'));

        assert!(read_previous_ending_excerpt(dir.path(), 1).await.is_none());
    }
}
