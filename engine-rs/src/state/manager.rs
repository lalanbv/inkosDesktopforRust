//! StateManager —— 项目/书籍状态层（write-next 所需子集）。
//!
//! 移植自 `packages/core/src/state/manager.ts`（821 行）的核心面：
//! book.json 存取、控制文档保障（author_intent/current_focus/style_guide
//! 方法论注入）、durable 章节进度、章节索引（含文件重建）、状态快照。
//!
//! 暂缓件（随后续端点按需移植）：acquireBookLock 文件锁（跨进程互斥——
//! strangler 模式下 Node/Rust 不同域端点不同时写同一书，41 号 runner 先用
//! 进程内 per-book 锁）、restore/rollback、isCompleteBookDirectory、
//! project config 存取。
//!
//! ## 移植纪律
//! - `resolveControlDocumentLanguage` 的怪癖：book.json 缺失/解析失败默认
//!   **en**；仅 `language === "zh"` 判 zh（其余值含 "en" 均落 en）
//! - 索引重建的文件名模式 `^(\d+)[_-]?(.*?)\.md$`：标题取余部（去前导 `_`、
//!   `_`→空格），空 → `第N章`；字数 = 去全部空白后的 UTF-16 码元数
//! - saveChapterIndex 的空索引护栏：空 + 无豁免 → 先重建，重建仍空才落盘空

use std::path::{Path, PathBuf};

use regex::Regex;
use std::sync::OnceLock;

use crate::models::book::BookConfig;
use crate::models::chapter::{ChapterMeta, ChapterStatus};
use crate::state::state_bootstrap::{bootstrap_structured_state_from_markdown, resolve_durable_story_progress};
use crate::state::store::FsStateStore;
use crate::utils::language::WritingLanguage;
use crate::utils::utc_time::utc_now_iso;
use crate::utils::writing_methodology::build_writing_methodology_section;

