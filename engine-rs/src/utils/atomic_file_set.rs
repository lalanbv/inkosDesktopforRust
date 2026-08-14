//! 原子文件集提交（transactional file set commit）。
//!
//! 移植自 `packages/core/src/utils/atomic-file-set.ts`（116 行）。写多删多的事务：
//! rootDir 下建临时事务目录 → staged 暂存全部写入 → 存量目标改名 backup →
//! staged 改名就位 → 任一步失败回滚（删除已就位目标 + 恢复 backup）。
//!
//! writer 的 `saveChapter` 用它保证章节 + 真相文件要么全部落盘、要么全部不动。
//!
//! ## 与 TS 的差异
//! TS 的 `renameFile` 注入参数是测试缝隙（生产恒用 `rename`），Rust 版不保留；
//! 回滚语义（逆序删除已就位 → 逆序恢复 backup → 汇总回滚错误）逐字对齐。

use std::io;
use std::path::{Path, PathBuf};

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
    match content {
        FileContent::Text(text) => tokio::fs::write(path, text).await,
        FileContent::Bytes(bytes) => tokio::fs::write(path, bytes).await,
    }
}

/// 原子提交文件集。对齐 TS `commitAtomicFileSet` 的三段式事务（staged →
/// backup → 就位）与失败回滚。
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

    Ok(())
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
}
