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
use crate::state::state_bootstrap::{
    bootstrap_structured_state_from_markdown, resolve_durable_story_progress,
};
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

/// [`StateManager::trash_book`] 的结果（R58/1676 号）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashBook {
    /// 书目录已移进回收站；值是它在回收站里的路径。
    Trashed(PathBuf),
    /// 没有这本书。
    Missing,
}

impl StateManager {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        StateManager {
            project_root: project_root.into(),
        }
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

    /// 项目级回收站（R58/1676 号）：`{projectRoot}/.trash/books/`——与 books 根同级。
    pub fn book_trash_dir(&self) -> PathBuf {
        self.project_root.join(".trash").join("books")
    }

    /// 删书：整个书目录移进项目级回收站，不硬删。
    ///
    /// 返回 [`TrashBook::Trashed`]（移走了，带回收站里的路径）或 [`TrashBook::Missing`]
    /// （没有这本书——与之前的删除一样不报错）。rename 失败直接返回错误，不降级为硬删。
    /// 回收站在 books 根之外，所以 `book_id` 必须是安全的路径段（否则 `books/<id>`
    /// 拼出来的就不是书目录）。
    pub async fn trash_book(&self, book_id: &str, now_millis: u64) -> Result<TrashBook, String> {
        if !crate::utils::is_safe_book_id(book_id) {
            return Err("Invalid book id".to_string());
        }
        let book_dir = self.book_dir(book_id);
        if !tokio::fs::try_exists(&book_dir).await.unwrap_or(false) {
            return Ok(TrashBook::Missing);
        }
        let trash_dir = self.book_trash_dir();
        let entry = crate::state::trash::reserve_dir(
            &trash_dir,
            &crate::state::trash::trashed_book_name(book_id, now_millis),
        )
        .await
        .map_err(|error| format!("Cannot create the book trash: {error}"))?;
        let trashed = trash_dir.join(&entry);
        tokio::fs::rename(&book_dir, &trashed)
            .await
            .map_err(|error| {
                // 目标目录是这个函数刚建出来的空目录；移不进去就不留它。
                let _ = std::fs::remove_dir(&trashed);
                format!("Cannot move book \"{book_id}\" to the trash: {error}")
            })?;
        Ok(TrashBook::Trashed(trashed))
    }

    pub fn state_dir(&self, book_id: &str) -> PathBuf {
        self.book_dir(book_id).join("story").join("state")
    }

    /// 读取 book.json（空文件报错；解析失败报错）。
    pub async fn load_book_config(&self, book_id: &str) -> Result<BookConfig, StateManagerError> {
        let raw = tokio::fs::read_to_string(self.book_dir(book_id).join("book.json")).await?;
        if raw.trim().is_empty() {
            return Err(StateManagerError::EmptyBookConfig {
                book_id: book_id.to_string(),
            });
        }
        serde_json::from_str(&raw).map_err(|e| StateManagerError::InvalidBookConfig {
            book_id: book_id.to_string(),
            message: e.to_string(),
        })
    }

    /// 写 book.json（pretty + 递归建目录）。
    pub async fn save_book_config(
        &self,
        book_id: &str,
        config: &BookConfig,
    ) -> Result<(), StateManagerError> {
        self.save_book_config_at(&self.book_dir(book_id), config)
            .await
    }

    pub async fn save_book_config_at(
        &self,
        book_dir: &Path,
        config: &BookConfig,
    ) -> Result<(), StateManagerError> {
        tokio::fs::create_dir_all(book_dir).await?;
        let serialized = serde_json::to_string_pretty(config)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        // 199 号：book.json 损坏 = 整本书不可加载——原子替换写。
        crate::utils::atomic_file_set::write_file_atomic(&book_dir.join("book.json"), &serialized)
            .await?;
        Ok(())
    }