/// 项目状态管理器。
#[derive(Debug, Clone)]
pub struct StateManager {
    project_root: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum StateManagerError {
    #[error("book.json is empty for book \"{book_id}\"")]
    EmptyBookConfig { book_id: String },
    #[error("book.json is invalid for book \"{book_id}\": {message}")]
    InvalidBookConfig { book_id: String, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl StateManager {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        StateManager { project_root: project_root.into() }
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn books_dir(&self) -> PathBuf {
        self.project_root.join("books")
    }

    pub fn book_dir(&self, book_id: &str) -> PathBuf {
        self.books_dir().join(book_id)
    }

    pub fn state_dir(&self, book_id: &str) -> PathBuf {
        self.book_dir(book_id).join("story").join("state")
    }

    /// 读取 book.json（空文件报错；解析失败报错）。
    pub async fn load_book_config(&self, book_id: &str) -> Result<BookConfig, StateManagerError> {
        let raw = tokio::fs::read_to_string(self.book_dir(book_id).join("book.json")).await?;
        if raw.trim().is_empty() {
            return Err(StateManagerError::EmptyBookConfig { book_id: book_id.to_string() });
        }
        serde_json::from_str(&raw).map_err(|e| StateManagerError::InvalidBookConfig {
            book_id: book_id.to_string(),
            message: e.to_string(),
        })
    }

    /// 写 book.json（pretty + 递归建目录）。
    pub async fn save_book_config(&self, book_id: &str, config: &BookConfig) -> Result<(), StateManagerError> {
        self.save_book_config_at(&self.book_dir(book_id), config).await
    }

    pub async fn save_book_config_at(
        &self,
        book_dir: &Path,
        config: &BookConfig,
    ) -> Result<(), StateManagerError> {
        tokio::fs::create_dir_all(book_dir).await?;
        let serialized = serde_json::to_string_pretty(config)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        tokio::fs::write(book_dir.join("book.json"), serialized).await?;
        Ok(())
    }

    /// 控制文档语言：book.json 的 language 严格等于 "zh" 才 zh；缺失/其余 → en。
    pub async fn resolve_control_document_language(&self, book_id: &str) -> WritingLanguage {
        let Ok(raw) = tokio::fs::read_to_string(self.book_dir(book_id).join("book.json")).await else {
            return WritingLanguage::En;
        };
        match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(value) if value.get("language").and_then(|v| v.as_str()) == Some("zh") => {
                WritingLanguage::Zh
            }
            _ => WritingLanguage::En,
        }
    }

    /// 保障控制文档（5 目录 + author_intent/current_focus + style_guide 方法论）。
    pub async fn ensure_control_documents(&self, book_id: &str, author_intent: Option<&str>) -> Result<(), StateManagerError> {
        let language = self.resolve_control_document_language(book_id).await;
        self.ensure_control_documents_at(&self.book_dir(book_id), language, author_intent)
            .await
    }

    pub async fn ensure_control_documents_at(
        &self,
        book_dir: &Path,
        language: WritingLanguage,
        author_intent: Option<&str>,
    ) -> Result<(), StateManagerError> {
        let story_dir = book_dir.join("story");
        let runtime_dir = story_dir.join("runtime");
        let outline_dir = story_dir.join("outline");
        let roles_major = story_dir.join("roles").join("主要角色");
        let roles_minor = story_dir.join("roles").join("次要角色");

        for dir in [&story_dir, &runtime_dir, &outline_dir, &roles_major, &roles_minor] {
            tokio::fs::create_dir_all(dir).await?;
        }

        let author_intent_content = match author_intent.map(str::trim) {
            Some(trimmed) if !trimmed.is_empty() => format!("{}\n", trimmed.trim_end()),
            _ => default_author_intent(language).to_string(),
        };
        write_if_missing(&story_dir.join("author_intent.md"), &author_intent_content).await?;
        write_if_missing(&story_dir.join("current_focus.md"), default_current_focus(language)).await?;

        // style_guide 必含方法论（无参照文本也要有）。
        let style_guide_path = story_dir.join("style_guide.md");
        match tokio::fs::read_to_string(&style_guide_path).await {
            Ok(existing) => {
                if !existing.contains("写作方法论") && !existing.contains("Writing Methodology") {
                    let updated =
                        format!("{existing}\n\n{}", build_writing_methodology_section(language));
                    tokio::fs::write(&style_guide_path, updated).await?;
                }
            }
            Err(_) => {
                tokio::fs::write(
                    &style_guide_path,
                    build_writing_methodology_section(language),
                )
                .await?;
            }
        }
        Ok(())
    }

    /// 下一章号：durable 连续工件链权威（结构化状态只 bootstrap 不采信）。
    pub async fn get_next_chapter_number(&self, book_id: &str) -> Result<u32, StateManagerError> {
        let book_dir = self.book_dir(book_id);
        // 失败按 0 处理（durable 链缺失 = 新书起始）。
        let durable_chapter = resolve_durable_story_progress(
            &FsStateStore,
            &book_dir.to_string_lossy(),
            None,
        )
        .await
        .unwrap_or(0);
        let _ = bootstrap_structured_state_from_markdown(
            &FsStateStore,
            &book_dir.to_string_lossy(),
            Some(durable_chapter as u32),
        )
        .await;
        Ok(durable_chapter as u32 + 1)
    }

    /// 已落盘章节数（`NNN_*.md` 去重计数）。
    pub async fn get_persisted_chapter_count(&self, book_id: &str) -> Result<usize, StateManagerError> {
        let chapters_dir = self.book_dir(book_id).join("chapters");
        let mut chapter_numbers: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let Ok(mut entries) = tokio::fs::read_dir(&chapters_dir).await else {
            return Ok(0);
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(captures) = chapter_file_re().captures(&name) {
                if let Ok(number) = captures[1].parse::<u32>() {
                    chapter_numbers.insert(number);
                }
            }
        }
        Ok(chapter_numbers.len())
    }

    /// 读取章节索引（空数组/损坏 → 文件重建兜底）。
    pub async fn load_chapter_index(&self, book_id: &str) -> Result<Vec<ChapterMeta>, StateManagerError> {
        let index_path = self.book_dir(book_id).join("chapters").join("index.json");
        match tokio::fs::read_to_string(&index_path).await {
            Ok(raw) => {
                let Ok(parsed) = serde_json::from_str::<Vec<ChapterMeta>>(&raw) else {
                    let rebuilt = self.rebuild_chapter_index_from_files_at(&self.book_dir(book_id)).await;
                    if !rebuilt.is_empty() {
                        return Ok(rebuilt);
                    }
                    return Ok(Vec::new());
                };
                if !parsed.is_empty() {
                    return Ok(parsed);
                }
                let rebuilt = self.rebuild_chapter_index_from_files_at(&self.book_dir(book_id)).await;
                if !rebuilt.is_empty() {
                    return Ok(rebuilt);
                }
                Ok(parsed)
            }
            Err(_) => {
                let rebuilt = self.rebuild_chapter_index_from_files_at(&self.book_dir(book_id)).await;
                if !rebuilt.is_empty() {
                    return Ok(rebuilt);
                }
                Ok(Vec::new())
            }
        }
    }

    /// 从章节文件重建索引（`NNN标题.md` / `NNN-标题.md` / `NNN.md`）。
    pub async fn rebuild_chapter_index_from_files_at(&self, book_dir: &Path) -> Vec<ChapterMeta> {
        let chapters_dir = book_dir.join("chapters");
        let Ok(mut entries) = tokio::fs::read_dir(&chapters_dir).await else {
            return Vec::new();
        };

        let mut rows: Vec<ChapterMeta> = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(captures) = rebuild_file_re().captures(&name) else {
                continue;
            };
            let Ok(number) = captures[1].parse::<u32>() else {
                continue;
            };
            if number == 0 {
                continue;
            }
            let file_path = chapters_dir.join(&name);
            let timestamp = entry
                .metadata()
                .await
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| {
                    crate::utils::utc_time::unix_to_utc_iso(
                        duration.as_secs() as i64,
                        duration.subsec_millis(),
                    )
                })
                .unwrap_or_else(utc_now_iso);
            let content = tokio::fs::read_to_string(&file_path).await.unwrap_or_default();
            // TS content.replace(/\s+/g, "").length：去全部空白后的码元数。
            let word_count = content
                .chars()
                .filter(|c| !c.is_whitespace())
                .count() as u32;
            // 标题：余部去前导 _、_ → 空格；空 → 第N章。
            let raw_title = captures
                .get(2)
                .map(|m| m.as_str())
                .unwrap_or("");
            let title = title_from_filename(raw_title, number);

            rows.push(ChapterMeta {
                number,
                title,
                status: ChapterStatus::ReadyForReview,
                word_count,
                created_at: timestamp.clone(),
                updated_at: timestamp,
                audit_issues: Vec::new(),
                length_warnings: Vec::new(),
                review_note: None,
                detection_score: None,
                detection_provider: None,
                detected_at: None,
                length_telemetry: None,
                token_usage: None,
            });
        }
        rows.sort_by(|a, b| a.number.cmp(&b.number));
        rows
    }

