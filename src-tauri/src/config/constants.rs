//! 全局常量收口（架构 minor M5：端口/超时/路径/版本零硬编码散落）。
//!
//! 命名规范：目录名 `*_DIR_NAME`、文件名 `*_FILE_NAME`、URL `*_URL`（M3c/M3d 增补）。
//! 所有 path 段均为单层（`engine`/`runtime`/...），多层由 `PathBuf::join` 组合，避免歧义。

/// inkos studio 默认端口（被占则 [`crate::supervisor::pick_free_port`] 递增）。
pub const DEFAULT_STUDIO_PORT: u16 = 4567;

/// 健康探测总超时（架构 §5.1：最长 30s）。
pub const HEALTH_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// 健康探测轮询间隔。
pub const HEALTH_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

/// CLI 入口相对 launch_engine_dir 的路径（engine/dist/index.js）。
///
/// engine/dist 是 packages/cli/dist 的副本（或 prod 下 pnpm deploy 的自包含 cli）；
/// cli 的 package.json `bin.dist/index.js` 即此入口。cliPackageRoot（studio.ts 经
/// import.meta.url 解析）= launch_engine_dir，其 `node_modules/@actalk/inkos-studio`
/// 候选由此命中（dev 经符号链接→repo node_modules；prod 经自包含 node_modules）。
pub const CLI_ENTRY_REL: &str = "dist/index.js";

/// OS app_data 下本应用目录名。
pub const APP_DATA_DIR_NAME: &str = "inkosDesktop";

/// engine 目录名（启动源 = resource_dir/engine 或 repo_root/engine；
/// 运行态副本 = app_data/engine，由 M3d updater 维护）。
pub const ENGINE_DIR_NAME: &str = "engine";

/// Rust 引擎目录名（inkos-engine-server 二进制 + static/ 前端 + manifest）。
/// 启动源 = resource_dir/engine-rust（打包）或 repo engine-rs/target（dev）；
/// 运行态副本 = app_data/engine-rust（未来 Rust 引擎 updater 落点）。
pub const RUST_ENGINE_DIR_NAME: &str = "engine-rust";

/// Rust 引擎服务二进制文件名（engine-rs 的 bin inkos-engine-server）。
pub const RUST_SERVER_BIN_NAME: &str = "inkos-engine-server";

/// Rust 引擎静态前端目录名（engine-rust/static = packages/studio/dist 副本，
/// INKOS_STATIC_DIR 直连面）。
pub const RUST_STATIC_DIR_NAME: &str = "static";

/// Rust 引擎内置技能目录名（engine-rust/skills = packages/core/skills 副本，
/// INKOS_BUILTIN_SKILLS_DIR 直连面——489 号部署缺口修复）。
pub const RUST_SKILLS_DIR_NAME: &str = "skills";

/// Rust 引擎内置题材目录名（engine-rust/genres = packages/core/genres 副本，
/// INKOS_BUILTIN_GENRES_DIR 直连面——489 号部署缺口修复）。
pub const RUST_GENRES_DIR_NAME: &str = "genres";

/// 后端选择 env 覆盖键（`rust`|`node`，大小写不敏感；优先于配置文件）。
pub const ENGINE_BACKEND_ENV: &str = "INKOS_ENGINE_BACKEND";

/// engine manifest 文件名。
pub const ENGINE_MANIFEST_FILE: &str = "manifest.json";

/// updater 回滚备份目录名（M3d）。
pub const ENGINE_BAK_DIR_NAME: &str = "engine.bak";

/// 运行时下载目录名（app_data 下，M3c node bootstrap 缓存于此）。
pub const RUNTIME_DIR_NAME: &str = "runtime";

/// node 二进制目录名（runtime/node/{ver}-{plat}-{arch}）。
pub const NODE_DIR_NAME: &str = "node";

/// 日志目录名（app_data/logs）。
pub const LOGS_DIR_NAME: &str = "logs";

/// 最近项目持久化文件名（app_data/projects.json，M3b）。
pub const PROJECTS_FILE_NAME: &str = "projects.json";

/// 项目内 inkos 密钥目录名（`.inkos`，与 inkos `loadSecrets` 默认一致）。
pub const SECRETS_DIR_NAME: &str = ".inkos";

/// 项目内 inkos 密钥文件名（`.inkos/secrets.json`）。
pub const SECRETS_FILE_NAME: &str = "secrets.json";

/// updater 下载暂存目录名（app_data/updates/staging，M3d）。
pub const UPDATES_DIR_NAME: &str = "updates";

/// updater 暂存子目录名。
pub const STAGING_DIR_NAME: &str = "staging";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_port_matches_inkos() {
        assert_eq!(DEFAULT_STUDIO_PORT, 4567);
    }

    #[test]
    fn cli_entry_is_engine_relative() {
        // M3a：CLI 入口相对 engine_dir（dist/index.js = cli package.json bin）。
        assert_eq!(CLI_ENTRY_REL, "dist/index.js");
    }

    #[test]
    fn dir_and_file_names_are_single_segments() {
        // 所有 *_DIR_NAME / *_FILE_NAME 必须单层（无 / ），防 join 歧义。
        for name in [
            APP_DATA_DIR_NAME,
            ENGINE_DIR_NAME,
            ENGINE_MANIFEST_FILE,
            ENGINE_BAK_DIR_NAME,
            RUNTIME_DIR_NAME,
            NODE_DIR_NAME,
            LOGS_DIR_NAME,
            PROJECTS_FILE_NAME,
            SECRETS_DIR_NAME,
            SECRETS_FILE_NAME,
            UPDATES_DIR_NAME,
            STAGING_DIR_NAME,
            RUST_ENGINE_DIR_NAME,
            RUST_SERVER_BIN_NAME,
            RUST_STATIC_DIR_NAME,
        ] {
            assert!(!name.contains(std::path::MAIN_SEPARATOR), "{name} 含路径分隔符");
            assert!(!name.contains('/'), "{name} 含 /");
            assert!(!name.is_empty(), "{name} 为空");
        }
    }
}
