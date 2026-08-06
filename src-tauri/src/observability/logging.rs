use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// 构建 subscriber：按天滚动日志 + 保留 7 天 + 异步写入，但**不**注册为全局。
///
/// 拆出这一层是因为 tracing 的全局默认 subscriber 每个进程只能设置一次
/// （`.init()` 第二次调用会 panic）。构建与注册分离后：
/// - 生产代码走 [`init_logging`]，进程启动时注册一次；
/// - 测试用 `tracing::subscriber::with_default` 装线程局部 subscriber，
///   可重复、可并行，不碰全局状态。
///
/// # 返回
/// - subscriber：待注册（全局或线程局部）
/// - [`WorkerGuard`]：持有期间保证异步 flush；drop 时阻塞写完剩余日志
fn build_subscriber(
    log_dir: PathBuf,
) -> Result<(impl tracing::Subscriber + Send + Sync + 'static, WorkerGuard)> {
    // 1. 确保日志目录存在
    std::fs::create_dir_all(&log_dir)
        .with_context(|| format!("创建日志目录失败: {:?}", log_dir))?;

    // 2. 按天滚动（Rotation::DAILY），文件名 inkos.YYYY-MM-DD.log
    //    （tracing-appender 用 "." 连接 prefix / 日期 / suffix）
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
    let file_layer = fmt::layer().json().with_writer(non_blocking);
    let stdout_layer = fmt::layer().compact().with_writer(std::io::stdout);

    // 5. 环境变量过滤（默认 INFO，可通过 RUST_LOG 覆盖）
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stdout_layer);

    Ok((subscriber, guard))
}

/// 初始化全局 tracing subscriber。进程内只应调用一次（重复调用会 panic）。
///
/// # 返回
/// - [`WorkerGuard`]：需持有到进程结束，否则退出时可能丢失尾部日志
pub fn init_logging(log_dir: PathBuf) -> Result<WorkerGuard> {
    let (subscriber, guard) = build_subscriber(log_dir)?;
    subscriber.init();
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// 用线程局部 subscriber 跑闭包——不触碰全局默认，因此可并行、可重复调用。
    fn with_logging<T>(log_dir: PathBuf, f: impl FnOnce() -> T) -> T {
        let (subscriber, guard) = build_subscriber(log_dir).expect("build_subscriber 应成功");
        let out = tracing::subscriber::with_default(subscriber, f);
        drop(guard); // 阻塞 flush，保证断言能读到日志内容
        out
    }

    #[test]
    fn test_build_subscriber_creates_log_file() {
        let temp = TempDir::new().unwrap();
        let log_dir = temp.path().to_path_buf();

        with_logging(log_dir.clone(), || {});

        let entries: Vec<_> = fs::read_dir(&log_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(!entries.is_empty(), "日志目录应有文件");

        // tracing-appender 用 "." 连接 prefix / 日期 / suffix，
        // 因此实际文件名是 inkos.YYYY-MM-DD.log（不是 inkos-YYYY-MM-DD.log）。
        let log_file = entries[0].file_name();
        let name = log_file.to_str().unwrap();
        assert!(
            name.starts_with("inkos."),
            "日志文件应以 inkos. 开头，实际: {name}"
        );
        assert!(
            name.ends_with(".log"),
            "日志文件应以 .log 结尾，实际: {name}"
        );
    }

    #[test]
    fn test_tracing_writes_to_file() {
        let temp = TempDir::new().unwrap();
        let log_dir = temp.path().to_path_buf();

        with_logging(log_dir.clone(), || {
            tracing::info!("test_message_12345");
        });

        let entries: Vec<_> = fs::read_dir(&log_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        let content = fs::read_to_string(entries[0].path()).unwrap();
        assert!(content.contains("test_message_12345"), "日志应包含测试消息");
    }
}
