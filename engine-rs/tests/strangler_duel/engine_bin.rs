//! 对跑支撑模块：真实 `inkos-engine-server` 二进制的启动命令。
//!
//! 1672 号从 `tests/llm_config_duel.rs`（1669 号）并入对跑二进制时抽出：真实
//! 二进制的定位与启动只此一处。由 `tests/strangler_duel.rs` 以 `#[path]` 引入
//! ——`tests/` 一级目录下的文件各是一个集成测试目标，同名子目录里的不是，cargo
//! 不会把它当成独立二进制去跑。

/// 真实引擎二进制的启动命令。产物路径由 cargo 的 `CARGO_BIN_EXE` 约定定位
/// （assert_cmd `cargo_bin`，仓内既定形态，见 `parent_watchdog_e2e`）。
pub fn engine_command() -> std::process::Command {
    let engine_bin = assert_cmd::cargo::cargo_bin("inkos-engine-server");
    std::process::Command::new(engine_bin)
}
