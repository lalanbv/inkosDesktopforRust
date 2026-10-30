//! 原子文件集提交（transactional file set commit）。
//!
//! 移植自 `packages/core/src/utils/atomic-file-set.ts`（116 行）。写多删多的事务：
//! rootDir 下建临时事务目录 → staged 暂存全部写入 → 存量目标改名 backup →
//! staged 改名就位 → 任一步失败回滚（删除已就位目标 + 恢复 backup）。
//!
//! writer 的 `saveChapter` 用它保证章节 + 真相文件要么全部落盘、要么全部不动。
//!
//! ## 持久性（760 号）
//! 写入统一 `sync_all` 落盘后再 `rename`，成功后尽力 fsync 受影响父目录：
//! 此前保证只覆盖进程崩溃（rename 对观察者原子），掉电/内核崩溃可重放出
//! 零长/半截目标；数据先 sync 后 rename 后，两类崩溃都收敛到「目标要么
//! 旧内容、要么完整新内容」。调用面实测（46 站点）全部为用户操作/章节
//! 步进级频率，无 per-token 热循环，fsync 成本可忽略。
//!
//! ## 与 TS 的差异
//! TS 的 `renameFile` 注入参数是测试缝隙（生产恒用 `rename`），Rust 版不保留；
//! 回滚语义（逆序删除已就位 → 逆序恢复 backup → 汇总回滚错误）逐字对齐。

use std::io;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt as _;

/// 单条写入。对齐 TS `AtomicFileWrite`（content: string | Uint8Array）。
#[derive(Debug, Clone)]
pub struct AtomicFileWrite {
    pub relative_path: String,
    pub content: FileContent,
}

#[derive(Debug, Clone)]
pub enum FileContent {
    Text(String),
    Bytes(Vec<u8>),
}

