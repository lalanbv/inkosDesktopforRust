use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use tracing_appender::rolling::{RollingFileAppender, Rotation};

/// 初始化 tracing subscriber：按天滚动日志 + 保留 7 天 + 异步写入。
///
/// # 参数
/// - `log_dir`：日志目录（如 `<app_data>/logs`），函数内按日期追加文件名
///
/// # 返回
/// - `WorkerGuard`：持有到进程结束保证异步 flush；drop 时阻塞完成写入
pub fn init_logging(log_dir: PathBuf) -> Result<WorkerGuard> {
    // 1. 确保日志目录存在
    std::fs::create_dir_all(&log_dir)
        .with_context(|| format!("创建日志目录失败: {:?}", log_dir))?;

    // 2. 按天滚动（Rotation::DAILY），文件名 inkos-YYYY-MM-DD.log
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("inkos")
        .filename_suffix("log")
        .max_log_files(7) // 保留 7 天
        .build(&log_dir)
        .with_context(|| "创建滚动日志失败")?;

    // 3. 异步写入（non-blocking），返回 WorkerGuard
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // 4. 组合 subscriber：文件层（JSON）+ stdout 层（人类可读）
    let file_layer = fmt::layer()
        .json()
        .with_writer(non_blocking);

    let stdout_layer = fmt::layer()
        .compact()
        .with_writer(std::io::stdout);

    // 5. 环境变量过滤（默认 INFO，可通过 RUST_LOG 覆盖）
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // 6. 注册全局 subscriber
    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stdout_layer)
        .init();

    Ok(guard)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_init_logging_creates_log_file() {
        let temp = TempDir::new().unwrap();
        let log_dir = temp.path().to_path_buf();

        let _guard = init_logging(log_dir.clone()).expect("init_logging 应成功");

        // 验证日志文件创建
        let entries: Vec<_> = fs::read_dir(&log_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(!entries.is_empty(), "日志目录应有文件");

        // 验证文件名格式（inkos-YYYY-MM-DD.log）
        let log_file = entries[0].file_name();
        let name = log_file.to_str().unwrap();
        assert!(name.starts_with("inkos-"), "日志文件应以 inkos- 开头");
        assert!(name.ends_with(".log"), "日志文件应以 .log 结尾");
    }

    #[test]
    fn test_tracing_writes_to_file() {
        let temp = TempDir::new().unwrap();
        let log_dir = temp.path().to_path_buf();

        let _guard = init_logging(log_dir.clone()).expect("init_logging 应成功");

        // 写测试日志
        tracing::info!("test_message_12345");

        // 强制 flush（drop guard）
        drop(_guard);

        // 验证日志内容
        let entries: Vec<_> = fs::read_dir(&log_dir).unwrap().filter_map(Result::ok).collect();
        let log_file = entries[0].path();
        let content = fs::read_to_string(log_file).unwrap();
        assert!(content.contains("test_message_12345"), "日志应包含测试消息");
    }
}
