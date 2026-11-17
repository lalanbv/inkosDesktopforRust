//! E2E 启动冒烟测试
//!
//! 验证 app 正常启动、日志初始化、版本记录。
//!
//! 运行：cargo test --release --test e2e_startup -- --ignored --test-threads=1
//!
//! 1431 号备案（形态敏感性）：app **无** data-dir override 契约——本测试曾注入
//! `INKOS_APP_DATA_DIR`（死契约，app 端零消费），实际隔离从未生效；测试结果
//! 依赖用户机器的全局 `projects.json` 形态：`last_opened` 有效（如 1426 号在
//! 本机种入仓根）→ auto 直进 studio + spawn 引擎；无效 → picker 挂 5s。两
//! 形态在 supervisor::spawn stdio null 化（1431 号）后均 5s 内 EOF 必达（孤儿
//! sidecar 不再握管道），本测试于两形态下皆绿。

use assert_cmd::Command;
use std::time::Duration;

#[test]
#[ignore] // 需要 release binary，CI 专用
fn test_startup_smoke() {
    // 启动 app（headless，5s 超时）
    let mut cmd = Command::cargo_bin("inkos-desktop").unwrap();
    cmd.timeout(Duration::from_secs(5));

    // app 启动后会等待用户选择项目，超时是预期行为
    // 只要 stdout 有日志输出即表示启动成功
    let output = cmd.output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("inkosDesktop 启动") || stdout.contains("INFO"),
        "应该输出启动日志"
    );
}

#[test]
#[ignore]
fn test_startup_logs_version() {
    // 1431 号：对齐 app 真实写入面（main.rs init_logging = 用户级
    // data_dir/<APP_DATA_DIR_NAME>/logs）——原 INKOS_APP_DATA_DIR 注入为死
    // 契约（app 端零消费），log_dir 检查自始走「目录不存在 → 宽容 return」
    // 死分支。对齐后三分支结构保留：目录在 → 验文件名格式；无文件/无目录
    // → 宽容通过（本机首启/日志清理后形态）。
    let log_dir = dirs::data_dir()
        .map(|d| {
            d.join(inkos_desktop::config::APP_DATA_DIR_NAME)
                .join("logs")
        })
        .unwrap_or_else(|| {
            std::env::temp_dir()
                .join(inkos_desktop::config::APP_DATA_DIR_NAME)
                .join("logs")
        });

    let mut cmd = Command::cargo_bin("inkos-desktop").unwrap();
    cmd.timeout(Duration::from_secs(5));

    // 启动 app（超时是预期的）
    let _ = cmd.output();

    // 验证日志目录存在
    if !log_dir.exists() {
        // macOS 上可能日志写到系统目录，检查 stdout 有日志输出即可
        return;
    }

    // 验证日志文件创建
    let log_files: Vec<_> = std::fs::read_dir(&log_dir)
        .expect("日志目录应存在")
        .filter_map(Result::ok)
        .collect();

    if log_files.is_empty() {
        // 日志目录存在但无文件，可能还在缓冲，通过
        return;
    }

    // 验证日志文件名符合格式：logging.rs appender = prefix "inkos" + 日期 +
    // suffix "log"（tracing-appender 点号连接）→ `inkos.YYYY-MM-DD.log`。
    // 1431 号勘误：原断言验 `inkos-`（连字符）系死分支期手打形态，与生产
    // 实现自始漂移——分支激活后首次执行现形，按生产派生修正。
    let has_valid_log = log_files.iter().any(|e| {
        e.file_name().to_string_lossy().starts_with("inkos.")
            && e.file_name().to_string_lossy().ends_with(".log")
    });

    assert!(has_valid_log, "日志文件名应符合 inkos.YYYYMMDD.log 格式");
}