    /// 控制文档语言：book.json 的 language 严格等于 "zh" 才 zh；缺失/其余 → en。
    pub async fn resolve_control_document_language(&self, book_id: &str) -> WritingLanguage {
        let Ok(raw) = tokio::fs::read_to_string(self.book_dir(book_id).join("book.json")).await
        else {
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
    pub async fn ensure_control_documents(
        &self,
        book_id: &str,
        author_intent: Option<&str>,
    ) -> Result<(), StateManagerError> {
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

        for dir in [
            &story_dir,
            &runtime_dir,
            &outline_dir,
            &roles_major,
            &roles_minor,
        ] {
            tokio::fs::create_dir_all(dir).await?;
        }

        let author_intent_content = match author_intent.map(str::trim) {
            Some(trimmed) if !trimmed.is_empty() => format!("{}\n", trimmed.trim_end()),
            _ => default_author_intent(language).to_string(),
        };
        write_if_missing(&story_dir.join("author_intent.md"), &author_intent_content).await?;
        write_if_missing(
            &story_dir.join("current_focus.md"),
            default_current_focus(language),
        )
        .await?;

        // style_guide 必含方法论（无参照文本也要有）。
        let style_guide_path = story_dir.join("style_guide.md");
        match tokio::fs::read_to_string(&style_guide_path).await {
            Ok(existing) => {
                if !existing.contains("写作方法论") && !existing.contains("Writing Methodology")
                {
                    let updated = format!(
                        "{existing}\n\n{}",
                        build_writing_methodology_section(language)
                    );
                    crate::utils::atomic_file_set::write_file_atomic(&style_guide_path, &updated)
                        .await?;
                }
            }
            Err(_) => {
                crate::utils::atomic_file_set::write_file_atomic(
                    &style_guide_path,
                    &build_writing_methodology_section(language),
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
        let durable_chapter =
            resolve_durable_story_progress(&FsStateStore, &book_dir.to_string_lossy(), None)
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

    /// 已落盘章节数（按前导章号去重；`NNNN_`/`NNNN-`/`NNNN.md` 同列——807 号与重建同口径）。
    pub async fn get_persisted_chapter_count(
        &self,
        book_id: &str,
    ) -> Result<usize, StateManagerError> {
        let chapters_dir = self.book_dir(book_id).join("chapters");
        let mut chapter_numbers: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let Ok(mut entries) = tokio::fs::read_dir(&chapters_dir).await else {
            return Ok(0);
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(number) = leading_number(&name) {
                chapter_numbers.insert(number);
            }
        }
        Ok(chapter_numbers.len())
    }

    /// 读取章节索引（空数组/损坏 → 文件重建兜底）。
    pub async fn load_chapter_index(
        &self,
        book_id: &str,
    ) -> Result<Vec<ChapterMeta>, StateManagerError> {
        let index_path = self.book_dir(book_id).join("chapters").join("index.json");
        match tokio::fs::read_to_string(&index_path).await {
            Ok(raw) => {
                let Ok(parsed) = serde_json::from_str::<Vec<ChapterMeta>>(&raw) else {
                    let rebuilt = self
                        .rebuild_chapter_index_from_files_at(&self.book_dir(book_id))
                        .await;
                    if !rebuilt.is_empty() {
                        return Ok(rebuilt);
                    }
                    return Ok(Vec::new());
                };
                if !parsed.is_empty() {
                    return Ok(parsed);
                }
                let rebuilt = self
                    .rebuild_chapter_index_from_files_at(&self.book_dir(book_id))
                    .await;
                if !rebuilt.is_empty() {
                    return Ok(rebuilt);
                }
                Ok(parsed)
            }
            Err(_) => {
                let rebuilt = self
                    .rebuild_chapter_index_from_files_at(&self.book_dir(book_id))
                    .await;
                if !rebuilt.is_empty() {
                    return Ok(rebuilt);
                }
                Ok(Vec::new())
            }
        }
    }

    /// 从章节文件重建索引（`NNN标题.md` / `NNN-标题.md` / `NNN.md`）。
    ///
    /// 813 号：同章双形态按章号去重（[`select_chapter_files`]：正典 `NNNN_` 优先，其次
    /// 文件名序）——此前每个匹配文件各产一行，legacy 连字符旧章与重写后的正典章并存时
    /// 索引出现两条同号条目。
    pub async fn rebuild_chapter_index_from_files_at(&self, book_dir: &Path) -> Vec<ChapterMeta> {
        let chapters_dir = book_dir.join("chapters");
        let Ok(mut entries) = tokio::fs::read_dir(&chapters_dir).await else {
            return Vec::new();
        };
        let mut names: Vec<String> = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }

        let mut rows: Vec<ChapterMeta> = Vec::new();
        for name in select_chapter_files(names) {
            let Some(captures) = rebuild_file_re().captures(&name) else {
                continue;
            };
            let Ok(number) = captures[1].parse::<u32>() else {
                continue;
            };
            let file_path = chapters_dir.join(&name);
            let timestamp = tokio::fs::metadata(&file_path)
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
            let content = tokio::fs::read_to_string(&file_path)
                .await
                .unwrap_or_default();
            // TS content.replace(/\s+/g, "").length：去全部空白后的码元数。
            let word_count = content.chars().filter(|c| !c.is_whitespace()).count() as u32;
            // 标题：余部去前导 _、_ → 空格；空 → 第N章。
            let raw_title = captures.get(2).map(|m| m.as_str()).unwrap_or("");
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
                volume_id: None,
                review_note: None,
                detection_score: None,
                detection_provider: None,
                detected_at: None,
                length_telemetry: None,
                token_usage: None,
            });
        }
        rows.sort_by_key(|a| a.number);
        rows
    }

    /// 写章节索引（空 + 无豁免 → 重建护栏）。
    pub async fn save_chapter_index(
        &self,
        book_id: &str,
        index: &[ChapterMeta],
    ) -> Result<(), StateManagerError> {
        self.save_chapter_index_at(&self.book_dir(book_id), index, false)
            .await
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
            if !rebuilt.is_empty() {
                rebuilt
            } else {
                index.to_vec()
            }
        } else {
            index.to_vec()
        };
        let serialized = serde_json::to_string_pretty(&safe_index)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        // 199 号：index.json 截断虽可重建自愈，但会丢 reviewNote/审计元数据——原子替换写。
        crate::utils::atomic_file_set::write_file_atomic(
            &chapters_dir.join("index.json"),
            &serialized,
        )
        .await?;
        Ok(())
    }

    /// 状态快照：7 个 markdown 真相文件 + state/*.json → story/snapshots/{n}/。
    pub async fn snapshot_state(
        &self,
        book_id: &str,
        chapter_number: u32,
    ) -> Result<(), StateManagerError> {
        self.snapshot_state_at(&self.book_dir(book_id), chapter_number)
            .await
    }

    /// 列出 books/ 下含 book.json 的目录名（48 号；对齐 TS listBooks，目录序）。
    pub async fn list_books(&self) -> Vec<String> {
        let mut book_ids = Vec::new();
        let Ok(mut entries) = tokio::fs::read_dir(self.books_dir()).await else {
            return book_ids;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if tokio::fs::try_exists(entry.path().join("book.json"))
                .await
                .unwrap_or(false)
            {
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
            if crate::utils::atomic_file_set::write_file_atomic(&story_dir.join(file), &content)
                .await
                .is_err()
            {
                return false;
            }
        }
        for file in OPTIONAL {
            let target = story_dir.join(file);
            match tokio::fs::read_to_string(snapshot_dir.join(file)).await {
                Ok(content) => {
                    let _ =
                        crate::utils::atomic_file_set::write_file_atomic(&target, &content).await;
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
                        let _ = crate::utils::atomic_file_set::write_file_atomic(
                            &state_dir.join(path.file_name().unwrap_or_default()),
                            &content,
                        )
                        .await;
                    }
                }
            }
        }
        if !restored_structured {
            let _ = tokio::fs::remove_dir_all(&state_dir).await;
        }
        true
    }

    /// 回滚到指定章快照：目标章之后的章节产物（md/快照/runtime/drafts）不再硬删，
    /// 整批移进 `chapters/.trash/rollback-{ts}-{target}/`；索引收敛到 kept。返回被
    /// 废弃的章号列表。
    ///
    /// R58/1676 号（对齐 TS `rollbackToChapter`）。顺序：先把所有要废、要被覆盖的
    /// 条目移进回收站批次（整批移完才动别的），再恢复快照、删 sqlite 加速索引
    /// （可重建的派生物，不进回收站）、写索引。移的过程中或之后任何一步失败，已移项
    /// 按逆序 rename 回原路径——盘面与操作前一致，不再 `let _ =` 吞错。
    pub async fn rollback_to_chapter(
        &self,
        book_id: &str,
        target_chapter: u32,
    ) -> Result<Vec<u32>, String> {
        let book_dir = self.book_dir(book_id);
        let index = self
            .load_chapter_index(book_id)
            .await
            .map_err(|e| e.to_string())?;

        let kept: Vec<ChapterMeta> = index
            .iter()
            .filter(|m| m.number <= target_chapter)
            .cloned()
            .collect();
        let discarded: Vec<u32> = index
            .iter()
            .filter(|m| m.number > target_chapter)
            .map(|m| m.number)
            .collect();

        // 章节正文与草稿（`NNNN_title.md` / legacy `NNNN-title.md` 前缀数字 > target；
        // 807 号：与索引重建同口径，旧章不得逃过删除）。
        let mut relatives = Vec::new();
        for dir in ["chapters", "story/drafts"] {
            relatives.extend(
                discarded_by_number(&book_dir.join(dir), dir, target_chapter, leading_number).await,
            );
        }
        // 快照目录（目录名数字 > target）。
        relatives.extend(
            discarded_by_number(
                &book_dir.join("story").join("snapshots"),
                "story/snapshots",
                target_chapter,
                |name| name.parse::<u32>().ok(),
            )
            .await,
        );
        // runtime 工件（`chapter-NN.` 前缀 > target）。
        relatives.extend(
            discarded_by_number(
                &book_dir.join("story").join("runtime"),
                "story/runtime",
                target_chapter,
                |name| {
                    name.strip_prefix("chapter-").and_then(|rest| {
                        rest.chars()
                            .take_while(|c| c.is_ascii_digit())
                            .collect::<String>()
                            .parse::<u32>()
                            .ok()
                    })
                },
            )
            .await,
        );
        // 恢复快照会覆盖或删掉真相文件、结构化状态；索引会被改写。原样一并入批——
        // 批次因此是「回滚前」的完整切片（除可重建的派生物）。
        for file in crate::state::trash::ROLLBACK_TRUTH_FILES {
            relatives.push(format!("story/{file}"));
        }
        relatives.extend(
            crate::state::trash::ROLLBACK_TRUTH_PATHS
                .iter()
                .map(|path| (*path).to_string()),
        );

        let trash_parent = book_dir.join("chapters").join(".trash");
        let batch_name = crate::state::trash::reserve_dir(
            &trash_parent,
            &crate::state::trash::trash_batch_name(
                crate::state::trash::now_millis(),
                target_chapter,
            ),
        )
        .await
        .map_err(|error| format!("Rollback failed: cannot create the trash batch: {error}"))?;
        let batch = format!("chapters/.trash/{batch_name}");
        let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
        for relative in &relatives {
            if let Err(error) =
                crate::state::trash::move_into_batch(&book_dir, &batch, relative, &mut moved).await
            {
                let stranded = crate::state::trash::undo_moves(&moved).await;
                if stranded.is_empty() {
                    let _ = tokio::fs::remove_dir_all(book_dir.join(&batch)).await;
                }
                return Err(format!(
                    "Rollback failed while moving {relative} to the trash batch: {error}{}",
                    crate::state::trash::stranded_note(&stranded)
                ));
            }
        }
        if moved.is_empty() {
            // 没有可废的内容：不留一个空批次目录。
            let _ = tokio::fs::remove_dir(book_dir.join(&batch)).await;
        }

        if !self.restore_state(book_id, target_chapter).await {
            let stranded = crate::state::trash::undo_moves(&moved).await;
            if stranded.is_empty() {
                let _ = tokio::fs::remove_dir_all(book_dir.join(&batch)).await;
            }
            return Err(format!(
                "Cannot restore snapshot for chapter {target_chapter} in \"{book_id}\"{}",
                crate::state::trash::stranded_note(&stranded)
            ));
        }
        // sqlite 加速索引（防废弃章回流检索）。
        for db_file in ["memory.db", "memory.db-shm", "memory.db-wal"] {
            let _ = tokio::fs::remove_file(book_dir.join("story").join(db_file)).await;
        }

        if let Err(error) = self.save_chapter_index(book_id, &kept).await {
            let stranded = crate::state::trash::undo_moves(&moved).await;
            if stranded.is_empty() {
                let _ = tokio::fs::remove_dir_all(book_dir.join(&batch)).await;
            }
            return Err(format!(
                "Rollback failed while saving the chapter index: {}{}",
                error,
                crate::state::trash::stranded_note(&stranded)
            ));
        }
        Ok(discarded)
    }

    pub async fn snapshot_state_at(
        &self,
        book_dir: &Path,
        chapter_number: u32,
    ) -> Result<(), StateManagerError> {
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
                crate::utils::atomic_file_set::write_file_atomic(
                    &snapshot_dir.join(file),
                    &content,
                )
                .await?;
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
                        crate::utils::atomic_file_set::write_file_atomic(
                            &snapshot_state_dir.join(file),
                            &content,
                        )
                        .await?;
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
    // 779 号：首写撕裂的代价是「文件已存在但残缺」，后续 ensure 永不修复——
    // 原子写让崩溃收敛到「无文件（下次补种）或完整文件」。
    crate::utils::atomic_file_set::write_file_atomic(path, content).await?;
    Ok(())
}

/// 章节文件前导章号（`NNNN_标题.md` / `NNNN-标题.md` / `NNNN.md`）——rollback 的章节/
/// 草稿文件匹配与已落盘计数共用。
///
/// 807 号：与 [`rebuild_file_re`] 同口径。此前用仅认下划线的严格正则：legacy 连字符
/// 旧章（夹具 683 号故意播种的形态）在 rollback 删章时逃过，随后被空索引触发的宽容
/// 重建捞回=「reject 后章节复活」（差分器尾批 reject(1) node 404 vs rust 200 根因）。
/// 读侧认什么为章，删侧与计数侧必须认同一集合。
pub(crate) fn leading_number(name: &str) -> Option<u32> {
    rebuild_file_re()
        .captures(name)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

/// 回滚要废的条目：`dir` 下名字经 `parse` 得到的章号大于 `target` 的那些，列成
/// `{relative_dir}/<name>`（排序——批次内的移动顺序稳定）。目录不存在 → 空。
async fn discarded_by_number(
    dir: &Path,
    relative_dir: &str,
    target: u32,
    parse: impl Fn(&str) -> Option<u32>,
) -> Vec<String> {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let mut relatives = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if parse(&name).is_some_and(|number| number > target) {
            relatives.push(format!("{relative_dir}/{name}"));
        }
    }
    relatives.sort();
    relatives
}

/// 正典章节文件名（`NNNN_标题.md`，writer 落盘形态）。
///
/// 854 号：数字类用显式 `[0-9]` 而非 `\d`——regex crate 的 `\d` 默认 Unicode 数字类
///（`\p{Nd}`，含全角 `０１２` / 阿拉伯-印度 `٠١٢`），JS `\d` 恒 ASCII；两侧对同一
/// 文件名的判定须恒等（TS `chapter-files.ts` 同步注释）。
fn is_canonical_chapter_file(name: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[0-9]+_.*\.md$").unwrap())
        .is_match(name)
}

/// 845 号：同章双形态择优的唯一裁决——`candidate` 是否应取代 `current`：正典 `NNNN_` 优先，
/// 同类内文件名序（字节序，与 `Vec<String>::sort` 同序）小者。索引重建（[`select_chapter_files`]）
/// 与读侧定位（`chapter_filename::find_chapter_file_by_number`）共用此函数——此前两者各持一套
///（682 读侧按 padded 类内字典序取 `'-'<'_'` 的连字符旧章，813 索引侧正典优先），同一本书出现
/// 「索引列 `0001_新`、详情开 `0001-旧`」缺口。TS `preferredChapterFile` 同策略。
pub(crate) fn chapter_file_candidate_wins(current: &str, candidate: &str) -> bool {
    let current_canonical = is_canonical_chapter_file(current);
    let candidate_canonical = is_canonical_chapter_file(candidate);
    if candidate_canonical != current_canonical {
        candidate_canonical
    } else {
        candidate < current
    }
}

/// 813 号：同章双形态去重——同一前导章号多个文件时按 [`chapter_file_candidate_wins`] 择一；
/// 章号 0 跳过。返回按章号升序的唯一章文件名（TS `selectChapterFiles` 同策略）。
/// 双形态来源：legacy 连字符旧章 + 重写后的正典章并存（writer 清理同章旧文件此前只认
/// 下划线前缀，813 号已改按前导章号；此处为索引重建侧的恢复兜底）。
/// 845 号：择优内联比较取代预排序（单趟，无整表排序）。
pub(crate) fn select_chapter_files(names: Vec<String>) -> Vec<String> {
    use std::collections::btree_map::Entry;
    let mut by_number: std::collections::BTreeMap<u32, String> = std::collections::BTreeMap::new();
    for name in names {
        let Some(number) = leading_number(&name) else {
            continue;
        };
        if number == 0 {
            continue;
        }
        match by_number.entry(number) {
            Entry::Vacant(slot) => {
                slot.insert(name);
            }
            Entry::Occupied(mut slot) => {
                if chapter_file_candidate_wins(slot.get(), &name) {
                    slot.insert(name);
                }
            }
        }
    }
    by_number.into_values().collect()
}

fn rebuild_file_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // 854 号：`[0-9]` 显式 ASCII 数字类（regex 的 `\d` 默认 Unicode，与 JS `\d` 不恒等，
    // 见 [`is_canonical_chapter_file`] 注释）。
    R.get_or_init(|| Regex::new(r"^([0-9]+)[_-]?(.*?)\.md$").unwrap())
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
            series_id: None,
            id: id.to_string(),
            title: "书".to_string(),
            platform: crate::models::book::Platform::Other,
            genre: "other".to_string(),
            status: crate::models::book::BookStatus::Active,
            target_chapters: 10,
            chapter_word_count: 3000,
            target_words: None,
            deadline: None,
            language: Some("zh".to_string()),
            created_at: String::new(),
            updated_at: String::new(),
            parent_book_id: None,
            fanfic_mode: None,
            series: None,
            writing: None,
            governance: None,
        }
    }

