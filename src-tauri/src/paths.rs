use std::path::{Path, PathBuf};

pub trait PathResolver {
    fn project_root(&self) -> &Path;
    fn submodule_root(&self) -> &Path;
    fn log_dir(&self) -> PathBuf;
}

pub struct AppPaths {
    project_root: PathBuf,
    submodule_root: PathBuf,
    app_data: PathBuf,
}

impl AppPaths {
    pub fn new(project_root: PathBuf, submodule_root: PathBuf) -> anyhow::Result<Self> {
        let app_data = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("无法解析平台 data 目录"))?
            .join("inkosDesktop");
        Ok(Self { project_root, submodule_root, app_data })
    }
}

impl PathResolver for AppPaths {
    fn project_root(&self) -> &Path { &self.project_root }
    fn submodule_root(&self) -> &Path { &self.submodule_root }
    fn log_dir(&self) -> PathBuf { self.app_data.join("logs") }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_project_and_submodule_roots() {
        let p = AppPaths::new(PathBuf::from("/tmp/proj"), PathBuf::from("/tmp/inkos")).unwrap();
        assert_eq!(p.project_root(), Path::new("/tmp/proj"));
        assert_eq!(p.submodule_root(), Path::new("/tmp/inkos"));
    }
    #[test]
    fn log_dir_under_app_data() {
        let p = AppPaths::new(PathBuf::from("/tmp/proj"), PathBuf::from("/tmp/inkos")).unwrap();
        assert!(p.log_dir().ends_with("inkosDesktop/logs"));
    }
}