    /// 写章节索引（空 + 无豁免 → 重建护栏）。
    pub async fn save_chapter_index(
        &self,
        book_id: &str,
        index: &[ChapterMeta],
    ) -> Result<(), StateManagerError> {
        self.save_chapter_index_at(&self.book_dir(book_id), index, false).await
    }

    pub async fn save_chapter_index_at(
        &self,
        book_dir: &Path,
        index: &[ChapterMeta],
        allow_empty_with_chapter_files: bool,
    ) -> Result<(), StateManagerError> {
        let chapters_dir = book_dir.join("chapters");
        tokio::fs::create_dir_all(&chapters_dir).await?;
        let safe_index: Vec<ChapterMeta> = if index.is_empty() && !allow_empty_with_chapter_files {
            let rebuilt = self.rebuild_chapter_index_from_files_at(book_dir).await;
            if !rebuilt.is_empty() { rebuilt } else { index.to_vec() }
        } else {
            index.to_vec()
        };
        let serialized = serde_json::to_string_pretty(&safe_index)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        tokio::fs::write(chapters_dir.join("index.json"), serialized).await?;
        Ok(())
    }

    /// 状态快照：7 个 markdown 真相文件 + state/*.json → story/snapshots/{n}/。
    pub async fn snapshot_state(&self, book_id: &str, chapter_number: u32) -> Result<(), StateManagerError> {
        self.snapshot_state_at(&self.book_dir(book_id), chapter_number).await
    }