/// 提交入参。对齐 TS `AtomicFileSet`。
pub struct AtomicFileSet<'a> {
    pub root_dir: &'a Path,
    pub writes: Vec<AtomicFileWrite>,
    pub deletes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AtomicFileSetError {
    #[error("Atomic file path must stay inside rootDir: {0}")]
    UnsafePath(String),
    #[error("Atomic file set contains duplicate write paths")]
    DuplicateWritePaths,
    #[error("Atomic file set cannot write and delete the same path")]
    WriteDeleteConflict,
    #[error("Atomic file commit failed and rollback was incomplete: {source}; rollback errors: {rollback:#?}")]
    IncompleteRollback {
        source: io::Error,
        rollback: Vec<io::Error>,
    },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// 词法规范化相对路径（`a/./b` → `a/b`，`a/../b` → `b`）。对齐 Node `normalize`
/// 的路径语义（文件路径，无尾随斜杠保留）。
fn normalize_relative(relative: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.last().map(|last| *last != "..").unwrap_or(false) {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// 安全校验 + 规范化。对齐 TS `safeRelativePath`：空 / 绝对路径 / `..` 逃逸 → 拒绝。
fn safe_relative_path(relative_path: &str) -> Result<String, AtomicFileSetError> {
    let normalized = normalize_relative(relative_path);
    if relative_path.trim().is_empty()
        || Path::new(relative_path).is_absolute()
        || normalized == ".."
        || normalized.starts_with("../")
    {
        return Err(AtomicFileSetError::UnsafePath(
            relative_path.to_string(),
        ));
    }
    Ok(normalized)
}

/// exists 判定（仅 NotFound 视为不存在，其他错误上抛）。对齐 TS `exists`。
async fn path_exists(path: &Path) -> io::Result<bool> {
    match tokio::fs::metadata(path).await {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// rm -r -f 等价（文件或目录均可，不存在吞错）。对齐 TS `rm(p, { recursive: true, force: true })`。
async fn rm_any(path: &Path) -> io::Result<()> {
    match tokio::fs::metadata(path).await {
        Ok(meta) if meta.is_dir() => {
            tokio::fs::remove_dir_all(path).await
        }
        Ok(_) => tokio::fs::remove_file(path).await,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

async fn write_content(path: &Path, content: &FileContent) -> io::Result<()> {
    let mut file = tokio::fs::File::create(path).await?;
    match content {
        FileContent::Text(text) => file.write_all(text.as_bytes()).await?,
        FileContent::Bytes(bytes) => file.write_all(bytes).await?,
    }
    file.sync_all().await
}

/// 原子提交文件集。对齐 TS `commitAtomicFileSet` 的三段式事务（staged →
/// backup → 就位）与失败回滚。
///
/// 760 号：staged 文件 `sync_all` 落盘后才进入 rename 段，全部就位后尽力
/// fsync 各触碰目标的父目录（rename 项本身落盘，防掉电重放丢 rename）——
/// 章节内容这类不可再生的用户数据，掉电后同样要么全旧、要么全新。
pub async fn commit_atomic_file_set(input: &AtomicFileSet<'_>) -> Result<(), AtomicFileSetError> {
    let mut writes: Vec<(String, &FileContent)> = Vec::with_capacity(input.writes.len());
    for entry in &input.writes {
        writes.push((safe_relative_path(&entry.relative_path)?, &entry.content));
    }
    let mut deletes: Vec<String> = Vec::with_capacity(input.deletes.len());
    for relative_path in &input.deletes {
        deletes.push(safe_relative_path(relative_path)?);
    }

    let mut write_paths: std::collections::HashSet<&String> = std::collections::HashSet::new();
    for (path, _) in &writes {
        if !write_paths.insert(path) {
            return Err(AtomicFileSetError::DuplicateWritePaths);
        }
    }
    if deletes.iter().any(|path| write_paths.contains(path)) {
        return Err(AtomicFileSetError::WriteDeleteConflict);
    }

    tokio::fs::create_dir_all(input.root_dir).await?;
    let transaction_dir = tempfile::Builder::new()
        .prefix(".inkos-file-txn-")
        .tempdir_in(input.root_dir)?;
    let staged_dir = transaction_dir.path().join("staged");
    let backup_dir = transaction_dir.path().join("backup");

    let mut touched_paths: Vec<&String> = write_paths.iter().copied().collect();
    touched_paths.extend(deletes.iter());
    let mut backups: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut committed_targets: Vec<PathBuf> = Vec::new();

    let result: io::Result<()> = async {
        for (relative_path, content) in &writes {
            let staged_path = staged_dir.join(relative_path);
            if let Some(parent) = staged_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            write_content(&staged_path, content).await?;
        }

        for relative_path in &touched_paths {
            let target = input.root_dir.join(relative_path);
            if let Some(parent) = target.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            if !path_exists(&target).await? {
                continue;
            }

            let backup = backup_dir.join(relative_path);
            if let Some(parent) = backup.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::rename(&target, &backup).await?;
            backups.push((target, backup));
        }

        for (relative_path, _) in &writes {
            let target = input.root_dir.join(relative_path);
            tokio::fs::rename(staged_dir.join(relative_path), &target).await?;
            committed_targets.push(target);
        }
        Ok(())
    }
    .await;

    if let Err(error) = result {
        let mut rollback_errors: Vec<io::Error> = Vec::new();
        for target in committed_targets.iter().rev() {
            if let Err(rollback_error) = rm_any(target).await {
                rollback_errors.push(rollback_error);
            }
        }
        for (target, backup) in backups.iter().rev() {
            let restore = async {
                rm_any(target).await?;
                if let Some(parent) = target.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::rename(backup, target).await
            }
            .await;
            if let Err(rollback_error) = restore {
                rollback_errors.push(rollback_error);
            }
        }

        if !rollback_errors.is_empty() {
            return Err(AtomicFileSetError::IncompleteRollback {
                source: error,
                rollback: rollback_errors,
            });
        }
        return Err(error.into());
    }

    // 760 号：rename 项落盘（尽力）——写入与删除的父目录各 sync 一次
    //（去重），防掉电重放把「target→backup 已持久、staged→target 丢失」
    // 重放成目标缺失。
    let mut synced_dirs: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for relative_path in &touched_paths {
        let target = input.root_dir.join(relative_path);
        if let Some(parent) = target.parent() {
            if synced_dirs.insert(parent.to_path_buf()) {
                sync_parent_dir_best_effort(&target).await;
            }
        }
    }

    Ok(())
}

/// rename 失败后的最大重试次数（首次尝试 + 3 = 至多 4 次）。
/// 对齐 TS `ATOMIC_WRITE_MAX_RETRIES`（R29/404 号）。
pub const ATOMIC_WRITE_MAX_RETRIES: u32 = 3;
/// 重试基础退避（线性：attempt × delayMs，毫秒）。
pub const ATOMIC_WRITE_RETRY_DELAY_MS: u64 = 50;

/// 重试决策（纯函数，golden 锁；`error_kind` 由 [`atomic_write_error_kind`]
/// 归一）：错误码在重试集合内且重试预算未用尽 → 重试。`attempt` 从 0 计。
pub fn atomic_write_retry_decision(
    error_kind: Option<&str>,
    attempt: u32,
    max_retries: u32,
) -> bool {
    let Some(kind) = error_kind else {
        return false;
    };
    if !matches!(kind, "EPERM" | "EBUSY") {
        return false;
    }
    attempt < max_retries
}

/// `std::io::Error` → 重试决策用的错误码（文件锁语义；无码 → None）。
/// pub 供差分测试（tests/golden_atomic_write_diff.rs）锁定码值映射。
pub fn atomic_write_error_kind(error: &std::io::Error) -> Option<&'static str> {
    match error.raw_os_error() {
        // EPERM（unix=1，Windows 经 PermissionDenied 呈现）。
        Some(1) => Some("EPERM"),
        // EBUSY（unix=16；Windows ERROR_SHARING_VIOLATION/LOCK_VIOLATION
        // 在 Rust 里也常映射为 EBUSY 语义）。
        Some(16) => Some("EBUSY"),
        _ => {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                Some("EPERM")
            } else {
                None
            }
        }
    }
}

/// 临时文件写入 + `sync_all` 落盘（760 号持久性升级）：数据在 `rename` 前
/// 已到稳定存储，掉电/内核崩溃后 rename 重放不会产出零长/半截目标。
async fn write_tmp_durable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = tokio::fs::File::create(path).await?;
    file.write_all(bytes).await?;
    file.sync_all().await
}

/// [`write_tmp_durable`] 的同步形态。
fn write_tmp_durable_sync(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// rename 成功后尽力 fsync 父目录（unix；Windows 无目录文件句柄语义，跳过）：
/// 让 rename 项本身也落盘，否则掉电重放可能「target→backup 已持久而
/// staged→target 丢失 = 目标缺失」。仅尽力——失败不回滚已成功的写入
/// （数据已 sync，最坏回退到 199 号的进程崩溃保证）。
#[cfg(unix)]
async fn sync_parent_dir_best_effort(path: &Path) {
    if let Some(dir) = path.parent() {
        if let Ok(handle) = tokio::fs::File::open(dir).await {
            let _ = handle.sync_all().await;
        }
    }
}

#[cfg(not(unix))]
async fn sync_parent_dir_best_effort(_path: &Path) {}

/// [`sync_parent_dir_best_effort`] 的同步形态。pub(crate) 供 llm/secrets 的
/// 0600 专用写路径复用（762 号）。
#[cfg(unix)]
pub(crate) fn sync_parent_dir_best_effort_sync(path: &Path) {
    if let Some(dir) = path.parent() {
        if let Ok(handle) = std::fs::File::open(dir) {
            let _ = handle.sync_all();
        }
    }
}

#[cfg(not(unix))]
pub(crate) fn sync_parent_dir_best_effort_sync(_path: &Path) {}

/// 单文件原子替换（199 号稳定性审计）：先写同目录唯一临时文件，再
/// `rename` 就位（同文件系统内 rename 对观察者原子）。进程在写入中途崩溃
/// 只会留下孤儿临时文件，目标文件要么是旧内容、要么是完整新内容——
/// 不再有「截断的 book.json 让整本书不可加载」的失败形态。
///
/// R29/404 号：rename 目标被占用（Windows 文件锁 EPERM/EBUSY）时线性退避
/// 重试（WNW v6.2.1 的 atomic_write_json 思路），放弃时清理临时文件。
///
/// 760 号：临时文件 `sync_all` 落盘后才 rename，成功后尽力 fsync 父目录——
/// 掉电/内核崩溃同样收敛到「要么旧、要么完整新」。
///
/// 适用于配置/索引类小文件（book.json / index.json / timeline.json）；
/// 多文件事务仍走 [`commit_atomic_file_set`]。
pub async fn write_file_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    write_file_atomic_bytes(path, content.as_bytes()).await
}

/// [`write_file_atomic`] 的字节形态（779 号）：图片/上传缓冲/导出包等
/// 非 UTF-8 内容的原子写。命名、持久性与重试语义与文本形态完全一致。
pub async fn write_file_atomic_bytes(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let tmp = atomic_tmp_path(path);
    write_tmp_durable(&tmp, content).await?;
    let mut attempt: u32 = 0;
    loop {
        match tokio::fs::rename(&tmp, path).await {
            Ok(()) => {
                sync_parent_dir_best_effort(path).await;
                return Ok(());
            }
            Err(error) => {
                let kind = atomic_write_error_kind(&error);
                if !atomic_write_retry_decision(kind, attempt, ATOMIC_WRITE_MAX_RETRIES) {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err(error);
                }
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_millis(
                    ATOMIC_WRITE_RETRY_DELAY_MS * u64::from(attempt),
                ))
                .await;
            }
        }
    }
}

/// [`write_file_atomic`] 的同步形态：供无法 `.await` 的同步事务闭包使用
/// （play_graph File 后端的 `with_transaction`/`flush` 在 rusqlite 同款同步
/// 接口内持久化）。临时文件命名、重试决策与清理语义与 async 版共用同一实现，
/// 仅退避改为 `std::thread::sleep`。760 号持久性语义与 async 版一致。
pub fn write_file_atomic_sync(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let tmp = atomic_tmp_path(path);
    write_tmp_durable_sync(&tmp, content)?;
    let mut attempt: u32 = 0;
    loop {
        match std::fs::rename(&tmp, path) {
            Ok(()) => {
                sync_parent_dir_best_effort_sync(path);
                return Ok(());
            }
            Err(error) => {
                let kind = atomic_write_error_kind(&error);
                if !atomic_write_retry_decision(kind, attempt, ATOMIC_WRITE_MAX_RETRIES) {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(error);
                }
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(
                    ATOMIC_WRITE_RETRY_DELAY_MS * u64::from(attempt),
                ));
            }
        }
    }
}

/// 同目录唯一临时文件路径 `.{name}.tmp-{pid}-{seq}`。async/sync 两形态共用
/// 一个进程级序号：并发写同一目标时各自的临时文件不会互相覆盖；同目录
/// 保证 rename 不跨文件系统。
fn atomic_tmp_path(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{}.tmp-{}-{}", file_name, std::process::id(), seq))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn read(path: &Path) -> String {
        tokio::fs::read_to_string(path).await.expect("读取")
    }

    #[tokio::test]
    async fn commit_writes_creates_files_and_dirs() {
        let dir = tempfile::tempdir().expect("临时目录");
        let set = AtomicFileSet {
            root_dir: dir.path(),
            writes: vec![
                AtomicFileWrite {
                    relative_path: "chapters/0001_第一章.md".to_string(),
                    content: FileContent::Text("# 第1章".to_string()),
                },
                AtomicFileWrite {
                    relative_path: "story/current_state.md".to_string(),
                    content: FileContent::Text("状态".to_string()),
                },
            ],
            deletes: vec![],
        };
        commit_atomic_file_set(&set).await.expect("提交");
        assert_eq!(read(&dir.path().join("chapters/0001_第一章.md")).await, "# 第1章");
        assert_eq!(read(&dir.path().join("story/current_state.md")).await, "状态");
    }

    #[tokio::test]
    async fn commit_deletes_superseded_files() {
        let dir = tempfile::tempdir().expect("临时目录");
        tokio::fs::create_dir_all(dir.path().join("chapters"))
            .await
            .expect("建目录");
        tokio::fs::write(dir.path().join("chapters/0001_旧标题.md"), "旧")
            .await
            .expect("写旧章");
        let set = AtomicFileSet {
            root_dir: dir.path(),
            writes: vec![AtomicFileWrite {
                relative_path: "chapters/0001_新标题.md".to_string(),
                content: FileContent::Text("新".to_string()),
            }],
            deletes: vec!["chapters/0001_旧标题.md".to_string()],
        };
        commit_atomic_file_set(&set).await.expect("提交");
        assert!(!dir.path().join("chapters/0001_旧标题.md").exists());
        assert!(dir.path().join("chapters/0001_新标题.md").exists());
    }

    #[tokio::test]
    async fn commit_overwrites_existing_file() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("story/current_state.md");
        tokio::fs::create_dir_all(target.parent().unwrap())
            .await
            .expect("建目录");
        tokio::fs::write(&target, "旧状态").await.expect("写旧状态");
        let set = AtomicFileSet {
            root_dir: dir.path(),
            writes: vec![AtomicFileWrite {
                relative_path: "story/current_state.md".to_string(),
                content: FileContent::Text("新状态".to_string()),
            }],
            deletes: vec![],
        };
        commit_atomic_file_set(&set).await.expect("提交");
        assert_eq!(read(&target).await, "新状态");
    }

    #[tokio::test]
    async fn commit_rolls_back_on_failure() {
        let dir = tempfile::tempdir().expect("临时目录");
        // rootDir/x 是文件，写 x/y.md 时 mkdir(x) 失败 → 触发回滚。
        tokio::fs::write(dir.path().join("x"), "占位文件").await.expect("写占位");
        let preserved = dir.path().join("story/current_state.md");
        tokio::fs::create_dir_all(preserved.parent().unwrap())
            .await
            .expect("建目录");
        tokio::fs::write(&preserved, "原有内容").await.expect("写原状态");

        let set = AtomicFileSet {
            root_dir: dir.path(),
            writes: vec![
                AtomicFileWrite {
                    relative_path: "story/current_state.md".to_string(),
                    content: FileContent::Text("新内容".to_string()),
                },
                AtomicFileWrite {
                    relative_path: "x/y.md".to_string(),
                    content: FileContent::Text("非法".to_string()),
                },
            ],
            deletes: vec![],
        };
        let err = commit_atomic_file_set(&set).await.expect_err("应失败");
        assert!(matches!(err, AtomicFileSetError::Io(_)), "got: {err:?}");
        // 回滚后原文件内容保持不变。
        assert_eq!(read(&preserved).await, "原有内容");
        // 事务目录被清理。
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("列目录")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".inkos-file-txn-"))
            .collect();
        assert!(leftovers.is_empty(), "leftover txns: {leftovers:?}");
    }

    #[tokio::test]
    async fn rejects_unsafe_paths() {
        let dir = tempfile::tempdir().expect("临时目录");
        for bad in ["../escape.md", "/etc/passwd", "  ", "a/../../escape.md"] {
            let set = AtomicFileSet {
                root_dir: dir.path(),
                writes: vec![AtomicFileWrite {
                    relative_path: bad.to_string(),
                    content: FileContent::Text("x".to_string()),
                }],
                deletes: vec![],
            };
            assert!(
                matches!(
                    commit_atomic_file_set(&set).await,
                    Err(AtomicFileSetError::UnsafePath(_))
                ),
                "应拒绝: {bad}"
            );
        }
    }

    #[tokio::test]
    async fn rejects_duplicate_and_conflict_paths() {
        let dir = tempfile::tempdir().expect("临时目录");
        let set = AtomicFileSet {
            root_dir: dir.path(),
            writes: vec![
                AtomicFileWrite {
                    relative_path: "a.md".to_string(),
                    content: FileContent::Text("x".to_string()),
                },
                AtomicFileWrite {
                    relative_path: "a.md".to_string(),
                    content: FileContent::Text("y".to_string()),
                },
            ],
            deletes: vec![],
        };
        assert!(matches!(
            commit_atomic_file_set(&set).await,
            Err(AtomicFileSetError::DuplicateWritePaths)
        ));

        let set = AtomicFileSet {
            root_dir: dir.path(),
            writes: vec![AtomicFileWrite {
                relative_path: "a.md".to_string(),
                content: FileContent::Text("x".to_string()),
            }],
            deletes: vec!["a.md".to_string()],
        };
        assert!(matches!(
            commit_atomic_file_set(&set).await,
            Err(AtomicFileSetError::WriteDeleteConflict)
        ));
    }

    #[tokio::test]
    async fn bytes_content_supported() {
        let dir = tempfile::tempdir().expect("临时目录");
        let set = AtomicFileSet {
            root_dir: dir.path(),
            writes: vec![AtomicFileWrite {
                relative_path: "bin.dat".to_string(),
                content: FileContent::Bytes(vec![1, 2, 3, 255]),
            }],
            deletes: vec![],
        };
        commit_atomic_file_set(&set).await.expect("提交");
        let raw = tokio::fs::read(dir.path().join("bin.dat")).await.expect("读取");
        assert_eq!(raw, vec![1, 2, 3, 255]);
    }

    #[test]
    fn normalize_collapses_dots() {
        assert_eq!(normalize_relative("a/./b.md"), "a/b.md");
        assert_eq!(normalize_relative("a/../b.md"), "b.md");
        assert_eq!(normalize_relative("./a.md"), "a.md");
    }
    #[tokio::test]
    async fn write_file_atomic_replaces_target_with_full_content() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("book.json");
        tokio::fs::write(&target, "旧内容").await.unwrap();
        write_file_atomic(&target, "{ \"完整新内容\" }").await.unwrap();
        let content = tokio::fs::read_to_string(&target).await.unwrap();
        assert_eq!(content, "{ \"完整新内容\" }");
        // 目录里不留孤儿临时文件。
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[tokio::test]
    async fn write_file_atomic_creates_new_file_and_cleans_up_on_rename_failure() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("nested").join("book.json");
        // 父目录缺失：写临时文件即失败，不留残留。
        assert!(write_file_atomic(&target, "x").await.is_err());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(leftovers.is_empty(), "失败路径不应留下临时文件");
    }

    #[test]
    fn write_file_atomic_sync_replaces_target_with_full_content() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("graph.json");
        std::fs::write(&target, "旧内容").unwrap();
        write_file_atomic_sync(&target, "{ \"完整新内容\" }".as_bytes()).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "{ \"完整新内容\" }");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn write_file_atomic_sync_cleans_up_when_parent_missing() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("nested").join("graph.json");
        assert!(write_file_atomic_sync(&target, b"x").is_err());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(leftovers.is_empty(), "失败路径不应留下临时文件");
    }

    // 760 号：durable 路径（File::create + write_all + sync_all）的零长边界。
    #[tokio::test]
    async fn write_file_atomic_accepts_empty_content() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("empty.json");
        write_file_atomic(&target, "").await.unwrap();
        assert_eq!(tokio::fs::read_to_string(&target).await.unwrap(), "");
    }

    // 779 号：字节形态与文本形态同语义（非 UTF-8 内容原样落盘）。
    #[tokio::test]
    async fn write_file_atomic_bytes_replaces_target_with_full_content() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("cover.png");
        tokio::fs::write(&target, [0u8, 1, 2]).await.unwrap();
        let payload: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0xFF, 0x00];
        write_file_atomic_bytes(&target, &payload).await.unwrap();
        let raw = tokio::fs::read(&target).await.unwrap();
        assert_eq!(raw, payload);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[tokio::test]
    async fn write_file_atomic_bytes_cleans_up_when_parent_missing() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("nested").join("cover.png");
        assert!(write_file_atomic_bytes(&target, b"x").await.is_err());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(leftovers.is_empty(), "失败路径不应留下临时文件");
    }

    #[tokio::test]
    async fn write_file_atomic_bytes_accepts_empty_content() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("empty.bin");
        write_file_atomic_bytes(&target, b"").await.unwrap();
        assert!(tokio::fs::read(&target).await.unwrap().is_empty());
    }

    #[test]
    fn write_file_atomic_sync_accepts_empty_content() {
        let dir = tempfile::tempdir().expect("临时目录");
        let target = dir.path().join("empty.bin");
        write_file_atomic_sync(&target, b"").unwrap();
        assert!(std::fs::read(&target).unwrap().is_empty());
    }

    #[test]
    fn atomic_tmp_path_stays_in_target_dir_and_is_unique_per_call() {
        let target = Path::new("/some/dir/state.json");
        let first = atomic_tmp_path(target);
        let second = atomic_tmp_path(target);
        assert_eq!(first.parent(), Some(Path::new("/some/dir")));
        assert!(first.file_name().unwrap().to_string_lossy().starts_with(".state.json.tmp-"));
        assert_ne!(first, second, "同目标连续两次取临时路径必须不同");
    }
}
