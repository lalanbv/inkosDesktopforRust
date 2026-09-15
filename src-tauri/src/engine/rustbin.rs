//! Rust 引擎后端（inkos-engine-server）的定位与启动规格。
//!
//! 绞杀者终切（全面 Rust 化迁移规划 §4 Phase 3 退出标准）：桌壳默认拉起
//! Rust 引擎二进制，Node sidecar 降级为回退路径。三面兼容性已由引擎侧验证：
//! - 契约：`/api/v1/*` 107 端点 + SSE `/api/v1/events`（strangler duel 8/8）；
//! - 磁盘：同读 `{project_root}/books`、`.inkos/secrets.json`（无状态迁移）；
//! - 前端：`INKOS_STATIC_DIR` 静态面 = Node sidecar `startStudioServer` 对应物。
//!
//! 健康探测差异：Rust 引擎的 `/` 仅在静态面存在时 200（纯 API 模式 404），
//! 而 `/api/v1/health` 恒 200（62 号 Rust 超集端点）——探测路径由调用方按后端选择。

use std::path::{Path, PathBuf};

use crate::config::{
    RUST_ENGINE_DIR_NAME, RUST_GENRES_DIR_NAME, RUST_SERVER_BIN_NAME,
    RUST_SKILLS_DIR_NAME, RUST_STATIC_DIR_NAME,
};
use crate::paths::PathResolver;
use crate::supervisor::LaunchSpec;
use std::collections::HashMap;

