//! 编辑事务控制器（edit-controller）。
//!
//! 移植自 `packages/core/src/interaction/edit-controller.ts`（523 行）的
//! `chapter-replace` 执行路径——HTTP 面唯一可达的 kind（`PUT /books/:id/
//! chapters/:num` 与 `POST .../versions/:versionId/restore`）。事务原子性
//! 与 TS 同款：版本归档先于正文覆写，索引标记在落盘后回写。
//!
//! 暂缓件（仅 agent-tools / project-tools 的 LLM 工具路径可达，随交互
//! runtime 移植）：entity-rename（全库内容替换 + 文件重命名规划）、
//! chapter-local-edit（精确/弹性空白/近似段落三级替换 + dice 相似度）、
//! truth-file-edit、focus-edit、planEditTransaction 规划面。

use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

use crate::models::chapter::{ChapterMeta, ChapterStatus};
use crate::state::chapter_workspace::{archive_chapter_version, ChapterVersionSource};
use crate::state::manager::StateManager;
use crate::state::store::FsStateStore;

/// 事务执行结果。对齐 TS `ExecutedEditTransaction`（camelCase 序列化）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutedEditTransaction {
    pub transaction_type: &'static str,
    pub book_id: String,
    pub chapter_number: u32,
    pub touched_files: Vec<String>,
    pub review_required: bool,
    pub summary: String,
}

/// `chapter-replace` 事务：归档旧稿 → 覆写正文 → 清 runtime 工件 →
/// 索引标记 audit-failed 待人工复核。
///
/// 对齐 TS `executeChapterReplace`。`now_millis` / `now_iso` 由调用方注入
/// （版本 id 与索引 updatedAt 共用同一时刻）。
pub async fn execute_chapter_replace(
    state: &StateManager,
    book_id: &str,
    chapter_number: u32,
    full_text: &str,
    version_source: ChapterVersionSource,
    now_millis: i64,
    now_iso: &str,
) -> Result<ExecutedEditTransaction, String> {
    let trimmed = full_text.trim();
    if trimmed.is_empty() {
        return Err("Chapter replacement requires fullText.".to_string());
    }
    let root = state.book_dir(book_id);
    let chapter_path = find_chapter_path(&root, chapter_number).await?;
    let previous_content = tokio::fs::read_to_string(&chapter_path)
        .await
        .map_err(|e| e.to_string())?;

    let book_dir = root.to_string_lossy().into_owned();
    archive_chapter_version(
        &FsStateStore,
        &book_dir,
        chapter_number,
        &previous_content,
        version_source,
        now_millis,
        now_iso,
    )
    .await
    .map_err(|e| e.to_string())?;

    let normalized = if trimmed.ends_with('\n') { trimmed.to_string() } else { format!("{trimmed}\n") };
    tokio::fs::write(&chapter_path, normalized.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let removed_runtime_files = clear_chapter_runtime_files(&root, chapter_number).await;

    let index = state
        .load_chapter_index(book_id)
        .await
        .map_err(|e| e.to_string())?;
    let updated_index = mark_chapter_for_manual_review(
        index,
        chapter_number,
        MANUAL_REPLACEMENT_ISSUE,
        rough_chapter_length(trimmed),
        now_iso,
    );
    state
        .save_chapter_index(book_id, &updated_index)
        .await
        .map_err(|e| e.to_string())?;

    let chapter_rel = chapter_path
        .strip_prefix(&root)
        .unwrap_or(&chapter_path)
        .to_string_lossy()
        .into_owned();
    Ok(ExecutedEditTransaction {
        transaction_type: "chapter-replace",
        book_id: book_id.to_string(),
        chapter_number,
        touched_files: [
            vec![chapter_rel],
            removed_runtime_files,
            vec!["chapters/index.json".to_string()],
        ]
        .concat(),
        review_required: true,
        summary: format!("Replaced chapter {chapter_number} and marked it for review."),
    })
}

/// TS `markChapterForManualReview` 的固定提示文案。
pub const MANUAL_REPLACEMENT_ISSUE: &str =
    "Manual chapter replacement requires review before continuation.";

/// 定位章节文件：`chapters/{NNNN}_*.md`（前缀必须带下划线，对齐 TS
/// `findChapterPath` 的 `startsWith(\`${padded}_\`)`；未命中 → 错误文案逐字）。
async fn find_chapter_path(root: &Path, chapter_number: u32) -> Result<PathBuf, String> {
    let chapters_dir = root.join("chapters");
    let prefix = format!("{:04}_", chapter_number);
    let mut matched: Option<String> = None;
    if let Ok(mut entries) = tokio::fs::read_dir(&chapters_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&prefix) && name.ends_with(".md") {
                matched = Some(name);
                break;
            }
        }
    }
    match matched {
        Some(file) => Ok(chapters_dir.join(file)),
        None => Err(format!("Chapter {chapter_number} not found.")),
    }
}

