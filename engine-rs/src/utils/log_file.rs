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

/// 截头轮转上限（211 号）：超过则保留尾部 [`KEEP_LOG_BYTES`]，防止任务
/// 级日志无限增长（get_logs 只取尾 100 行，截头零信息损失）。
const MAX_LOG_BYTES: u64 = 512 * 1024;
const KEEP_LOG_BYTES: usize = 256 * 1024;

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
    // 轮转失败不阻断追加（静默继续——最坏情况只是文件继续增长）。
    let _ = rotate_if_oversized(&path, MAX_LOG_BYTES, KEEP_LOG_BYTES);
    let entry = serde_json::json!({
        "timestamp": crate::utils::utc_time::utc_now_iso(),
        "level": level,
        "tag": tag,
        "message": message,
    });
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{entry}")
}

/// 截头轮转：超 `max_bytes` 时按行边界保留尾部 `keep_bytes`，temp+rename
/// 原子替换（参数化上限供测试注入小值）。
fn rotate_if_oversized(path: &std::path::Path, max_bytes: u64, keep_bytes: usize) -> std::io::Result<()> {
    let Ok(meta) = std::fs::metadata(path) else {
        return Ok(());
    };
    if meta.len() <= max_bytes {
        return Ok(());
    }
    let content = std::fs::read(path)?;
    let cut = content.len().saturating_sub(keep_bytes);
    // 从切点向后找行边界（保 JSON 行完整）。
    let start = content[cut..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| cut + i + 1)
        .unwrap_or(cut);
    let tmp = path.with_extension("log.tmp");
    std::fs::write(&tmp, &content[start..])?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 211 号：截头轮转——超上限保留尾部（按行边界），首部被截、尾部保留。
    #[test]
    fn rotates_head_when_oversized() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inkos.log");
        let mk_line = |i: usize| {
            format!(
                "{{\"timestamp\":\"t\",\"level\":\"info\",\"tag\":\"t\",\"message\":\"line-{i} padding-padding-padding\"}}"
            )
        };
        // 直接构造超限文件（上限注入 512B / 保留 256B）。
        let mut content = String::new();
        for i in 0..40 {
            content.push_str(&mk_line(i));
            content.push('\n');
        }
        std::fs::write(&path, &content).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(before.len() > 512);
        rotate_if_oversized(&path, 512, 256).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.len() <= 300, "轮转后应接近保留窗：{}", after.len());
        // 首部行被截、尾行保留、所有行仍是完整 JSON。
        assert!(!after.contains("line-0\""));
        assert!(after.contains("line-39"));
        for line in after.trim().split('\n') {
            assert!(serde_json::from_str::<serde_json::Value>(line).is_ok(), "行边界保持完整：{line}");
        }
        // 未超限时不动文件。
        rotate_if_oversized(&path, 512, 256).unwrap();
        let again = std::fs::read_to_string(&path).unwrap();
        assert_eq!(again.len(), after.len());
    }

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
