//! 编辑事务控制器（edit-controller）。
//!
//! 移植自 `packages/core/src/interaction/edit-controller.ts` 的执行路径：
//! chapter-replace（REST `PUT /books/:id/chapters/:num` 与版本恢复）、
//! entity-rename 与 chapter-local-edit（89 号：agent-tools 书会话确定性
//! 编辑工具族可达）。事务原子性与 TS 同款：版本归档先于正文覆写，索引
//! 标记在落盘后回写。
//!
//! 暂缓件（仅规划面/其余工具路径可达，随后续轮次）：truth-file-edit、
//! focus-edit、planEditTransaction 规划面、chapter-rewrite。

use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

use crate::models::chapter::{ChapterMeta, ChapterStatus};
use crate::state::chapter_workspace::{archive_chapter_version, ChapterVersionSource};
use crate::state::manager::StateManager;
use crate::state::store::FsStateStore;

/// 事务执行结果。对齐 TS `ExecutedEditTransaction`（camelCase 序列化；
/// chapterNumber 仅章级事务存在——entity-rename 无此键）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutedEditTransaction {
    pub transaction_type: &'static str,
    pub book_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_number: Option<u32>,
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
        chapter_number: Some(chapter_number),
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

/// chapter-local-edit 的复核提示文案（TS 逐字）。
pub const MANUAL_TEXT_EDIT_ISSUE: &str =
    "Manual text edit requires review before continuation.";

/// `entity-rename` 事务：全库内容替换 + 文件重命名规划。对齐 TS
/// `executeEntityRename`——snapshots/点目录不可改写（冻结历史与废弃稿）；
/// 重命名目标含路径分隔符即拒（LLM 供给边界）；碰撞在写入前中止。
pub async fn execute_entity_rename(
    state: &StateManager,
    book_id: &str,
    old_value: &str,
    new_value: &str,
) -> Result<ExecutedEditTransaction, String> {
    if new_value.contains('/') || new_value.contains('\\') {
        return Err(format!(
            "Invalid rename target \"{new_value}\": entity names cannot contain path separators."
        ));
    }
    let root = state.book_dir(book_id);
    let files = collect_editable_files(&root).await;
    let planned_renames = plan_entity_file_renames(&root, &files, old_value, new_value).await?;
    let matcher = Regex::new(&regex::escape(old_value)).map_err(|e| e.to_string())?;
    // 插入序去重集合（对齐 JS Set 的 touched 语义）。
    let mut touched: Vec<String> = Vec::new();

    for file_path in &files {
        let content = tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| e.to_string())?;
        let next_content = matcher
            .replace_all(&content, regex::NoExpand(new_value))
            .into_owned();
        if next_content == content {
            continue;
        }
        tokio::fs::write(file_path, next_content.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let relative = file_path
            .strip_prefix(&root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .into_owned();
        if !touched.contains(&relative) {
            touched.push(relative);
        }
    }

    for planned in &planned_renames {
        tokio::fs::rename(&planned.from_abs, &planned.to_abs)
            .await
            .map_err(|e| e.to_string())?;
        touched.retain(|entry| entry != &planned.from_rel);
        if !touched.contains(&planned.to_rel) {
            touched.push(planned.to_rel.clone());
        }
    }

    if touched.is_empty() {
        return Err(format!(
            "No occurrences of \"{old_value}\" were found in \"{book_id}\"."
        ));
    }

    let rename_note = if planned_renames.is_empty() {
        String::new()
    } else {
        format!(
            " ({} file{} renamed on disk)",
            planned_renames.len(),
            if planned_renames.len() == 1 { "" } else { "s" }
        )
    };
    Ok(ExecutedEditTransaction {
        transaction_type: "entity-rename",
        book_id: book_id.to_string(),
        chapter_number: None,
        summary: format!(
            "Renamed {old_value} to {new_value} across {} files{rename_note}.",
            touched.len()
        ),
        touched_files: touched,
        review_required: false,
    })
}

/// `chapter-local-edit` 事务：三级目标替换（精确 → 弹性空白 → 近似段落）→
/// 归档 → 覆写 → 清 runtime → 索引标记复核。对齐 TS `executeChapterLocalEdit`。
pub async fn execute_chapter_local_edit(
    state: &StateManager,
    book_id: &str,
    chapter_number: u32,
    target_text: &str,
    replacement_text: &str,
    now_millis: i64,
    now_iso: &str,
) -> Result<ExecutedEditTransaction, String> {
    if target_text.is_empty() {
        return Err("Chapter-local edits require targetText and replacementText.".to_string());
    }
    let root = state.book_dir(book_id);
    let chapter_path = find_chapter_path(&root, chapter_number).await?;
    let content = tokio::fs::read_to_string(&chapter_path)
        .await
        .map_err(|e| e.to_string())?;
    let next_content = replace_chapter_target_text(&content, target_text, replacement_text);
    if next_content == content {
        return Err(format!("Target text was not found in chapter {chapter_number}."));
    }

    let book_dir = root.to_string_lossy().into_owned();
    archive_chapter_version(
        &FsStateStore,
        &book_dir,
        chapter_number,
        &content,
        ChapterVersionSource::Agent,
        now_millis,
        now_iso,
    )
    .await
    .map_err(|e| e.to_string())?;
    tokio::fs::write(&chapter_path, next_content.as_bytes())
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
        MANUAL_TEXT_EDIT_ISSUE,
        rough_chapter_length(&next_content),
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
        transaction_type: "chapter-local-edit",
        book_id: book_id.to_string(),
        chapter_number: Some(chapter_number),
        touched_files: [
            vec![chapter_rel],
            removed_runtime_files,
            vec!["chapters/index.json".to_string()],
        ]
        .concat(),
        review_required: true,
        summary: format!("Patched chapter {chapter_number} and marked it for review."),
    })
}