/// 清除 `story/runtime/chapter-NNNN.*` 工件（保留 user-brief），返回书内
/// 相对路径列表。对齐 TS `clearChapterRuntimeFiles`（unlink 失败静默）。
async fn clear_chapter_runtime_files(root: &Path, chapter_number: u32) -> Vec<String> {
    let padded = format!("{chapter_number:04}");
    let keep = format!("chapter-{padded}.user-brief.md");
    let runtime_dir = root.join("story").join("runtime");
    let mut removed = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&runtime_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("chapter-{padded}.")) && name != keep {
                let _ = tokio::fs::remove_file(entry.path()).await;
                removed.push(format!("story/runtime/{name}"));
            }
        }
    }
    removed
}

/// 索引标记：状态 → audit-failed、updatedAt、wordCount 覆写、去重后追加
/// `[warning] {issue}`。对齐 TS `markChapterForManualReview`。
fn mark_chapter_for_manual_review(
    index: Vec<ChapterMeta>,
    chapter_number: u32,
    issue: &str,
    word_count: u32,
    now_iso: &str,
) -> Vec<ChapterMeta> {
    index
        .into_iter()
        .map(|mut chapter| {
            if chapter.number == chapter_number {
                chapter.status = ChapterStatus::AuditFailed;
                chapter.updated_at = now_iso.to_string();
                chapter.word_count = word_count;
                chapter.audit_issues.retain(|existing| !existing.contains(issue));
                chapter.audit_issues.push(format!("[warning] {issue}"));
            }
            chapter
        })
        .collect()
}

/// 粗略字数：剥 frontmatter（首个 `---...---` 块）与标题行，去全部空白后
/// 按 UTF-16 码元计数。对齐 TS `roughChapterLength`。
fn rough_chapter_length(content: &str) -> u32 {
    let stripped = frontmatter_re().replacen(content, 1, "");
    let stripped = heading_line_re().replace_all(&stripped, "");
    let compacted: String = stripped.chars().filter(|c| !c.is_whitespace()).collect();
    compacted.encode_utf16().count() as u32
}

fn frontmatter_re() -> &'static Regex {
    // TS: /^---[\s\S]*?---\s*/m（非全局，仅替换首个匹配）
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^---[\s\S]*?---\s*").expect("frontmatter regex"))
}

