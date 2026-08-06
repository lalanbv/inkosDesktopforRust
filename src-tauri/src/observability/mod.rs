//! 可观测性基础设施：结构化日志 + 崩溃上报 + 诊断命令。

pub mod logging;
pub mod crash;
pub mod diagnostics;

pub use logging::init_logging;
pub use crash::init_panic_hook;
pub use diagnostics::{cmd_get_diagnostics, DiagnosticInfo};
