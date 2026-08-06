//! 跨平台路径解析（架构 §4 paths 模块）。
//!
//! M3a 修订（审计 minor）：
//! - 移除 `submodule_root`（M3a 后无 Rust 代码再用 inkos 仓库根——engine 改由
//!   [`crate::engine::resolve_engine_dir`] 经 resource_dir/CARGO_MANIFEST_DIR 定位，
//!   即 src-tauri/engine）。比重命名死代码更干净。
//! - 新增 `launch_engine_dir`（启动用 engine 源，由 resolve_engine_dir 在 main 决定），
//!   与 `engine_dir`（app_data/engine，M3d updater 运行态副本目标）解耦。
//! - 收口 app_data 子目录（runtime/logs/projects/updates/engine），消除散落 join。

use std::path::{Path, PathBuf};

use crate::config::{
    APP_DATA_DIR_NAME, ENGINE_DIR_NAME, LOGS_DIR_NAME, PROJECTS_FILE_NAME, RUNTIME_DIR_NAME,
    STAGING_DIR_NAME, UPDATES_DIR_NAME,
};

/// 路径解析抽象（Adapter 模式）：测试用 mock，生产用 [`AppPaths`]。
pub trait PathResolver {
    /// 用户项目根（sidecar 的 cwd；inkos 数据写入此）。
    fn project_root(&self) -> &Path;
    /// 启动用 engine 源（resolve_engine_dir 结果；build_launch 的 cli_entry 基址）。
    fn launch_engine_dir(&self) -> &Path;
    /// 运行时下载目录（app_data/runtime；M3c node 缓存）。
    fn runtime_dir(&self) -> PathBuf;
    /// 日志目录（app_data/logs）。
    fn log_dir(&self) -> PathBuf;
    /// 最近项目持久化文件（app_data/projects.json；M3b）。
    fn projects_path(&self) -> PathBuf;
    /// updater 暂存目录（app_data/updates/staging；M3d）。
    fn updates_staging_dir(&self) -> PathBuf;
    /// engine 运行态副本目录（app_data/engine；M3d updater 替换目标）。
    fn engine_dir(&self) -> PathBuf;
}

/// 生产 PathResolver：基于 OS app_data 目录派生所有路径。
pub struct AppPaths {
    project_root: PathBuf,
    launch_engine_dir: PathBuf,
    app_data: PathBuf,
}

impl AppPaths {
    /// 构造：project_root（用户项目）、launch_engine_dir（启动 engine 源，由
    /// [`crate::engine::resolve_engine_dir`] 算出注入）。
    /// app_data 由 `dirs::data_dir()` 解析（失败则 Err，无静默吞错）。
    pub fn new(project_root: PathBuf, launch_engine_dir: PathBuf) -> anyhow::Result<Self> {
        let app_data = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("无法解析平台 data 目录"))?
            .join(APP_DATA_DIR_NAME);
        Ok(Self {
            project_root,
            launch_engine_dir,
            app_data,
        })
    }

    /// 测试用构造：允许注入 app_data（绕过 `dirs::data_dir` 的 OS 依赖）。
    #[cfg(test)]
    pub(crate) fn with_app_data(
        project_root: PathBuf,
        launch_engine_dir: PathBuf,
        app_data: PathBuf,
    ) -> Self {
        Self {
            project_root,
            launch_engine_dir,
            app_data,
        }
    }
}

impl PathResolver for AppPaths {
    fn project_root(&self) -> &Path {
        &self.project_root
    }
    fn launch_engine_dir(&self) -> &Path {
        &self.launch_engine_dir
    }
    fn runtime_dir(&self) -> PathBuf {
        self.app_data.join(RUNTIME_DIR_NAME)
    }
    fn log_dir(&self) -> PathBuf {
        self.app_data.join(LOGS_DIR_NAME)
    }
    fn projects_path(&self) -> PathBuf {
        self.app_data.join(PROJECTS_FILE_NAME)
    }
    fn updates_staging_dir(&self) -> PathBuf {
        self.app_data.join(UPDATES_DIR_NAME).join(STAGING_DIR_NAME)
    }
    fn engine_dir(&self) -> PathBuf {
        self.app_data.join(ENGINE_DIR_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AppPaths {
        AppPaths::with_app_data(
            PathBuf::from("/tmp/proj"),
            PathBuf::from("/tmp/src-tauri/engine"),
            PathBuf::from("/tmp/appdata"),
        )
    }

    #[test]
    fn resolves_project_and_launch_engine_roots() {
        let p = sample();
        assert_eq!(p.project_root(), Path::new("/tmp/proj"));
        assert_eq!(
            p.launch_engine_dir(),
            Path::new("/tmp/src-tauri/engine")
        );
    }

    #[test]
    fn log_dir_under_app_data() {
        let p = sample();
        assert_eq!(p.log_dir(), PathBuf::from("/tmp/appdata/logs"));
    }

    #[test]
    fn runtime_dir_under_app_data() {
        let p = sample();
        assert_eq!(p.runtime_dir(), PathBuf::from("/tmp/appdata/runtime"));
    }

    #[test]
    fn projects_path_under_app_data() {
        let p = sample();
        assert_eq!(p.projects_path(), PathBuf::from("/tmp/appdata/projects.json"));
    }

    #[test]
    fn updates_staging_dir_under_app_data() {
        let p = sample();
        assert_eq!(
            p.updates_staging_dir(),
            PathBuf::from("/tmp/appdata/updates/staging")
        );
    }

    #[test]
    fn engine_dir_under_app_data() {
        // app_data/engine = M3d updater 运行态副本目标（与 launch_engine_dir 解耦）。
        let p = sample();
        assert_eq!(p.engine_dir(), PathBuf::from("/tmp/appdata/engine"));
    }

    #[test]
    fn new_resolves_app_data_from_os() {
        // AppPaths::new 应从 dirs::data_dir() 解析 app_data（非空即成功）。
        let p = AppPaths::new(
            PathBuf::from("/tmp/proj"),
            PathBuf::from("/tmp/src-tauri/engine"),
        );
        assert!(p.is_ok(), "本机应能解析 data_dir");
    }
}