fn heading_line_re() -> &'static Regex {
    // TS: /^#{1,6}\s+.*$/gm
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^#{1,6}\s+.*$").expect("heading regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(root: &Path) -> StateManager {
        StateManager::new(root.to_path_buf())
    }

    async fn fixture(root: &Path) {
        let book = root.join("books").join("b1");
        tokio::fs::create_dir_all(book.join("chapters")).await.unwrap();
        tokio::fs::create_dir_all(book.join("story").join("runtime")).await.unwrap();
        tokio::fs::write(book.join("chapters").join("0003_Demo.md"), "# 第3章 旧稿\n\n旧正文。").await.unwrap();
        tokio::fs::write(book.join("story").join("runtime").join("chapter-0003.plan.md"), "plan").await.unwrap();
        tokio::fs::write(book.join("story").join("runtime").join("chapter-0003.user-brief.md"), "brief").await.unwrap();
        tokio::fs::write(
            book.join("chapters").join("index.json"),
            r#"[{"number":3,"title":"Demo","status":"ready-for-review","wordCount":4,"createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]}]"#,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn replaces_archives_marks_and_clears_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root).await;
        let result = execute_chapter_replace(
            &state(&root),
            "b1",
            3,
            "# 第3章 新稿\n\n人工修改后的正文。",
            ChapterVersionSource::Manual,
            1_782_864_000_000,
            "2026-07-01T00:00:00.000Z",
        )
        .await
        .unwrap();

        assert_eq!(result.transaction_type, "chapter-replace");
        assert!(result.review_required);
        assert_eq!(result.summary, "Replaced chapter 3 and marked it for review.");
        // touchedFiles：章节文件 + runtime 工件（user-brief 保留）+ index。
        assert_eq!(result.touched_files[0], "chapters/0003_Demo.md");
        assert_eq!(result.touched_files[1], "story/runtime/chapter-0003.plan.md");
        assert!(*result.touched_files.last().unwrap() == "chapters/index.json");

        let book = root.join("books").join("b1");
        // 正文覆写 + 补尾换行。
        let content = tokio::fs::read_to_string(book.join("chapters").join("0003_Demo.md")).await.unwrap();
        assert_eq!(content, "# 第3章 新稿\n\n人工修改后的正文。\n");
        // 旧稿归档（_manual_ 段 + 原 frontmatter/正文完整保留）。
        let versions_dir = book.join("chapters").join(".versions").join("0003");
        let mut names: Vec<String> = Vec::new();
        let mut entries = tokio::fs::read_dir(&versions_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        assert_eq!(names.len(), 1);
        assert!(names[0].starts_with("1782864000000_manual_"));
        let archived = tokio::fs::read_to_string(versions_dir.join(&names[0])).await.unwrap();
        assert_eq!(archived, "# 第3章 旧稿\n\n旧正文。");
        // runtime：plan 删除、user-brief 保留。
        assert!(!book.join("story").join("runtime").join("chapter-0003.plan.md").exists());
        assert!(book.join("story").join("runtime").join("chapter-0003.user-brief.md").exists());
        // 索引：audit-failed + [warning] 注入 + updatedAt/wordCount 覆写。
        let index: Vec<ChapterMeta> = serde_json::from_str(
            &tokio::fs::read_to_string(book.join("chapters").join("index.json")).await.unwrap(),
        )
        .unwrap();
        assert_eq!(index[0].status, ChapterStatus::AuditFailed);
        assert_eq!(index[0].updated_at, "2026-07-01T00:00:00.000Z");
        assert_eq!(index[0].audit_issues, vec![format!("[warning] {MANUAL_REPLACEMENT_ISSUE}")]);
        assert_eq!(index[0].word_count, 9); // 标题行剥除，正文 9 个 CJK 码元
    }

    #[tokio::test]
    async fn empty_fulltext_and_missing_chapter_errors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root).await;
        let err = execute_chapter_replace(
            &state(&root),
            "b1",
            3,
            "   ",
            ChapterVersionSource::Manual,
            0,
            "2026-07-01T00:00:00.000Z",
        )
        .await
        .unwrap_err();
        assert_eq!(err, "Chapter replacement requires fullText.");

        let err = execute_chapter_replace(
            &state(&root),
            "b1",
            5,
            "正文",
            ChapterVersionSource::Restore,
            0,
            "2026-07-01T00:00:00.000Z",
        )
        .await
        .unwrap_err();
        assert_eq!(err, "Chapter 5 not found.");
    }

    #[test]
    fn rough_length_strips_frontmatter_headings_and_whitespace() {
        let content = "---\ntitle: x\n---\n# 第1章 标题\n\n正文内容。\n## 小节\n尾部";
        assert_eq!(rough_chapter_length(content), 7); // 「正文内容。尾部」7 个码元
        // 非 frontmatter 开头时首行 `---` 不剥离（TS ^--- 行首锚定）。
        assert_eq!(rough_chapter_length("正文"), 2);
    }
}