/// 可编辑文件收集：递归遍历；snapshots 与点目录（如 chapters/.trash）跳过
/// （冻结历史与废弃稿不可改写）；仅 .md/.json/.ya?ml/.txt。对齐 TS
/// `collectEditableFiles`（缺目录静默为空）。
async fn collect_editable_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            if name == "snapshots" || name.starts_with('.') {
                continue;
            }
            files.extend(Box::pin(collect_editable_files(&path)).await);
            continue;
        }
        let lower = name.to_lowercase();
        if lower.ends_with(".md")
            || lower.ends_with(".json")
            || lower.ends_with(".yaml")
            || lower.ends_with(".yml")
            || lower.ends_with(".txt")
        {
            files.push(path);
        }
    }
    files
}

struct PlannedFileRename {
    from_abs: PathBuf,
    to_abs: PathBuf,
    from_rel: String,
    to_rel: String,
}

/// 文件重命名规划：文件名含旧名 → 全替换为新名；目标已存在即整体中止
/// （避免内容半重写）。对齐 TS `planEntityFileRenames`。
async fn plan_entity_file_renames(
    root: &Path,
    files: &[PathBuf],
    old_value: &str,
    new_value: &str,
) -> Result<Vec<PlannedFileRename>, String> {
    let mut planned = Vec::new();
    for file_path in files {
        let base = file_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !base.contains(old_value) {
            continue;
        }
        let next_base = base.replace(old_value, new_value);
        if next_base == base {
            continue;
        }
        let to_abs = file_path
            .parent()
            .map(|parent| parent.join(&next_base))
            .unwrap_or_else(|| PathBuf::from(&next_base));
        if tokio::fs::metadata(&to_abs).await.is_ok() {
            let from_rel = file_path
                .strip_prefix(root)
                .unwrap_or(file_path)
                .to_string_lossy()
                .into_owned();
            return Err(format!(
                "Cannot rename \"{from_rel}\" to \"{next_base}\": a file with that name already exists."
            ));
        }
        planned.push(PlannedFileRename {
            from_rel: file_path
                .strip_prefix(root)
                .unwrap_or(file_path)
                .to_string_lossy()
                .into_owned(),
            to_rel: to_abs
                .strip_prefix(root)
                .unwrap_or(&to_abs)
                .to_string_lossy()
                .into_owned(),
            from_abs: file_path.clone(),
            to_abs,
        });
    }
    Ok(planned)
}

/// 三级目标替换：精确子串全替换 → 弹性空白正则 → 近似段落定位。
/// 对齐 TS `replaceChapterTargetText`。
fn replace_chapter_target_text(content: &str, target_text: &str, replacement_text: &str) -> String {
    if content.contains(target_text) {
        return content.replace(target_text, replacement_text);
    }
    if let Some(pattern) = flexible_whitespace_pattern(target_text) {
        if pattern.is_match(content) {
            return pattern
                .replace_all(content, regex::NoExpand(replacement_text))
                .into_owned();
        }
    }
    replace_approximate_paragraph(content, target_text, replacement_text)
}

/// 弹性空白：目标按空白切分 ≥2 段时以 `\s+` 连接（全局匹配）。
fn flexible_whitespace_pattern(target_text: &str) -> Option<regex::Regex> {
    let parts: Vec<&str> = target_text.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let pattern = parts
        .iter()
        .map(|part| regex::escape(part))
        .collect::<Vec<_>>()
        .join(r"\s+");
    regex::Regex::new(&pattern).ok()
}

