use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::panic;
use std::time::{SystemTime, UNIX_EPOCH};

/// Crash dump 结构（JSON 序列化）
#[derive(Debug, Serialize, Deserialize)]
pub struct CrashDump {
    pub timestamp: u64,
    pub thread: String,
    pub payload: String,
    pub backtrace: String,
}

/// 初始化全局 panic hook，捕获崩溃并写 JSON dump 到 crash_dir。
///
/// # 行为
/// - 每次 panic 生成一个 `crash-<unix_timestamp>.json`
/// - 保留最近 10 个 dump（LRU 删除旧文件）
/// - 写完 dump 后调用默认 panic handler（保持终端输出）
pub fn init_panic_hook(crash_dir: PathBuf) -> Result<()> {
    use anyhow::Context;
    use std::fs;

    // 1. 确保 crash 目录存在
    fs::create_dir_all(&crash_dir)
        .with_context(|| format!("创建 crash 目录失败: {:?}", crash_dir))?;

    // 2. 保存默认 panic handler
    let default_panic = panic::take_hook();

    // 3. 设置自定义 panic hook
    panic::set_hook(Box::new(move |panic_info| {
        // 3.1 收集崩溃信息
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();

        let payload = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(|s| s.clone())
            })
            .unwrap_or_else(|| "Box<Any>".to_string());

        let backtrace = std::backtrace::Backtrace::force_capture().to_string();

        let dump = CrashDump {
            timestamp,
            thread,
            payload,
            backtrace,
        };

        // 3.2 写 crash dump
        let dump_path = crash_dir.join(format!("crash-{}.json", timestamp));
        if let Ok(json) = serde_json::to_string_pretty(&dump) {
            let _ = fs::write(&dump_path, json);
        }

        // 3.3 LRU 清理（保留最近 10 个）
        if let Ok(entries) = fs::read_dir(&crash_dir) {
            let mut files: Vec<_> = entries
                .filter_map(Result::ok)
                .filter(|e| {
                    e.path()
                        .file_name()
                        .and_then(|n| n.to_str())
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

            // 删除第 11 个及以后的文件
            for old_file in files.iter().skip(10) {
                let _ = fs::remove_file(old_file.path());
            }
        }

        // 3.4 调用默认 handler（保持终端输出）
        default_panic(panic_info);
    }));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_panic_hook_writes_crash_dump() {
        let temp = TempDir::new().unwrap();
        let crash_dir = temp.path().to_path_buf();

        init_panic_hook(crash_dir.clone()).expect("init_panic_hook 应成功");

        // 触发 panic（在子线程，避免杀测试进程）
        let handle = std::thread::spawn(|| {
            panic!("test_panic_message_67890");
        });
        let _ = handle.join(); // 预期 Err（panic 传播）

        // 验证 crash dump 创建
        let entries: Vec<_> = fs::read_dir(&crash_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(!entries.is_empty(), "crash 目录应有 dump 文件");

        let dump_file = entries[0].path();
        let content = fs::read_to_string(dump_file).unwrap();
        let dump: CrashDump = serde_json::from_str(&content).unwrap();

        assert!(dump.payload.contains("test_panic_message_67890"));
        assert!(!dump.thread.is_empty());
    }

    #[test]
    fn test_crash_dump_lru_cleanup() {
        let temp = TempDir::new().unwrap();
        let crash_dir = temp.path().to_path_buf();

        init_panic_hook(crash_dir.clone()).expect("init_panic_hook 应成功");

        // 模拟创建 12 个旧 crash dump（超出 10 个上限）
        for i in 0..12 {
            let dump = CrashDump {
                timestamp: 1000 + i,
                thread: "test".to_string(),
                payload: format!("crash_{}", i),
                backtrace: "".to_string(),
            };
            let path = crash_dir.join(format!("crash-{}.json", 1000 + i));
            fs::write(path, serde_json::to_string(&dump).unwrap()).unwrap();
        }

        // 触发新 panic（应触发 LRU 清理）
        let handle = std::thread::spawn(|| {
            panic!("trigger_lru_cleanup");
        });
        let _ = handle.join();

        // 验证只保留最近 10 个
        let entries: Vec<_> = fs::read_dir(&crash_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 10, "crash 目录应只保留 10 个 dump");
    }
}