    /// 列出 books/ 下含 book.json 的目录名（48 号；对齐 TS listBooks，目录序）。
    pub async fn list_books(&self) -> Vec<String> {
        let mut book_ids = Vec::new();
        let Ok(mut entries) = tokio::fs::read_dir(self.books_dir()).await else {
            return book_ids;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if tokio::fs::try_exists(entry.path().join("book.json")).await.unwrap_or(false) {
                book_ids.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        book_ids
    }

    /// 从快照恢复真相文件（48 号；对齐 TS restoreState）。
    /// 必需文件（current_state/pending_hooks）任一缺失 → false；
    /// 可选文件快照缺失时删除目标（回退到快照时刻的状态）。
    pub async fn restore_state(&self, book_id: &str, chapter_number: u32) -> bool {
        let story_dir = self.book_dir(book_id).join("story");
        let snapshot_dir = story_dir.join("snapshots").join(chapter_number.to_string());
        const REQUIRED: [&str; 2] = ["current_state.md", "pending_hooks.md"];
        const OPTIONAL: [&str; 5] = [
            "particle_ledger.md",
            "chapter_summaries.md",
            "subplot_board.md",
            "emotional_arcs.md",
            "character_matrix.md",
        ];
        for file in REQUIRED {
            let Ok(content) = tokio::fs::read_to_string(snapshot_dir.join(file)).await else {
                return false;
            };
            if tokio::fs::write(story_dir.join(file), content).await.is_err() {
                return false;
            }
        }
        for file in OPTIONAL {
            let target = story_dir.join(file);
            match tokio::fs::read_to_string(snapshot_dir.join(file)).await {
                Ok(content) => {
                    let _ = tokio::fs::write(&target, content).await;
                }
                Err(_) => {
                    let _ = tokio::fs::remove_file(&target).await;
                }
            }
        }
        // 结构化 state/：快照有内容则恢复，否则整目录删除。
        let state_dir = self.state_dir(book_id);
        let snapshot_state_dir = snapshot_dir.join("state");
        let mut restored_structured = false;
        if let Ok(mut entries) = tokio::fs::read_dir(&snapshot_state_dir).await {
            let mut files: Vec<std::path::PathBuf> = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                files.push(entry.path());
            }
            if !files.is_empty() {
                restored_structured = true;
                let _ = tokio::fs::create_dir_all(&state_dir).await;
                for path in files {
                    if let Ok(content) = tokio::fs::read_to_string(&path).await {
                        let _ = tokio::fs::write(state_dir.join(path.file_name().unwrap_or_default()), content).await;
                    }
                }
            }
        }
        if !restored_structured {
            let _ = tokio::fs::remove_dir_all(&state_dir).await;
        }
        true
    }

    /// 回滚到指定章快照：删除其后所有章节产物（md/快照/runtime/drafts/
    /// sqlite 加速索引），索引收敛到 kept。返回被废弃的章号列表
    /// （48 号；对齐 TS rollbackToChapter）。
    pub async fn rollback_to_chapter(
        &self,
        book_id: &str,
        target_chapter: u32,
    ) -> Result<Vec<u32>, String> {
        if !self.restore_state(book_id, target_chapter).await {
            return Err(format!(
                "Cannot restore snapshot for chapter {target_chapter} in \"{book_id}\""
            ));
        }
        let book_dir = self.book_dir(book_id);
        let chapters_dir = book_dir.join("chapters");
        let index = self
            .load_chapter_index(book_id)
            .await
            .map_err(|e| e.to_string())?;

        let kept: Vec<ChapterMeta> = index.iter().filter(|m| m.number <= target_chapter).cloned().collect();
        let discarded: Vec<u32> = index.iter().filter(|m| m.number > target_chapter).map(|m| m.number).collect();

        // 章节正文与草稿（`NN_title.md` 前缀数字 > target）。
        for dir in [chapters_dir, book_dir.join("story").join("drafts")] {
            if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let Some(num) = leading_number(&name) else { continue };
                    if num > target_chapter {
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
        // 快照目录（目录名数字 > target）。
        let snapshots_dir = book_dir.join("story").join("snapshots");
        if let Ok(mut entries) = tokio::fs::read_dir(&snapshots_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Ok(num) = name.parse::<u32>() {
                    if num > target_chapter {
                        let _ = tokio::fs::remove_dir_all(entry.path()).await;
                    }
                }
            }
        }
        // runtime 工件（`chapter-NN.` 前缀 > target）。
        let runtime_dir = book_dir.join("story").join("runtime");
        if let Ok(mut entries) = tokio::fs::read_dir(&runtime_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(num) = name.strip_prefix("chapter-").and_then(|rest| {
                    rest.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok()
                }) else { continue };
                if num > target_chapter {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
        }
        // sqlite 加速索引（防废弃章回流检索）。
        for db_file in ["memory.db", "memory.db-shm", "memory.db-wal"] {
            let _ = tokio::fs::remove_file(book_dir.join("story").join(db_file)).await;
        }

        self.save_chapter_index(book_id, &kept)
            .await
            .map_err(|e| e.to_string())?;
        Ok(discarded)
    }

    pub async fn snapshot_state_at(&self, book_dir: &Path, chapter_number: u32) -> Result<(), StateManagerError> {
        let story_dir = book_dir.join("story");
        let snapshot_dir = story_dir.join("snapshots").join(chapter_number.to_string());
        tokio::fs::create_dir_all(&snapshot_dir).await?;

        const FILES: [&str; 7] = [
            "current_state.md",
            "particle_ledger.md",
            "pending_hooks.md",
            "chapter_summaries.md",
            "subplot_board.md",
            "emotional_arcs.md",
            "character_matrix.md",
        ];
        for file in FILES {
            if let Ok(content) = tokio::fs::read_to_string(story_dir.join(file)).await {
                tokio::fs::write(snapshot_dir.join(file), content).await?;
            }
        }

        let state_dir = story_dir.join("state");
        let snapshot_state_dir = snapshot_dir.join("state");
        if let Ok(mut entries) = tokio::fs::read_dir(&state_dir).await {
            let mut files: Vec<String> = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                files.push(entry.file_name().to_string_lossy().into_owned());
            }
            if !files.is_empty() {
                tokio::fs::create_dir_all(&snapshot_state_dir).await?;
                for file in files {
                    if let Ok(content) = tokio::fs::read_to_string(state_dir.join(&file)).await {
                        tokio::fs::write(snapshot_state_dir.join(file), content).await?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn default_author_intent(language: WritingLanguage) -> &'static str {
    if language == WritingLanguage::En {
        "# Author Intent\n\n(Describe the long-horizon vision for this book here.)\n"
    } else {
        "# 作者意图\n\n（在这里描述这本书的长期创作方向。）\n"
    }
}

fn default_current_focus(language: WritingLanguage) -> &'static str {
    if language == WritingLanguage::En {
        "# Current Focus\n\n## Active Focus\n\n(Describe what the next 1-3 chapters should prioritize.)\n"
    } else {
        "# 当前聚焦\n\n## 当前重点\n\n（描述接下来 1-3 章最需要优先推进的内容。）\n"
    }
}

/// 标题化：去前导 `_`、`_` → 空格、trim；空 → `第N章`。
fn title_from_filename(raw: &str, number: u32) -> String {
    let stripped = raw.strip_prefix('_').unwrap_or(raw);
    let spaced = stripped.replace('_', " ").trim().to_string();
    if spaced.is_empty() {
        format!("第{number}章")
    } else {
        spaced
    }
}

async fn write_if_missing(path: &Path, content: &str) -> Result<(), StateManagerError> {
    if tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(());
    }
    tokio::fs::write(path, content).await?;
    Ok(())
}

fn chapter_file_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\d+)_.*\.md$").unwrap())
}

/// `^(\d+)_.*\.md$` 前缀数字（rollback 的章节/草稿文件匹配，复用
/// chapter_file_re 同一模式）。
fn leading_number(name: &str) -> Option<u32> {
    chapter_file_re()
        .captures(name)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

fn rebuild_file_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\d+)[_-]?(.*?)\.md$").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_from_filename_matrix() {
        assert_eq!(title_from_filename("夜探藏书阁", 3), "夜探藏书阁");
        assert_eq!(title_from_filename("_夜探", 3), "夜探");
        assert_eq!(title_from_filename("夜_探", 3), "夜 探");
        assert_eq!(title_from_filename("", 3), "第3章");
        assert_eq!(title_from_filename("  ", 3), "第3章");
    }

    fn book_config(id: &str) -> BookConfig {
        BookConfig {
            id: id.to_string(),
            title: "书".to_string(),
            platform: crate::models::book::Platform::Other,
            genre: "other".to_string(),
            status: crate::models::book::BookStatus::Active,
            target_chapters: 10,
            chapter_word_count: 3000,
            language: Some("zh".to_string()),
            created_at: String::new(),
            updated_at: String::new(),
            parent_book_id: None,
            fanfic_mode: None,
            writing: None,
        }
    }

    #[tokio::test]
    async fn book_config_roundtrip_and_empty_guard() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        manager.save_book_config("b1", &book_config("b1")).await.unwrap();
        let loaded = manager.load_book_config("b1").await.unwrap();
        assert_eq!(loaded.title, "书");

        let book_dir = manager.book_dir("b2");
        tokio::fs::create_dir_all(&book_dir).await.unwrap();
        tokio::fs::write(book_dir.join("book.json"), "   ").await.unwrap();
        assert!(matches!(
            manager.load_book_config("b2").await.unwrap_err(),
            StateManagerError::EmptyBookConfig { .. }
        ));
    }

