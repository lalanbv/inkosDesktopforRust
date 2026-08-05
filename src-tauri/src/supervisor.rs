use crate::config::CLI_ENTRY_REL;
#[cfg(test)]
use crate::config::DEFAULT_STUDIO_PORT; // Task 5 spawn 默认端口将引用此常量
use crate::paths::PathResolver;
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::PathBuf;

/// 启动规格：描述如何拉起 inkos studio sidecar 进程。
///
/// 构造后不可变；调用方应视为只读快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: PathBuf,
    pub port: u16,
}

/// 从 `start` 起递增找首个可绑定的端口（含 `start`）。
///
/// 探测区间为 `[start, start+1000)`；探测方式为 `TcpListener::bind`，
/// 成功即说明端口当前空闲。返回 `Some(port)` 表示该端口在探测瞬间可用，
/// 不保证后续仍可用（TOCTOU 由 spawn 侧的失败处理兜底，见 Task 5）。
pub fn pick_free_port(start: u16) -> Option<u16> {
    (start..start.saturating_add(1000))
        .find(|p| TcpListener::bind(("127.0.0.1", *p)).is_ok())
}

/// 根据路径解析器与端口构造 `LaunchSpec`。
///
/// - `program` = `node_bin`
/// - `args` = `[<submodule>/packages/cli/dist/index.js, "studio", "--port", <port>]`
/// - `env` 注入 `INKOS_PROJECT_ROOT` 与 `INKOS_STUDIO_PORT`
/// - `cwd` = `project_root`
///
/// 本函数为纯逻辑：不执行 spawn、不做 I/O，所有副作用由调用方承担。
pub fn build_launch<R: PathResolver>(paths: &R, port: u16, node_bin: &str) -> LaunchSpec {
    let cli_entry = paths.submodule_root().join(CLI_ENTRY_REL);
    let mut env = HashMap::new();
    env.insert(
        "INKOS_PROJECT_ROOT".to_string(),
        paths.project_root().to_string_lossy().into_owned(),
    );
    env.insert("INKOS_STUDIO_PORT".to_string(), port.to_string());
    LaunchSpec {
        program: node_bin.to_string(),
        args: vec![
            cli_entry.to_string_lossy().into_owned(),
            "studio".to_string(),
            "--port".to_string(),
            port.to_string(),
        ],
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
        sub: PathBuf,
    }
    impl PathResolver for DummyPaths {
        fn project_root(&self) -> &std::path::Path {
            &self.proj
        }
        fn submodule_root(&self) -> &std::path::Path {
            &self.sub
        }
        fn log_dir(&self) -> PathBuf {
            self.proj.join("log")
        }
    }

    #[test]
    fn pick_free_port_returns_bindable_port() {
        let p = pick_free_port(DEFAULT_STUDIO_PORT).expect("应找到空闲端口");
        // 返回的端口确实可绑定（再次 bind 成功说明未被占）
        assert!(TcpListener::bind(("127.0.0.1", p)).is_ok());
    }

    #[test]
    fn build_launch_sets_project_root_and_port_env() {
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp/proj"),
            sub: PathBuf::from("/tmp/inkos"),
        };
        let spec = build_launch(&paths, 4567, "/usr/bin/node");
        assert_eq!(spec.env.get("INKOS_PROJECT_ROOT").unwrap(), "/tmp/proj");
        assert_eq!(spec.env.get("INKOS_STUDIO_PORT").unwrap(), "4567");
    }

    #[test]
    fn build_launch_invokes_studio_with_port() {
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp/proj"),
            sub: PathBuf::from("/tmp/inkos"),
        };
        let spec = build_launch(&paths, 4567, "/usr/bin/node");
        assert_eq!(spec.program, "/usr/bin/node");
        assert!(spec.args[0].ends_with("packages/cli/dist/index.js"));
        assert_eq!(&spec.args[1..], &["studio", "--port", "4567"]);
    }

    #[test]
    fn build_launch_cwd_is_project_root() {
        // 补充测试：cwd 应等于 project_root，覆盖 LaunchSpec.cwd 字段
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp/proj"),
            sub: PathBuf::from("/tmp/inkos"),
        };
        let spec = build_launch(&paths, 4567, "/usr/bin/node");
        assert_eq!(spec.cwd, PathBuf::from("/tmp/proj"));
    }

    #[test]
    fn build_launch_port_field_matches_input() {
        // 补充测试：LaunchSpec.port 应等于传入端口
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp/proj"),
            sub: PathBuf::from("/tmp/inkos"),
        };
        let spec = build_launch(&paths, 7654, "/usr/bin/node");
        assert_eq!(spec.port, 7654);
    }

    #[test]
    fn launch_spec_is_cloneable_and_equal() {
        // 补充测试：LaunchSpec 派生 Clone/PartialEq/Eq，符合不可变快照契约
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp/proj"),
            sub: PathBuf::from("/tmp/inkos"),
        };
        let spec = build_launch(&paths, 4567, "/usr/bin/node");
        let cloned = spec.clone();
        assert_eq!(spec, cloned);
    }
}
