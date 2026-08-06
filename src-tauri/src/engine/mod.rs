//! engine 模块：inkos 自包含运行时（dist + 资产 + node_modules）的定位与校验。
//!
//! M3a：定义 engine 目录解析（[`resolve_engine_dir`]）+ manifest 读写（[`manifest`]）。
//! M3c 增 node bootstrap（`node.rs`）；M3d 增 updater（独立 `updater` 模块）。

pub mod manifest;
pub mod node;

use std::path::{Path, PathBuf};

use crate::config::ENGINE_DIR_NAME;

/// 解析**启动用** engine 源目录。
///
/// 优先级：
/// 1. **debug 构建（dev）**：`dev_engine_root/engine`（= `CARGO_MANIFEST_DIR`/engine =
///    `src-tauri/engine`，原始组装，node_modules 符号链接完好）。
///    —— 必须优先：Tauri 在 dev 会把 resource（含 engine/）**复制**到 `target/debug/`，
///    符号链接在复制中断裂（node_modules → packages/cli/node_modules 失效）→
///    `Cannot find package 'commander'`。用原始 src-tauri/engine 避开。
/// 2. **release 构建（prod）**：`resource_dir/engine`（打包 .app 内，M3e 用真实自包含
///    node_modules，无符号链接问题）。
/// 3. 兜底：`dev_engine_root/engine`。
///
/// `dev_engine_root` = `CARGO_MANIFEST_DIR`（= src-tauri）。prod 下它是烘焙的构建机路径
/// （用户机不存在，即 C1 bug），故 prod 必走 resource_dir。
///
/// 纯函数（无 Tauri 依赖）：main 把 `app.path().resource_dir()` 的 Option 与
/// `CARGO_MANIFEST_DIR` 传入。
pub fn resolve_engine_dir(resource_dir: Option<&Path>, dev_engine_root: &Path) -> PathBuf {
    // dev（debug 构建）：优先原始 src-tauri/engine（符号链接完好，避开 target/debug 复制副本）。
    #[cfg(debug_assertions)]
    {
        let dev = dev_engine_root.join(ENGINE_DIR_NAME);
        if dev.is_dir() {
            return dev;
        }
    }
    // prod（release 构建）或 dev 原始缺失：resource_dir/engine。
    if let Some(rd) = resource_dir {
        let candidate = rd.join(ENGINE_DIR_NAME);
        if candidate.is_dir() {
            return candidate;
        }
    }
    dev_engine_root.join(ENGINE_DIR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试用 tempdir 作 dev_engine_root（其下无 engine）→ debug_assertions 分支确定性跳过，
    // 覆盖 prod（resource_dir）与兜底（dev_engine_root）路径。

    #[test]
    fn prefers_resource_dir_engine_when_dev_missing() {
        let resource = tempfile::tempdir().unwrap();
        let dev_root = tempfile::tempdir().unwrap(); // 空，无 engine
        std::fs::create_dir_all(resource.path().join(ENGINE_DIR_NAME)).unwrap();
        let resolved = resolve_engine_dir(Some(resource.path()), dev_root.path());
        assert_eq!(resolved, resource.path().join(ENGINE_DIR_NAME));
    }

    #[test]
    fn falls_back_to_dev_engine_root_when_resource_missing() {
        let resource = tempfile::tempdir().unwrap(); // 无 engine
        let dev_root = tempfile::tempdir().unwrap(); // 无 engine
        let resolved = resolve_engine_dir(Some(resource.path()), dev_root.path());
        assert_eq!(resolved, dev_root.path().join(ENGINE_DIR_NAME));
    }

    #[test]
    fn falls_back_to_dev_engine_root_when_resource_none() {
        let dev_root = tempfile::tempdir().unwrap();
        let resolved = resolve_engine_dir(None, dev_root.path());
        assert_eq!(resolved, dev_root.path().join(ENGINE_DIR_NAME));
    }

    #[test]
    #[cfg(debug_assertions)]
    fn debug_prefers_dev_engine_root_when_present() {
        // dev（debug）：dev_engine_root/engine 存在 → 优先于 resource_dir（避开 target/debug 复制副本）。
        let resource = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(resource.path().join(ENGINE_DIR_NAME)).unwrap();
        let dev_root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dev_root.path().join(ENGINE_DIR_NAME)).unwrap();
        let resolved = resolve_engine_dir(Some(resource.path()), dev_root.path());
        assert_eq!(resolved, dev_root.path().join(ENGINE_DIR_NAME));
    }
}
