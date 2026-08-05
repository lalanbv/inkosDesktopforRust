pub mod jsonio;
pub mod store;
pub mod sync;

// 顶层 re-export：main.rs / 集成代码用 `secrets::sync_on_startup` /
// `secrets::spawn_writeback` 短路径访问；底层定义在 sync 模块。
pub use sync::{spawn_writeback, sync_on_startup};