    #[tokio::test]
    async fn book_config_roundtrip_and_empty_guard() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        manager
            .save_book_config("b1", &book_config("b1"))
            .await
            .unwrap();
        let loaded = manager.load_book_config("b1").await.unwrap();
        assert_eq!(loaded.title, "书");

        let book_dir = manager.book_dir("b2");
        tokio::fs::create_dir_all(&book_dir).await.unwrap();
        tokio::fs::write(book_dir.join("book.json"), "   ")
            .await
            .unwrap();
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
        assert_eq!(
            manager.resolve_control_document_language("x").await,
            WritingLanguage::En
        );

        let book_dir = manager.book_dir("zh-book");
        tokio::fs::create_dir_all(&book_dir).await.unwrap();
        tokio::fs::write(book_dir.join("book.json"), r#"{"language":"zh"}"#)
            .await
            .unwrap();
        assert_eq!(
            manager.resolve_control_document_language("zh-book").await,
            WritingLanguage::Zh
        );

        tokio::fs::write(book_dir.join("book.json"), r#"{"language":"en"}"#)
            .await
            .unwrap();
        assert_eq!(
            manager.resolve_control_document_language("zh-book").await,
            WritingLanguage::En
        );
    }

    fn write_book_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    /// 目录下的全部文件：相对路径（POSIX 分隔）→ 字节。
    fn tree(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    out.insert(
                        path.strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                        std::fs::read(&path).unwrap(),
                    );
                }
            }
        }
        let mut out = std::collections::BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    /// R58 夹具：三章（三种命名形态）、草稿、快照 0–3、runtime 工件、真相文件、结构化
    /// 状态、sqlite 加速索引；章节索引落盘。返回书目录。
    async fn rollback_fixture(manager: &StateManager) -> PathBuf {
        let book = manager.book_dir("rb");
        for (name, content) in [
            ("0001_风起.md", "c1"),
            ("0002-云涌.md", "c2"),
            ("0003.md", "c3"),
        ] {
            write_book_file(&book.join("chapters").join(name), content);
        }
        write_book_file(
            &book.join("story").join("drafts").join("0002_云涌.md"),
            "d2",
        );
        write_book_file(&book.join("story").join("drafts").join("0003.md"), "d3");
        for n in 0..=3 {
            let snapshot = book.join("story").join("snapshots").join(n.to_string());
            write_book_file(&snapshot.join("current_state.md"), &format!("state-{n}"));
            write_book_file(&snapshot.join("pending_hooks.md"), &format!("hooks-{n}"));
            write_book_file(
                &snapshot.join("chapter_summaries.md"),
                &format!("summaries-{n}"),
            );
            write_book_file(
                &snapshot.join("state").join("manifest.json"),
                &format!("manifest-{n}"),
            );
        }
        for name in [
            "chapter-01.trace.json",
            "chapter-02.trace.json",
            "chapter-03.context.json",
        ] {
            write_book_file(&book.join("story").join("runtime").join(name), name);
        }
        for (name, content) in [
            ("current_state.md", "state-3"),
            ("pending_hooks.md", "hooks-3"),
            ("chapter_summaries.md", "summaries-3"),
            ("particle_ledger.md", "ledger-3"),
        ] {
            write_book_file(&book.join("story").join(name), content);
        }
        write_book_file(
            &book.join("story").join("state").join("manifest.json"),
            "manifest-3",
        );
        write_book_file(
            &book.join("story").join("state").join("extra.json"),
            "extra-3",
        );
        write_book_file(&book.join("story").join("memory.db"), "db");
        write_book_file(&book.join("story").join("memory.db-wal"), "wal");
        let index = manager.load_chapter_index("rb").await.unwrap();
        assert_eq!(index.len(), 3, "{index:?}");
        manager.save_chapter_index("rb", &index).await.unwrap();
        book
    }

    /// 回滚到第 1 章之后仍在原位的文件（目标章及之前的产物、快照 0 与 1）；夹具里其余
    /// 文件都该进回收站批次。
    const STILL_LIVE_AFTER_ROLLBACK_TO_1: [&str; 10] = [
        "chapters/0001_风起.md",
        "story/runtime/chapter-01.trace.json",
        "story/snapshots/0/chapter_summaries.md",
        "story/snapshots/0/current_state.md",
        "story/snapshots/0/pending_hooks.md",
        "story/snapshots/0/state/manifest.json",
        "story/snapshots/1/chapter_summaries.md",
        "story/snapshots/1/current_state.md",
        "story/snapshots/1/pending_hooks.md",
        "story/snapshots/1/state/manifest.json",
    ];

    fn assert_no_trash_batch(book: &Path) {
        let leftover = std::fs::read_dir(book.join("chapters").join(".trash"))
            .map(|entries| entries.flatten().count())
            .unwrap_or(0);
        assert_eq!(leftover, 0, "不该留下回收站批次");
    }

    /// R58/1676 号：回滚不硬删——被废的章、草稿、快照、runtime 工件，连同会被覆盖或
    /// 删除的真相文件、结构化状态、章节索引，整批进
    /// `chapters/.trash/rollback-{ts}-{target}/`，书内相对路径原样、逐字节等于回滚前。
    /// sqlite 加速索引是可重建的派生物，照旧直接删。
    #[tokio::test]
    async fn rollback_moves_discarded_content_into_a_trash_batch() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let book = rollback_fixture(&manager).await;
        let before = tree(&book);

        let mut discarded = manager.rollback_to_chapter("rb", 1).await.unwrap();
        discarded.sort_unstable();
        assert_eq!(discarded, vec![2, 3]);

        let trash = book.join("chapters").join(".trash");
        let batches: Vec<String> = std::fs::read_dir(&trash)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(batches.len(), 1, "{batches:?}");
        assert!(
            batches[0].starts_with("rollback-") && batches[0].ends_with("-1"),
            "{}",
            batches[0]
        );
        let batch = trash.join(&batches[0]);

        let after = tree(&book);
        let trashed = tree(&batch);
        for kept in STILL_LIVE_AFTER_ROLLBACK_TO_1 {
            assert_eq!(after.get(kept), before.get(kept), "{kept} 不该动");
        }
        let mut expected: Vec<&str> = before
            .keys()
            .map(String::as_str)
            .filter(|rel| !STILL_LIVE_AFTER_ROLLBACK_TO_1.contains(rel))
            .collect();
        // 派生物不进回收站。
        expected.retain(|rel| !rel.starts_with("story/memory.db"));
        let missing: Vec<&str> = expected
            .iter()
            .copied()
            .filter(|rel| !trashed.contains_key(*rel))
            .collect();
        let extra: Vec<&str> = trashed
            .keys()
            .map(String::as_str)
            .filter(|rel| !expected.contains(rel))
            .collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "批次缺：{missing:?}；批次多：{extra:?}"
        );
        for rel in expected {
            assert_eq!(
                trashed.get(rel),
                before.get(rel),
                "{rel} 应逐字节等于回滚前"
            );
        }
        // 状态恢复到快照 1；快照 1 没有的可选文件与多余的结构化状态文件不在了。
        assert_eq!(after["story/current_state.md"], b"state-1");
        assert_eq!(after["story/pending_hooks.md"], b"hooks-1");
        assert_eq!(after["story/chapter_summaries.md"], b"summaries-1");
        assert_eq!(after["story/state/manifest.json"], b"manifest-1");
        assert!(!after.contains_key("story/particle_ledger.md"));
        assert!(!after.contains_key("story/state/extra.json"));
        assert!(!after.contains_key("story/memory.db"));
        assert!(!after.contains_key("story/memory.db-wal"));
        let numbers: Vec<u32> = manager
            .load_chapter_index("rb")
            .await
            .unwrap()
            .iter()
            .map(|meta| meta.number)
            .collect();
        assert_eq!(numbers, vec![1]);
    }

    /// R58/1676 号负路径：移的过程中 rename 失败（让 `story/runtime` 不可写）→ 已移项
    /// 全部回位，盘面与操作前逐字节一致，不留批次目录。
    #[cfg(unix)]
    #[tokio::test]
    async fn rollback_failure_moves_everything_back() {
        if std::env::var("USER")
            .map(|user| user == "root")
            .unwrap_or(false)
        {
            eprintln!("root 无视目录权限——跳过");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let book = rollback_fixture(&manager).await;
        let before = tree(&book);
        let runtime = book.join("story").join("runtime");
        std::fs::set_permissions(
            &runtime,
            std::os::unix::fs::PermissionsExt::from_mode(0o555),
        )
        .unwrap();
        let error = manager.rollback_to_chapter("rb", 1).await.unwrap_err();
        std::fs::set_permissions(
            &runtime,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        assert!(
            error.contains("story/runtime") && error.contains("trash batch"),
            "{error}"
        );
        assert_eq!(tree(&book), before, "回位之后盘面应与操作前逐字节一致");
        assert_no_trash_batch(&book);
    }

    /// R58/1676 号负路径：目标章没有快照 → 回滚报错，什么都不动。
    #[tokio::test]
    async fn rollback_without_the_target_snapshot_leaves_the_book_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let book = rollback_fixture(&manager).await;
        std::fs::remove_dir_all(book.join("story").join("snapshots").join("1")).unwrap();
        let before = tree(&book);
        let error = manager.rollback_to_chapter("rb", 1).await.unwrap_err();
        assert!(
            error.starts_with("Cannot restore snapshot for chapter 1"),
            "{error}"
        );
        assert_eq!(tree(&book), before);
        assert_no_trash_batch(&book);
    }

    /// R58/1676 号：删书不硬删——整个书目录进项目级回收站
    /// `{root}/.trash/books/{id}--{ts}/`，回收站里的相对路径与原书目录一致（含
    /// `.versions`、章级 `.trash`、导出产物）。书不存在 → Missing；同名已在 → 追加后缀。
    #[tokio::test]
    async fn trash_book_moves_the_whole_book_directory() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let book = manager.book_dir("b1");
        write_book_file(&book.join("book.json"), "{}");
        write_book_file(&book.join("chapters").join("0001_风起.md"), "c1");
        write_book_file(
            &book.join("chapters").join(".trash").join("0009_old.md"),
            "old",
        );
        write_book_file(&book.join(".versions").join("0001").join("v.md"), "v");
        write_book_file(&book.join("exports").join("b1.epub"), "epub");
        let before = tree(&book);
        let trash = manager.book_trash_dir();
        assert_eq!(trash, dir.path().join(".trash").join("books"));

        assert_eq!(
            manager.trash_book("missing", 1).await.unwrap(),
            TrashBook::Missing
        );
        assert!(!trash.exists(), "没有书可删时不建回收站");

        let trashed = manager.trash_book("b1", 1_795_599_007_123).await.unwrap();
        assert_eq!(
            trashed,
            TrashBook::Trashed(trash.join("b1--20261125093007"))
        );
        assert!(!book.exists());
        assert_eq!(
            tree(&trash.join("b1--20261125093007")),
            before,
            "回收站里的书应与删之前逐字节一致"
        );
        assert!(manager.list_books().await.is_empty());

        // 同一秒内再删一本同 id 的书：追加后缀，不盖上一份。
        write_book_file(&book.join("book.json"), "{}");
        let again = manager.trash_book("b1", 1_795_599_007_123).await.unwrap();
        assert_eq!(
            again,
            TrashBook::Trashed(trash.join("b1--20261125093007-2"))
        );
        assert_eq!(tree(&trash.join("b1--20261125093007")), before);

        // 不是安全路径段的 id 一律拒绝。
        for bad in ["../victim", "..", "a/b"] {
            assert!(manager.trash_book(bad, 1).await.is_err(), "{bad}");
        }
        assert!(!dir.path().join("victim").exists());
    }

    /// 807 号：rollback 删章与索引重建同口径——legacy 连字符旧章（`NNNN-标题.md`）
    /// 必须随目标章之后的章节一起删除，否则被空索引触发的宽容重建捞回=章节复活。
    #[tokio::test]
    async fn rollback_to_chapter_deletes_legacy_hyphen_chapter_files() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let book_dir = manager.book_dir("legacy");
        let chapters = book_dir.join("chapters");
        let snapshot0 = book_dir.join("story").join("snapshots").join("0");
        tokio::fs::create_dir_all(&chapters).await.unwrap();
        tokio::fs::create_dir_all(&snapshot0).await.unwrap();
        tokio::fs::write(chapters.join("0001-镜中醒来.md"), "legacy")
            .await
            .unwrap();
        tokio::fs::write(chapters.join("0002_续章.md"), "canonical")
            .await
            .unwrap();
        tokio::fs::write(chapters.join("0003.md"), "bare")
            .await
            .unwrap();
        tokio::fs::write(snapshot0.join("current_state.md"), "# state")
            .await
            .unwrap();
        tokio::fs::write(snapshot0.join("pending_hooks.md"), "# hooks")
            .await
            .unwrap();
        // 索引缺失 → load 走宽容重建，三种命名全部入索引（读侧口径）。
        let before = manager.load_chapter_index("legacy").await.unwrap();
        let mut numbers: Vec<u32> = before.iter().map(|m| m.number).collect();
        numbers.sort_unstable();
        assert_eq!(numbers, vec![1, 2, 3]);

        let mut discarded = manager.rollback_to_chapter("legacy", 0).await.unwrap();
        discarded.sort_unstable();
        assert_eq!(discarded, vec![1, 2, 3]);
        for name in ["0001-镜中醒来.md", "0002_续章.md", "0003.md"] {
            assert!(!chapters.join(name).exists(), "{name} 应被删除");
        }
        // 文件已清空 → 索引为空且不再被重建捞回。
        assert!(manager
            .load_chapter_index("legacy")
            .await
            .unwrap()
            .is_empty());
    }

    /// 807 号：已落盘计数与重建同口径（按章号去重，legacy 连字符形态同列）。
    #[tokio::test]
    async fn persisted_chapter_count_includes_legacy_hyphen_files_deduped_by_number() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let chapters = manager.book_dir("b").join("chapters");
        tokio::fs::create_dir_all(&chapters).await.unwrap();
        tokio::fs::write(chapters.join("0001-旧.md"), "a")
            .await
            .unwrap();
        tokio::fs::write(chapters.join("0001_新.md"), "b")
            .await
            .unwrap(); // 同章双形态=1
        tokio::fs::write(chapters.join("0002_二.md"), "c")
            .await
            .unwrap();
        tokio::fs::write(chapters.join("index.json"), "[]")
            .await
            .unwrap();
        assert_eq!(manager.get_persisted_chapter_count("b").await.unwrap(), 2);
    }

    /// 813 号：同章双形态去重——正典 `NNNN_` 优先，其次文件名序；章号 0 跳过；按章号升序。
    #[test]
    fn select_chapter_files_prefers_canonical_and_dedupes_by_number() {
        let names = vec![
            "0002-b.md".to_string(),
            "0001_新.md".to_string(),
            "0001-旧.md".to_string(),
            "0003.md".to_string(),
            "0000_zero.md".to_string(),
            "index.json".to_string(),
            "0002-a.md".to_string(),
        ];
        assert_eq!(
            select_chapter_files(names),
            vec![
                "0001_新.md".to_string(),
                "0002-a.md".to_string(),
                "0003.md".to_string()
            ]
        );
    }

    /// 813 号：索引重建对同章双形态只产一行（正典形态的标题胜出）。
    #[tokio::test]
    async fn rebuild_chapter_index_dedupes_same_chapter_files() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let chapters = manager.book_dir("dup").join("chapters");
        tokio::fs::create_dir_all(&chapters).await.unwrap();
        tokio::fs::write(chapters.join("0001-旧题.md"), "legacy")
            .await
            .unwrap();
        tokio::fs::write(chapters.join("0001_新题.md"), "canonical")
            .await
            .unwrap();
        tokio::fs::write(chapters.join("0002.md"), "bare")
            .await
            .unwrap();
        let rows = manager
            .rebuild_chapter_index_from_files_at(&manager.book_dir("dup"))
            .await;
        let numbers: Vec<u32> = rows.iter().map(|m| m.number).collect();
        assert_eq!(numbers, vec![1, 2]);
        assert_eq!(rows[0].title, "新题");
    }

    /// 785 号：get_next_chapter_number 读端点语义探针——legacy 书（章节产物，
    /// 无结构化状态）首读补齐四件且章号权威=durable 链（784 号台账行为面保持）；
    /// 状态齐备后二读零写（manifest mtime 不变 = fsync 原子写不再每次触发，
    /// 784 号台账 9ms 归因的端到端回归锁）。
    #[tokio::test]
    async fn get_next_chapter_number_ensures_legacy_state_then_stops_rewriting() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let book_dir = manager.book_dir("b1");
        tokio::fs::create_dir_all(book_dir.join("chapters"))
            .await
            .unwrap();
        tokio::fs::write(book_dir.join("book.json"), r#"{"language":"zh"}"#)
            .await
            .unwrap();
        tokio::fs::write(book_dir.join("chapters").join("1_开篇.md"), "正文")
            .await
            .unwrap();
        tokio::fs::write(book_dir.join("chapters").join("2_推进.md"), "正文")
            .await
            .unwrap();

        // legacy 书首读：durable 链权威 → 3；ensure 补齐结构化状态。
        assert_eq!(manager.get_next_chapter_number("b1").await.unwrap(), 3);
        let state_dir = book_dir.join("story").join("state");
        for file in [
            "manifest.json",
            "current_state.json",
            "hooks.json",
            "chapter_summaries.json",
        ] {
            assert!(state_dir.join(file).exists(), "ensure 补齐 {file}");
        }
        let manifest = tokio::fs::read_to_string(state_dir.join("manifest.json"))
            .await
            .unwrap();
        assert!(
            manifest.contains("\"lastAppliedChapter\": 2"),
            "manifest 记 durable 进度"
        );

        // 二读：快路径零写——manifest mtime 不变。
        let before = tokio::fs::metadata(state_dir.join("manifest.json"))
            .await
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(manager.get_next_chapter_number("b1").await.unwrap(), 3);
        let after = tokio::fs::metadata(state_dir.join("manifest.json"))
            .await
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after, "快路径二读零写（mtime 不变）");
    }

    #[tokio::test]
    async fn ensure_control_documents_creates_defaults_and_methodology() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let book_dir = manager.book_dir("b1");
        tokio::fs::create_dir_all(&book_dir).await.unwrap();
        tokio::fs::write(book_dir.join("book.json"), r#"{"language":"zh"}"#)
            .await
            .unwrap();

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
        let intent_again =
            tokio::fs::read_to_string(book_dir.join("story").join("author_intent.md"))
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
        tokio::fs::write(chapters.join("0001_初醒.md"), "# 第1章 初醒\n正文")
            .await
            .unwrap();
        tokio::fs::write(chapters.join("0002_风起.md"), "# 第2章 风起\n正文")
            .await
            .unwrap();
        // 断链：0004 存在 0003 缺失 → durable 到 2。
        tokio::fs::write(chapters.join("0004_断链.md"), "# 第4章\n正文")
            .await
            .unwrap();
        assert_eq!(manager.get_next_chapter_number("b1").await.unwrap(), 3);
        assert_eq!(manager.get_persisted_chapter_count("b1").await.unwrap(), 3);
    }

    #[tokio::test]
    async fn chapter_index_rebuild_and_empty_guard() {
        let dir = tempfile::tempdir().unwrap();
        let manager = StateManager::new(dir.path());
        let chapters = manager.book_dir("b1").join("chapters");
        tokio::fs::create_dir_all(&chapters).await.unwrap();
        tokio::fs::write(chapters.join("0001_初醒.md"), "# 第1章\n正 文")
            .await
            .unwrap();

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
        let saved: Vec<ChapterMeta> = serde_json::from_str(
            &tokio::fs::read_to_string(chapters.join("index.json"))
                .await
                .unwrap(),
        )
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
        tokio::fs::create_dir_all(story.join("state"))
            .await
            .unwrap();
        tokio::fs::write(story.join("current_state.md"), "状态")
            .await
            .unwrap();
        tokio::fs::write(story.join("state").join("hooks.json"), "{}")
            .await
            .unwrap();
        // 不存在的文件静默跳过。
        manager.snapshot_state("b1", 2).await.unwrap();
        let snapshot = story.join("snapshots").join("2");
        assert_eq!(
            tokio::fs::read_to_string(snapshot.join("current_state.md"))
                .await
                .unwrap(),
            "状态"
        );
        assert_eq!(
            tokio::fs::read_to_string(snapshot.join("state").join("hooks.json"))
                .await
                .unwrap(),
            "{}"
        );
        assert!(!snapshot.join("pending_hooks.md").exists());
    }
}
