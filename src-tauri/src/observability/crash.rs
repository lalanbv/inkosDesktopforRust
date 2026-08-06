use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::panic;
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 保留的最近 crash dump 数量（超出按修改时间删最旧的）。
const MAX_CRASH_DUMPS: usize = 10;

/// Crash dump 结构（JSON 序列化）
#[derive(Debug, Serialize, Deserialize)]
pub struct CrashDump {
    pub timestamp: u64,
    pub thread: String,
    pub payload: String,
    pub backtrace: String,
}

/// 写一个 crash dump 并做 LRU 清理。
///
/// 从 panic hook 里拆出来是为了可测：hook 本身是进程级全局状态（`set_hook` 每次
/// 调用都覆盖上一个），测试并行跑时会互相抢；而这个函数是纯 I/O，可以直接对着
/// 临时目录断言，不碰全局状态。
fn write_crash_dump(crash_dir: &Path, dump: &CrashDump) {
    use std::fs;

    // 文件名用纳秒而非秒：同一秒内的多次 panic（多线程崩溃、崩溃风暴）会算出
    // 相同的秒级时间戳，两个线程并发写同一路径会把 JSON 写成交错的半截内容。
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let dump_path = crash_dir.join(format!("crash-{nanos}.json"));
    if let Ok(json) = serde_json::to_string_pretty(dump) {
        let _ = fs::write(&dump_path, json);
    }

    // LRU 清理（保留最近 MAX_CRASH_DUMPS 个）
    if let Ok(entries) = fs::read_dir(crash_dir) {
        let mut files: Vec<_> = entries
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("crash-") && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect();

        // 按修改时间排序（最新在前）
        files.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        });
        files.reverse();

        for old_file in files.iter().skip(MAX_CRASH_DUMPS) {
            let _ = fs::remove_file(old_file.path());
        }
    }
}

/// 从 `PanicHookInfo` 里提取崩溃现场信息。
fn collect_dump(panic_info: &panic::PanicHookInfo<'_>) -> CrashDump {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let thread = std::thread::current()
        .name()
        .unwrap_or("<unnamed>")
        .to_string();

    // panic! 的 payload 可能是 &str（字面量）或 String（格式化）——两种都要取到。
    let payload = panic_info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic_info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "Box<Any>".to_string());

    let backtrace = std::backtrace::Backtrace::force_capture().to_string();

    CrashDump {
        timestamp,
        thread,
        payload,
        backtrace,
    }
}

/// 初始化全局 panic hook，捕获崩溃并写 JSON dump 到 `crash_dir`。
///
/// # 行为
/// - 每次 panic 生成一个 `crash-<unix_nanos>.json`
/// - 保留最近 10 个 dump（按修改时间删最旧的）
/// - 写完 dump 后调用默认 panic handler（保持终端输出）
pub fn init_panic_hook(crash_dir: PathBuf) -> Result<()> {
    use anyhow::Context;
    use std::fs;

    fs::create_dir_all(&crash_dir)
        .with_context(|| format!("创建 crash 目录失败: {:?}", crash_dir))?;

    let default_panic = panic::take_hook();

    panic::set_hook(Box::new(move |panic_info| {
        write_crash_dump(&crash_dir, &collect_dump(panic_info));
        default_panic(panic_info);
    }));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn sample_dump(payload: &str) -> CrashDump {
        CrashDump {
            timestamp: 1_700_000_000,
            thread: "test-thread".to_string(),
            payload: payload.to_string(),
            backtrace: "<backtrace>".to_string(),
        }
    }

    fn dump_files(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("crash-") && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect()
    }

    #[test]
    fn test_write_crash_dump_produces_readable_json() {
        let temp = TempDir::new().unwrap();

        write_crash_dump(temp.path(), &sample_dump("test_panic_message_67890"));

        let files = dump_files(temp.path());
        assert_eq!(files.len(), 1, "应写出一个 dump 文件");

        let content = fs::read_to_string(&files[0]).unwrap();
        let parsed: CrashDump = serde_json::from_str(&content).expect("dump 应是合法 JSON");
        assert_eq!(parsed.payload, "test_panic_message_67890");
        assert_eq!(parsed.thread, "test-thread");
    }

    #[test]
    fn test_crash_dump_lru_cleanup() {
        let temp = TempDir::new().unwrap();

        // 写满上限 + 3，每次都触发一轮 LRU 清理
        for i in 0..MAX_CRASH_DUMPS + 3 {
            write_crash_dump(temp.path(), &sample_dump(&format!("crash_{i}")));
        }

        let files = dump_files(temp.path());
        assert_eq!(
            files.len(),
            MAX_CRASH_DUMPS,
            "crash 目录应只保留 {MAX_CRASH_DUMPS} 个 dump"
        );
    }

    #[test]
    fn test_same_instant_dumps_do_not_collide() {
        let temp = TempDir::new().unwrap();

        // 连续两次写入（同一秒内）必须产生两个独立文件——秒级时间戳会撞名，
        // 导致并发写入把 JSON 截断成非法内容。
        write_crash_dump(temp.path(), &sample_dump("first"));
        write_crash_dump(temp.path(), &sample_dump("second"));

        let files = dump_files(temp.path());
        assert_eq!(files.len(), 2, "同一秒内的两次 dump 不应互相覆盖");

        for file in &files {
            let content = fs::read_to_string(file).unwrap();
            serde_json::from_str::<CrashDump>(&content).expect("每个 dump 都应是合法 JSON");
        }
    }

    #[test]
    fn test_panic_hook_captures_real_panic() {
        // 装 hook 改的是进程级全局状态，与其他装全局状态的测试串行。
        let _global = crate::observability::GLOBAL_STATE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let temp = TempDir::new().unwrap();
        let crash_dir = temp.path().to_path_buf();

        let previous = panic::take_hook();
        init_panic_hook(crash_dir.clone()).expect("init_panic_hook 应成功");

        // 在子线程 panic，避免杀掉测试进程
        let handle = std::thread::spawn(|| panic!("test_panic_message_67890"));
        let _ = handle.join(); // 预期 Err（panic 已传播）

        // 还原 hook，避免影响后续测试
        panic::set_hook(previous);

        let files = dump_files(&crash_dir);
        assert!(!files.is_empty(), "crash 目录应有 dump 文件");

        let content = fs::read_to_string(&files[0]).unwrap();
        let dump: CrashDump = serde_json::from_str(&content).expect("dump 应是合法 JSON");
        assert!(dump.payload.contains("test_panic_message_67890"));
        assert!(!dump.thread.is_empty());
    }
}