/// 近似段落定位（目标定位器，非语义改写引擎）：归一化 NFKC+lower+仅留
/// 字母数字；段长 ≥24 且在目标 0.35x–3x 之间；dice 二元组相似度阈值
/// 0.72（高分 0.86 或领先 0.06）。对齐 TS `replaceApproximateParagraph`。
fn replace_approximate_paragraph(content: &str, target_text: &str, replacement_text: &str) -> String {
    let target = normalize_approximate_text(target_text);
    let target_len = utf16_len(&target);
    if target_len < 24 {
        return content.to_string();
    }

    let mut best: Option<(usize, usize, f64)> = None;
    let mut second_best_score = 0.0f64;
    for (start, end) in paragraph_spans(content) {
        let raw = &content[start..end];
        let normalized = normalize_approximate_text(raw);
        let normalized_len = utf16_len(&normalized);
        if normalized_len < 24 {
            continue;
        }
        let normalized_len_f = normalized_len as f64;
        if normalized_len_f < target_len as f64 * 0.35 || normalized_len_f > target_len as f64 * 3.0 {
            continue;
        }
        let score = approximate_text_score(&target, &normalized);
        match &best {
            None => best = Some((start, end, score)),
            Some((_, _, best_score)) if score > *best_score => {
                second_best_score = *best_score;
                best = Some((start, end, score));
            }
            Some((_, _, best_score)) if score > second_best_score && score <= *best_score => {
                second_best_score = score;
            }
            _ => {}
        }
    }

    let Some((start, end, best_score)) = best else {
        return content.to_string();
    };
    if best_score < 0.72 || (best_score < 0.86 && best_score - second_best_score < 0.06) {
        return content.to_string();
    }
    format!("{}{replacement_text}{}", &content[..start], &content[end..])
}

/// 段落切分：复刻 JS `/\S[\s\S]*?(?=\n\s*\n|$)/g`——每段自首个非空白
/// 字符起，至其后最早的空白行边界或文本末尾（惰性 + 前瞻的等价手写）。
fn paragraph_spans(content: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = content[cursor..].find(|c: char| !c.is_whitespace()) {
        let start = cursor + offset;
        // find_at 需字符边界：跨过段首字符（JS 惰性量词最少消费一个 \S）。
        let after_start = start + content[start..].chars().next().map_or(1, char::len_utf8);
        let end = blank_line_re()
            .find_at(content, after_start)
            .map(|m| m.start())
            .unwrap_or(content.len());
        spans.push((start, end));
        if end >= content.len() {
            break;
        }
        cursor = end;
    }
    spans
}

fn blank_line_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n\s*\n").expect("blank line regex"))
}

fn normalize_approximate_text(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let normalized: String = text.nfkc().collect();
    normalized
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphabetic() || c.is_numeric())
        .collect()
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn approximate_text_score(target: &str, candidate: &str) -> f64 {
    if candidate.contains(target) || target.contains(candidate) {
        let left = utf16_len(target) as f64;
        let right = utf16_len(candidate) as f64;
        return left.min(right) / left.max(right);
    }
    dice_coefficient(&to_bigrams(target), &to_bigrams(candidate))
}

fn to_bigrams(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 {
        return chars.iter().map(|c| c.to_string()).collect();
    }
    chars.windows(2).map(|w| format!("{}{}", w[0], w[1])).collect()
}

