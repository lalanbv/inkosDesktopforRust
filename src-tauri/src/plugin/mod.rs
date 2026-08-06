//! WASM 插件系统
//!
//! 商业级扩展架构：安全沙箱 + 细粒度权限 + 热加载 + ABI 版本管理。
//!
//! ## 设计原则
//!
//! - **安全优先**：Wasmtime 沙箱，最小权限原则，资源限制
//! - **向后兼容**：ABI 版本化，降级加载，旧插件在新版本中仍可用
//! - **高性能 0GC**：预编译缓存，共享内存，零拷贝序列化
//! - **开发者友好**：wit 接口定义，自动生成绑定，清晰的错误信息

pub mod manifest;
pub mod types;

pub use manifest::{parse_manifest, PluginManifest};
pub use types::{Capability, PluginError, PluginMetadata};