    #[tokio::test]
    async fn control_document_language_quirk() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        // book.json 缺失 → en。
        assert_eq!(manager.resolve_control_document_language("x").await, WritingLanguage::En);

        let book_dir = manager.book_dir("zh-book");
        tokio::fs::create_dir_all(&book_dir).await.unwrap();
        tokio::fs::write(book_dir.join("book.json"), r#"{"language":"zh"}"#).await.unwrap();
        assert_eq!(manager.resolve_control_document_language("zh-book").await, WritingLanguage::Zh);

        tokio::fs::write(book_dir.join("book.json"), r#"{"language":"en"}"#).await.unwrap();
        assert_eq!(manager.resolve_control_document_language("zh-book").await, WritingLanguage::En);
    }

    #[tokio::test]
    async fn ensure_control_documents_creates_defaults_and_methodology() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let book_dir = manager.book_dir("b1");
        tokio::fs::create_dir_all(&book_dir).await.unwrap();
        tokio::fs::write(book_dir.join("book.json"), r#"{"language":"zh"}"#).await.unwrap();

        manager
            .ensure_control_documents_at(&book_dir, WritingLanguage::Zh, Some("  自定义意图  "))
            .await
            .unwrap();
        let intent = tokio::fs::read_to_string(book_dir.join("story").join("author_intent.md"))
            .await
            .unwrap();
        assert_eq!(intent, "自定义意图\n");
        let focus = tokio::fs::read_to_string(book_dir.join("story").join("current_focus.md"))
            .await
            .unwrap();
        assert!(focus.contains("（描述接下来 1-3 章最需要优先推进的内容。）"));
        let style = tokio::fs::read_to_string(book_dir.join("story").join("style_guide.md"))
            .await
            .unwrap();
        assert!(style.contains("写作方法论"));

        // 二次执行不重复注入方法论；已有 author_intent 不覆盖。
        manager
            .ensure_control_documents_at(&book_dir, WritingLanguage::Zh, None)
            .await
            .unwrap();
        let style_again = tokio::fs::read_to_string(book_dir.join("story").join("style_guide.md"))
            .await
            .unwrap();
        assert_eq!(style_again, style);
        let intent_again = tokio::fs::read_to_string(book_dir.join("story").join("author_intent.md"))
            .await
            .unwrap();
        assert_eq!(intent_again, "自定义意图\n");
    }

    #[tokio::test]
    async fn next_chapter_number_from_durable_chain() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let chapters = manager.book_dir("b1").join("chapters");
        tokio::fs::create_dir_all(&chapters).await.unwrap();
        tokio::fs::write(chapters.join("0001_初醒.md"), "# 第1章 初醒\n正文").await.unwrap();
        tokio::fs::write(chapters.join("0002_风起.md"), "# 第2章 风起\n正文").await.unwrap();
        // 断链：0004 存在 0003 缺失 → durable 到 2。
        tokio::fs::write(chapters.join("0004_断链.md"), "# 第4章\n正文").await.unwrap();
        assert_eq!(manager.get_next_chapter_number("b1").await.unwrap(), 3);
        assert_eq!(manager.get_persisted_chapter_count("b1").await.unwrap(), 3);
    }