fn dice_coefficient(left: &[String], right: &[String]) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let mut counts: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for item in left {
        *counts.entry(item.as_str()).or_insert(0) += 1;
    }
    let mut overlap = 0i64;
    for item in right {
        if let Some(count) = counts.get_mut(item.as_str()) {
            if *count <= 0 {
                continue;
            }
            *count -= 1;
            overlap += 1;
        }
    }
    (2.0 * overlap as f64) / (left.len() + right.len()) as f64
}

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

    #[tokio::test]
    async fn entity_rename_rewrites_content_and_renames_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let book = root.join("books").join("b1");
        tokio::fs::create_dir_all(book.join("chapters")).await.unwrap();
        tokio::fs::create_dir_all(book.join("story").join("roles").join("主要角色")).await.unwrap();
        tokio::fs::create_dir_all(book.join("story").join("snapshots").join("1")).await.unwrap();
        tokio::fs::write(book.join("chapters").join("0001_风起.md"), "林动走出青阳镇。").await.unwrap();
        tokio::fs::write(book.join("story").join("current_state.md"), "主角：林动").await.unwrap();
        tokio::fs::write(
            book.join("story").join("roles").join("主要角色").join("林动.md"),
            "# 林动\n主角。",
        )
        .await
        .unwrap();
        // snapshots 与 .trash 不可改写。
        tokio::fs::write(book.join("story").join("snapshots").join("1").join("s.md"), "林动快照").await.unwrap();

        let result = execute_entity_rename(&state(&root), "b1", "林动", "林震").await.unwrap();
        assert_eq!(result.transaction_type, "entity-rename");
        assert!(!result.review_required);
        assert_eq!(
            result.summary,
            "Renamed 林动 to 林震 across 3 files (1 file renamed on disk)."
        );
        // 内容替换 + 索引键无 chapterNumber。
        assert_eq!(
            tokio::fs::read_to_string(book.join("chapters").join("0001_风起.md")).await.unwrap(),
            "林震走出青阳镇。"
        );
        assert_eq!(
            tokio::fs::read_to_string(book.join("story").join("snapshots").join("1").join("s.md")).await.unwrap(),
            "林动快照"
        );
        // 文件重命名。
        assert!(book.join("story").join("roles").join("主要角色").join("林震.md").is_file());
        assert!(!book.join("story").join("roles").join("主要角色").join("林动.md").exists());
        let serialized = serde_json::to_value(&result).unwrap();
        assert!(serialized.get("chapterNumber").is_none());

        // 无命中 → 错误文案逐字。
        let err = execute_entity_rename(&state(&root), "b1", "不存在的人", "新人").await.unwrap_err();
        assert_eq!(err, "No occurrences of \"不存在的人\" were found in \"b1\".");
        // 路径分隔符拒绝。
        let err = execute_entity_rename(&state(&root), "b1", "林震", "../逃逸").await.unwrap_err();
        assert_eq!(
            err,
            "Invalid rename target \"../逃逸\": entity names cannot contain path separators."
        );
    }

    #[tokio::test]
    async fn chapter_local_edit_three_level_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root).await;
        let book = root.join("books").join("b1");

        // ① 精确替换 + 归档 + 索引复核标记。
        let result = execute_chapter_local_edit(
            &state(&root),
            "b1",
            3,
            "旧正文。",
            "新正文。",
            1_782_864_000_000,
            "2026-07-01T00:00:00.000Z",
        )
        .await
        .unwrap();
        assert_eq!(result.transaction_type, "chapter-local-edit");
        assert_eq!(result.summary, "Patched chapter 3 and marked it for review.");
        assert_eq!(result.touched_files[0], "chapters/0003_Demo.md");
        let content = tokio::fs::read_to_string(book.join("chapters").join("0003_Demo.md")).await.unwrap();
        assert_eq!(content, "# 第3章 旧稿\n\n新正文。");
        let index: Vec<ChapterMeta> = serde_json::from_str(
            &tokio::fs::read_to_string(book.join("chapters").join("index.json")).await.unwrap(),
        )
        .unwrap();
        assert_eq!(index[0].audit_issues, vec![format!("[warning] {MANUAL_TEXT_EDIT_ISSUE}")]);

        // ② 弹性空白：内容空白形态与目标不同（多个空格 vs 换行缩进）仍命中。
        tokio::fs::write(book.join("chapters").join("0003_Demo.md"), "# 第3章 旧稿\n\n新   正文。")
            .await
            .unwrap();
        let result = execute_chapter_local_edit(
            &state(&root),
            "b1",
            3,
            "新\n  正文。",
            "弹性正文。",
            0,
            "2026-07-02T00:00:00.000Z",
        )
        .await
        .unwrap();
        assert!(result.summary.starts_with("Patched chapter 3"));
        let content = tokio::fs::read_to_string(book.join("chapters").join("0003_Demo.md")).await.unwrap();
        assert_eq!(content, "# 第3章 旧稿\n\n弹性正文。");

        // ③ 近似段落：目标为段落的近似改写（≥24 码元 + dice 阈值）。
        tokio::fs::write(
            book.join("chapters").join("0003_Demo.md"),
            "# 第3章 旧稿\n\n他缓缓抬起头来望向远方的山峦，山风吹动他的衣角猎猎作响。\n\n第二段保持原样不动。",
        )
        .await
        .unwrap();
        let result = execute_chapter_local_edit(
            &state(&root),
            "b1",
            3,
            "他缓缓抬起头来望向远方的山峦，山风吹动他的衣角猎猎作响！",
            "近似替换后的段落。",
            0,
            "2026-07-03T00:00:00.000Z",
        )
        .await
        .unwrap();
        assert_eq!(result.transaction_type, "chapter-local-edit");
        let content = tokio::fs::read_to_string(book.join("chapters").join("0003_Demo.md")).await.unwrap();
        assert_eq!(content, "# 第3章 旧稿\n\n近似替换后的段落。\n\n第二段保持原样不动。");

        // 未命中 → 错误文案逐字；缺 targetText → 前置守卫。
        let err = execute_chapter_local_edit(&state(&root), "b1", 3, "根本不存在的文字", "x", 0, "t")
            .await
            .unwrap_err();
        assert_eq!(err, "Target text was not found in chapter 3.");
        let err = execute_chapter_local_edit(&state(&root), "b1", 3, "", "x", 0, "t").await.unwrap_err();
        assert_eq!(err, "Chapter-local edits require targetText and replacementText.");
    }
}
