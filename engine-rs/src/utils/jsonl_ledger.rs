//! 1666 号：jsonl 账本「读旧 + 拼接 + 原子写」追加——usage 账本与 prompt
//! 版本账本共用（TS 镜像 = packages/core/src/utils/append-queue.ts + 两处
//! store 的 `runSerially`/读错误区分）。
//!
//! 对 writing-log 惯例的两点加固：
//!
//! 1. **进程内串行化**：原子写只保证「单次写要么旧要么新」，不保证「读—改—
//!    写」整体。两个并发追加各自读到 N 行、各写 N+1 行 = 丢行，append-only
//!    语义被破（实测 32 路并发追加后只剩 2 行）。同一文件路径的追加经互斥锁
//!    排队；不同路径互不阻塞。
//! 2. **读错误 ≠ 空账本**：账本读不出来（权限/IO）时**中止追加**并返回 Err。
//!    此前任何读错误都被当成空账本，随后原子写用「只有新行」的内容覆盖掉
//!    整本历史——一次读失败毁一本账。只有 NotFound（首次追加）才是空账本。
//!
//! 跨进程（两条引擎腿同时写同一账本）不在此列。

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

type PathLock = Arc<tokio::sync::Mutex<()>>;

/// 路径 → 追加互斥锁。条目数以「进程打开过的账本路径数」为界（服务进程 =
/// 项目根数），不随追加次数增长。
fn path_locks() -> &'static std::sync::Mutex<HashMap<PathBuf, PathLock>> {
    static LOCKS: OnceLock<std::sync::Mutex<HashMap<PathBuf, PathLock>>> = OnceLock::new();
    LOCKS.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// 读账本文本：NotFound → 空串；其余读错误原样上抛（调用方须中止追加）。
async fn read_ledger_text(path: &Path) -> io::Result<String> {
    match tokio::fs::read_to_string(path).await {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

/// 追加：`existing_lines` 把既有文本归一为行（坏行由调用方按各自类型丢弃），
/// `new_lines` 接在其后，整体原子写回。同一路径的追加串行执行。
pub async fn append_jsonl(
    path: &Path,
    existing_lines: impl FnOnce(&str) -> Vec<String>,
    new_lines: Vec<String>,
) -> io::Result<()> {
    let lock = {
        let mut locks = path_locks().lock().expect("jsonl ledger lock map poisoned");
        locks
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _guard = lock.lock().await;
    let text = read_ledger_text(path).await?;
    let mut lines = existing_lines(&text);
    lines.extend(new_lines);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    crate::utils::atomic_file_set::write_file_atomic(path, &(lines.join("\n") + "\n")).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_appends_keep_every_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let mut handles = Vec::new();
        for index in 0..32u64 {
            let path = path.clone();
            handles.push(tokio::spawn(async move {
                // 既有行原样保留（各账本的类型归一在这一步做；此处恒等）。
                append_jsonl(
                    &path,
                    |text| {
                        text.lines()
                            .filter(|line| !line.is_empty())
                            .map(str::to_string)
                            .collect()
                    },
                    vec![format!("line-{index}")],
                )
                .await
            }));
        }
        for handle in handles {
            handle.await.unwrap().unwrap();
        }
        let text = tokio::fs::read_to_string(&path).await.unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 32, "并发追加丢行");
    }

    #[tokio::test]
    async fn existing_lines_are_normalized_and_kept_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("ledger.jsonl");
        append_jsonl(&path, |_| Vec::new(), vec!["a".to_string()])
            .await
            .unwrap();
        append_jsonl(
            &path,
            |text| {
                text.lines()
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect()
            },
            vec!["b".to_string(), "c".to_string()],
        )
        .await
        .unwrap();
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "a\nb\nc\n");
    }

    /// chmod 000 后账本读不出来：追加须中止（PermissionDenied），既有内容不被
    /// 「只有新行」的内容覆盖。
    #[cfg(unix)]
    #[tokio::test]
    async fn unreadable_ledger_aborts_append_and_keeps_content() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        tokio::fs::write(&path, "keep\n").await.unwrap();
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();
        // 前置自检（不是被测行为）：本环境仍读得到 = 无视权限位（root），
        // 用例不成立，还原后跳过。
        let environment_honors_permissions = tokio::fs::read_to_string(&path).await.is_err();
        let appended = append_jsonl(&path, |_| Vec::new(), vec!["new".to_string()]).await;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .await
            .unwrap();
        if !environment_honors_permissions {
            eprintln!("本环境 chmod 000 不生效（root？）——跳过");
            return;
        }
        assert!(
            matches!(appended, Err(ref error) if error.kind() == io::ErrorKind::PermissionDenied),
            "读不出来必须中止追加：{appended:?}"
        );
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "keep\n");
    }
}