    #[tokio::test]
    async fn chapter_index_rebuild_and_empty_guard() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let chapters = manager.book_dir("b1").join("chapters");
        tokio::fs::create_dir_all(&chapters).await.unwrap();
        tokio::fs::write(chapters.join("0001_初醒.md"), "# 第1章\n正 文").await.unwrap();

        // index.json 缺失 → 重建。
        let index = manager.load_chapter_index("b1").await.unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].number, 1);
        assert_eq!(index[0].title, "初醒");
        assert_eq!(index[0].status, ChapterStatus::ReadyForReview);
        // TS replace(/\s+/g, "") 不剥标题行："#第1章正文" = 6 码元。
        assert_eq!(index[0].word_count, 6);

        // 空索引落盘护栏：文件存在时空索引被重建替换。
        manager.save_chapter_index("b1", &[]).await.unwrap();
        let saved: Vec<ChapterMeta> =
            serde_json::from_str(&tokio::fs::read_to_string(chapters.join("index.json")).await.unwrap())
                .unwrap();
        assert_eq!(saved.len(), 1);

        // 非空索引正常落盘。
        let mut custom = index.clone();
        custom[0].title = "手改标题".into();
        manager.save_chapter_index("b1", &custom).await.unwrap();
        let reloaded = manager.load_chapter_index("b1").await.unwrap();
        assert_eq!(reloaded[0].title, "手改标题");
    }

    #[tokio::test]
    async fn snapshot_copies_truth_and_state_files() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let story = manager.book_dir("b1").join("story");
        tokio::fs::create_dir_all(story.join("state")).await.unwrap();
        tokio::fs::write(story.join("current_state.md"), "状态").await.unwrap();
        tokio::fs::write(story.join("state").join("hooks.json"), "{}").await.unwrap();
        // 不存在的文件静默跳过。
        manager.snapshot_state("b1", 2).await.unwrap();
        let snapshot = story.join("snapshots").join("2");
        assert_eq!(
            tokio::fs::read_to_string(snapshot.join("current_state.md")).await.unwrap(),
            "状态"
        );
        assert_eq!(
            tokio::fs::read_to_string(snapshot.join("state").join("hooks.json")).await.unwrap(),
            "{}"
        );
        assert!(!snapshot.join("pending_hooks.md").exists());
    }
}
