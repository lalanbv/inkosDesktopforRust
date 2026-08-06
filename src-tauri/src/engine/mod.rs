//! engine 模块：inkos 自包含运行时（dist + 资产 + node_modules）的定位与校验。
//!
//! M3a：定义 engine 目录解析（[`resolve_engine_dir`]）+ manifest 读写（[`manifest`]）。
//! M3c 增 node bootstrap（`node.rs`）；M3d 增 updater（独立 `updater` 模块）。

pub mod manifest;

use std::path::{Path, PathBuf};

use crate::config::ENGINE_DIR_NAME;

/// 解析**启动用** engine 源目录。
///
/// 优先 prod：`resource_dir/engine`（Tauri 打包后随包分发的 resource，存在即用）。
/// 回退 dev：`dev_engine_root/engine`（`desktop-package-engine.sh` 在 `src-tauri/engine`
/// 组装；`dev_engine_root` = `CARGO_MANIFEST_DIR` = src-tauri 目录）。
///
/// 为何 dev 回退用 `CARGO_MANIFEST_DIR`（src-tauri）而非仓库根：零交叉纪律要求
/// engine/ 只进 `src-tauri/`（被 `src-tauri/.gitignore` 忽略），故 engine 与 Cargo.toml
/// 同级。prod 下 `CARGO_MANIFEST_DIR` 是烘焙的构建机路径（用户机不存在，即 C1 bug），
/// 故 prod 必须走 `resource_dir`——这正是本函数优先 resource_dir 的原因。
///
/// 纯函数（无 Tauri 依赖）：main 把 `app.path().resource_dir()` 的 Option 与
/// `CARGO_MANIFEST_DIR` 传入，便于单测覆盖两条分支。
///
/// 注意：此为**启动源**（只读）；M3d updater 维护的**运行态副本**在
/// `app_data/engine`（[`crate::paths::PathResolver::engine_dir`]），两者解耦。
pub fn resolve_engine_dir(resource_dir: Option<&Path>, dev_engine_root: &Path) -> PathBuf {
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

    #[test]
    fn prefers_resource_dir_engine_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let resource = tmp.path();
        // 模拟 prod：resource/engine 存在。
        std::fs::create_dir_all(resource.join(ENGINE_DIR_NAME)).unwrap();
        let resolved = resolve_engine_dir(Some(resource), Path::new("/Users/me/src-tauri"));
        assert_eq!(resolved, resource.join(ENGINE_DIR_NAME));
    }

    #[test]
    fn falls_back_to_dev_engine_root_when_resource_missing() {
        // resource_dir 给定但其下无 engine → 回退 dev_engine_root/engine。
        let tmp = tempfile::tempdir().unwrap();
        let resolved = resolve_engine_dir(Some(tmp.path()), Path::new("/Users/me/src-tauri"));
        assert_eq!(resolved, PathBuf::from("/Users/me/src-tauri/engine"));
    }

    #[test]
    fn falls_back_to_dev_engine_root_when_resource_none() {
        // dev 无 resource_dir → 直接 dev_engine_root/engine（= src-tauri/engine）。
        let resolved = resolve_engine_dir(None, Path::new("/Users/me/src-tauri"));
        assert_eq!(resolved, PathBuf::from("/Users/me/src-tauri/engine"));
    }
}
