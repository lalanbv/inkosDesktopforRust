//! 可观测性基础设施：结构化日志 + 崩溃上报 + 诊断命令。

pub mod logging;
pub mod crash;
pub mod diagnostics;

pub use logging::init_logging;
pub use crash::init_panic_hook;
pub use diagnostics::{cmd_get_diagnostics, DiagnosticInfo};

/// 串行化「安装进程级全局状态」的测试。
///
/// `init_logging`（tracing 全局 subscriber）与 `init_panic_hook`（`std::panic`
/// 全局钩子）都改的是进程唯一的一份状态。cargo test 默认多线程并行跑，两个测试
/// 同时装会互相覆盖——后装的钩子接走前一个测试预期的 panic，dump 落进别人的临时
/// 目录，表现为随机失败。持这把锁即可保证同一时刻只有一个这类测试在跑。
#[cfg(test)]
pub(crate) static GLOBAL_STATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
