//! 项目级事件日志通道（210 号）：`inkos.log` JSON 行追加写。
//!
//! 背景：双端 `GET /api/v1/logs` 均读 `projectRoot/inkos.log`，但此前
//! **零写入方**（Node logger 只有 stderr/SSE sink；Rust bin 无 tracing
//! subscriber，warn 全 no-op）——LogViewer/doctor 日志面整体空转。
//!
//! 形态对齐 Node `LogEntry`（`{timestamp, level, tag, message}`）——
//! get_logs 的 JSON 行解析与 UI LogViewer 字段面直接可用。本通道只承载
//! **任务级事件**（连写失败/中止等）：管线细粒度日志仍走 tracing（stdout
//! 面由壳层/终端订阅），避免在 lib 层 init 全局 subscriber 与 Tauri 壳
//! 的订阅冲突。写入失败静默（日志不得阻断业务）。

use std::io::Write;

/// 追加一条 JSON 行事件到 `{root}/inkos.log`（目录缺失自动创建）。
pub fn append_log_event(root: &std::path::Path, level: &str, tag: &str, message: &str) {
    if let Err(e) = append_log_event_checked(root, level, tag, message) {
        eprintln!("[log-file] inkos.log 追加失败（忽略）：{e}");
    }
}

/// 受检形态（供测试断言）。
pub fn append_log_event_checked(
    root: &std::path::Path,
    level: &str,
    tag: &str,
    message: &str,
) -> std::io::Result<()> {
    let path = root.join("inkos.log");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let entry = serde_json::json!({
        "timestamp": crate::utils::utc_time::utc_now_iso(),
        "level": level,
        "tag": tag,
        "message": message,
    });
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{entry}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_json_lines_parsable_by_get_logs() {
        let dir = tempfile::tempdir().unwrap();
        append_log_event_checked(dir.path(), "warn", "write-next", "已完成 1/2 章并落盘").unwrap();
        append_log_event_checked(dir.path(), "error", "write-next", "第 3 章失败：HTTP 429").unwrap();
        let content = std::fs::read_to_string(dir.path().join("inkos.log")).unwrap();
        let lines: Vec<&str> = content.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed["level"].is_string());
            assert!(parsed["tag"].is_string());
            assert!(parsed["message"].is_string());
            assert!(parsed["timestamp"].is_string());
        }
    }
}