/// 解析启动用 inkos-engine-server 二进制路径。
///
/// 优先级：
/// 1. `app_data/engine-rust/inkos-engine-server`（未来 Rust 引擎 updater 的
///    运行态副本落点，与 Node 引擎 `app_data/engine` 对称）；
/// 2. `resource_dir/engine-rust/inkos-engine-server`（prod：bundle.resources
///    打包副本，desktop-package-rust-engine.sh 组装）；
/// 3. **debug 构建（dev）**：`{dev_repo_root}/engine-rs/target/{release,debug}/
///    inkos-engine-server`（cargo build --release 产物，先 release 后 debug）。
///
/// 全部 miss → `None`（调用方回退 Node 后端并告警，不阻断启动）。
///
/// `dev_repo_root` = 仓根（= `CARGO_MANIFEST_DIR/..`）；prod 下是烘焙的构建机
/// 路径（用户机不存在），与 [`super::resolve_engine_dir`] 的 dev_engine_root
/// 语义一致——prod 必走前两级。
pub fn resolve_server_bin(
    app_data: Option<&Path>,
    resource_dir: Option<&Path>,
    _dev_repo_root: &Path,
) -> Option<PathBuf> {
    // 1. app_data 运行态副本（updater 落点）。
    if let Some(ad) = app_data {
        let candidate = ad.join(RUST_ENGINE_DIR_NAME).join(RUST_SERVER_BIN_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // 2. prod：resource_dir 打包副本。
    if let Some(rd) = resource_dir {
        let candidate = rd.join(RUST_ENGINE_DIR_NAME).join(RUST_SERVER_BIN_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // 3. dev：engine-rs/target 构建产物（release 优先，debug 兜底）。
    #[cfg(debug_assertions)]
    {
        let target = _dev_repo_root.join("engine-rs").join("target");
        for profile in ["release", "debug"] {
            let candidate = target.join(profile).join(RUST_SERVER_BIN_NAME);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 解析 Rust 引擎的静态前端目录（`INKOS_STATIC_DIR`）。
///
/// - prod：`resource_dir/engine-rust/static`（打包脚本放入 studio dist 副本）；
/// - dev：`{dev_repo_root}/packages/studio/dist`（`desktop-build-inkos.sh` 产物）。
///
/// 缺失 → `None`（纯 API 模式：引擎仍健康可探 `/api/v1/health`，但 webview
/// 导航 `/` 会 404——dev 未构建前端时的预期形态，调用方告警提示）。
/// 解析引擎内置技能/题材目录（489 号部署缺口修复）。
///
/// 桌面默认引擎的 builtin 根 = env 或 `assets/*` 相对 CWD，而 cwd=用户项目根
/// 必缺失 → 15 内置技能/15 内置题材静默不可用（488 号差分器坐实 16 vs 1 /
/// 15 vs 2）。打包副本 = engine-rust/{skills,genres}（desktop-package-rust-
/// engine.sh 组装）；dev 回退 = 仓根 packages/core/{skills,genres}（TS 同源）。
/// 两目录必须同时在场才返回（半套不如不注入，保持引擎 env/缺省语义单一）。
pub fn resolve_builtin_asset_dirs(
    resource_dir: Option<&Path>,
    _dev_repo_root: &Path,
) -> Option<(PathBuf, PathBuf)> {
    if let Some(rd) = resource_dir {
        let engine_dir = rd.join(RUST_ENGINE_DIR_NAME);
        let skills = engine_dir.join(RUST_SKILLS_DIR_NAME);
        let genres = engine_dir.join(RUST_GENRES_DIR_NAME);
        if skills.is_dir() && genres.is_dir() {
            return Some((skills, genres));
        }
    }
    #[cfg(debug_assertions)]
    {
        let skills = _dev_repo_root.join("packages").join("core").join("skills");
        let genres = _dev_repo_root.join("packages").join("core").join("genres");
        if skills.is_dir() && genres.is_dir() {
            return Some((skills, genres));
        }
    }
    None
}

pub fn resolve_static_dir(
    resource_dir: Option<&Path>,
    _dev_repo_root: &Path,
) -> Option<PathBuf> {
    if let Some(rd) = resource_dir {
        let candidate = rd.join(RUST_ENGINE_DIR_NAME).join(RUST_STATIC_DIR_NAME);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    #[cfg(debug_assertions)]
    {
        let candidate = _dev_repo_root.join("packages").join("studio").join("dist");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Rust 引擎健康探测路径（恒 200 的超集端点，不依赖静态面）。
pub const HEALTH_PROBE_PATH: &str = "/api/v1/health";

/// 构造 Rust 引擎启动规格。
///
/// - `program` = server 二进制；`args` = 空（配置全走 env，与 bin 契约一致）；
/// - `env`：`INKOS_PORT`（监听端口）、`INKOS_PROJECT_ROOT`（项目根——bin 默认
///   读 CWD，显式注入消除对 cwd 的隐式依赖）、`INKOS_STATIC_DIR`（可选静态面）；
/// - `cwd` = `project_root`（与 env 等价的兜底，且与 Node 路径行为一致）。
///
/// 纯逻辑：不 spawn、不做 I/O。LLM 端点 env（`INKOS_LLM_*` 等）不注入——
/// 引擎侧 `effective_router` 热解析 inkos.json 服务项 + secrets 优先，
/// 启动 env 仅是回退端点，桌面场景无需预设。
pub fn build_launch<R: PathResolver>(
    paths: &R,
    port: u16,
    server_bin: &Path,
    static_dir: Option<&Path>,
    builtin_dirs: Option<&(PathBuf, PathBuf)>,
) -> LaunchSpec {
    let mut env = HashMap::new();
    env.insert("INKOS_PORT".to_string(), port.to_string());
    env.insert(
        "INKOS_PROJECT_ROOT".to_string(),
        paths.project_root().to_string_lossy().into_owned(),
    );
    if let Some(dir) = static_dir {
        env.insert(
            "INKOS_STATIC_DIR".to_string(),
            dir.to_string_lossy().into_owned(),
        );
    }
    if let Some((skills_dir, genres_dir)) = builtin_dirs {
        // 489 号：内置技能/题材根注入——缺失时默认引擎静默丢 15 技能/15 题材。
        env.insert(
            "INKOS_BUILTIN_SKILLS_DIR".to_string(),
            skills_dir.to_string_lossy().into_owned(),
        );
        env.insert(
            "INKOS_BUILTIN_GENRES_DIR".to_string(),
            genres_dir.to_string_lossy().into_owned(),
        );
    }
    LaunchSpec {
        program: server_bin.to_string_lossy().into_owned(),
        args: Vec::new(),
        env,
        cwd: paths.project_root().to_path_buf(),
        port,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyPaths {
        proj: PathBuf,
    }
    impl PathResolver for DummyPaths {
        fn project_root(&self) -> &std::path::Path {
            &self.proj
        }
        fn launch_engine_dir(&self) -> &std::path::Path {
            &self.proj
        }
        fn runtime_dir(&self) -> PathBuf {
            self.proj.join("runtime")
        }
        fn log_dir(&self) -> PathBuf {
            self.proj.join("log")
        }
        fn projects_path(&self) -> PathBuf {
            self.proj.join("projects.json")
        }
        fn updates_staging_dir(&self) -> PathBuf {
            self.proj.join("updates/staging")
        }
        fn engine_dir(&self) -> PathBuf {
            self.proj.join("engine")
        }
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"#!/bin/sh\n").unwrap();
    }

    #[test]
    fn app_data_copy_wins_over_resource_and_dev() {
        let app_data = tempfile::tempdir().unwrap();
        let resource = tempfile::tempdir().unwrap();
        let dev_root = tempfile::tempdir().unwrap(); // 无 engine-rs target
        let bin = app_data.path().join(RUST_ENGINE_DIR_NAME).join(RUST_SERVER_BIN_NAME);
        touch(&bin);
        let resource_bin =
            resource.path().join(RUST_ENGINE_DIR_NAME).join(RUST_SERVER_BIN_NAME);
        touch(&resource_bin);

        let resolved =
            resolve_server_bin(Some(app_data.path()), Some(resource.path()), dev_root.path());
        assert_eq!(resolved, Some(bin), "app_data 运行态副本应最优先");
    }

    #[test]
    fn resource_copy_used_when_app_data_missing() {
        let app_data = tempfile::tempdir().unwrap(); // 空
        let resource = tempfile::tempdir().unwrap();
        let dev_root = tempfile::tempdir().unwrap();
        let bin = resource.path().join(RUST_ENGINE_DIR_NAME).join(RUST_SERVER_BIN_NAME);
        touch(&bin);

        let resolved =
            resolve_server_bin(Some(app_data.path()), Some(resource.path()), dev_root.path());
        assert_eq!(resolved, Some(bin), "prod resource 副本次优先");
    }

    #[test]
    fn none_when_no_candidate_exists() {
        let app_data = tempfile::tempdir().unwrap();
        let resource = tempfile::tempdir().unwrap();
        let dev_root = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_server_bin(Some(app_data.path()), Some(resource.path()), dev_root.path()),
            None,
            "全 miss 应回退 Node 后端"
        );
    }

    #[test]
    fn static_dir_prefers_resource_then_dev() {
        let resource = tempfile::tempdir().unwrap();
        let dev_root = tempfile::tempdir().unwrap();
        let rstatic = resource.path().join(RUST_ENGINE_DIR_NAME).join(RUST_STATIC_DIR_NAME);
        std::fs::create_dir_all(&rstatic).unwrap();
        assert_eq!(
            resolve_static_dir(Some(resource.path()), dev_root.path()),
            Some(rstatic)
        );

        // resource 无 → dev 的 packages/studio/dist（debug 断言）。
        #[cfg(debug_assertions)]
        {
            let resource2 = tempfile::tempdir().unwrap();
            let dev_dist = dev_root.path().join("packages").join("studio").join("dist");
            std::fs::create_dir_all(&dev_dist).unwrap();
            assert_eq!(
                resolve_static_dir(Some(resource2.path()), dev_root.path()),
                Some(dev_dist)
            );
        }
    }

    #[test]
    fn build_launch_sets_env_contract() {
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp/proj"),
        };
        let bin = PathBuf::from("/opt/engine-rust/inkos-engine-server");
        let static_dir = PathBuf::from("/opt/engine-rust/static");

        let spec = build_launch(&paths, 7788, &bin, Some(&static_dir), None);
        assert_eq!(spec.program, "/opt/engine-rust/inkos-engine-server");
        assert!(spec.args.is_empty(), "Rust 引擎配置全走 env，无 args");
        assert_eq!(spec.env.get("INKOS_PORT").unwrap(), "7788");
        assert_eq!(spec.env.get("INKOS_PROJECT_ROOT").unwrap(), "/tmp/proj");
        assert_eq!(
            spec.env.get("INKOS_STATIC_DIR").unwrap(),
            "/opt/engine-rust/static"
        );
        assert_eq!(spec.cwd, PathBuf::from("/tmp/proj"));
        assert_eq!(spec.port, 7788);
    }

    #[test]
    fn build_launch_without_static_dir_omits_env() {
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp/proj"),
        };
        let bin = PathBuf::from("/opt/engine-rust/inkos-engine-server");
        let spec = build_launch(&paths, 7788, &bin, None, None);
        assert!(
            !spec.env.contains_key("INKOS_STATIC_DIR"),
            "无静态面不应注入空 INKOS_STATIC_DIR（空值会被 bin 当目录解析）"
        );
    }

    #[test]
    fn build_launch_injects_builtin_dirs_env() {
        // 489 号：内置技能/题材根注入——桌面默认引擎此前 cwd=项目根必缺失
        // assets/*，静默丢 15 技能/15 题材（双引擎差分器坐实）。
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp/proj"),
        };
        let bin = PathBuf::from("/opt/engine-rust/inkos-engine-server");
        let builtin = (
            PathBuf::from("/opt/engine-rust/skills"),
            PathBuf::from("/opt/engine-rust/genres"),
        );
        let spec = build_launch(&paths, 7788, &bin, None, Some(&builtin));
        assert_eq!(
            spec.env.get("INKOS_BUILTIN_SKILLS_DIR").unwrap(),
            "/opt/engine-rust/skills"
        );
        assert_eq!(
            spec.env.get("INKOS_BUILTIN_GENRES_DIR").unwrap(),
            "/opt/engine-rust/genres"
        );

        let spec_without = build_launch(&paths, 7788, &bin, None, None);
        assert!(!spec_without.env.contains_key("INKOS_BUILTIN_SKILLS_DIR"));
        assert!(!spec_without.env.contains_key("INKOS_BUILTIN_GENRES_DIR"));
    }

    #[test]
    fn resolve_builtin_asset_dirs_requires_both_dirs() {
        let resource = tempfile::tempdir().unwrap();
        let dev_root = tempfile::tempdir().unwrap();
        let engine_dir = resource.path().join(RUST_ENGINE_DIR_NAME);
        mkdir_all(&engine_dir.join(RUST_SKILLS_DIR_NAME));
        // genres 缺失 → 不注入半套
        assert!(resolve_builtin_asset_dirs(Some(resource.path()), dev_root.path()).is_none());
        mkdir_all(&engine_dir.join(RUST_GENRES_DIR_NAME));
        let resolved = resolve_builtin_asset_dirs(Some(resource.path()), dev_root.path());
        assert!(resolved.is_some(), "skills+genres 双在 → 注入");
    }

    fn mkdir_all(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
    }
}
