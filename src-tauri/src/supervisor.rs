use crate::config::CLI_ENTRY_REL;
#[cfg(test)]
use crate::config::DEFAULT_STUDIO_PORT; // Task 5 spawn 默认端口将引用此常量
use crate::paths::PathResolver;
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::PathBuf;

// process_group 来自 CommandExt：Unix 在 std::os::unix::process，Windows 在 std::os::windows::process。
// 我们只在 Unix 用它（Windows 走 taskkill /T，不需要新进程组）；但 cfg(unix) 引入即可覆盖。
#[cfg(unix)]
use std::os::unix::process::CommandExt;
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

/// 拉起 sidecar 子进程。
///
/// 关键设计：`process_group(0)` 让子进程成为新进程组 leader（pgid=child pid）。
/// 这是为了解决 Task 3 实测发现：inkos CLI（`node packages/cli/dist/index.js studio`）
/// 会 spawn 一个 tsx 孙子进程作为真正的 HTTP 服务；CLI 父进程退出后 tsx 被 init
/// （PID 1）收养，`child.kill()` 只杀 CLI 父进程、留下孤儿 tsx 继续占端口。
/// 新进程组让我们后续能用 [`kill_tree`] 一次性杀掉整组（CLI + tsx）。
///
/// `process_group` 在 Rust 1.64+ 于 Unix/Windows 均稳定可用。
pub fn spawn(spec: &LaunchSpec) -> anyhow::Result<std::process::Child> {
    std::process::Command::new(&spec.program)
        .args(&spec.args)
        .envs(spec.env.iter())
        .current_dir(&spec.cwd)
        .process_group(0)
        .spawn()
        .map_err(Into::into)
}

/// 杀掉 [`spawn`] 拉起的整棵进程树。
///
/// 必须配合 `spawn`（用了 `process_group(0)`）使用——`child.id()` 同时也是进程组 ID。
///
/// - Unix：`libc::kill(-pgid, SIGTERM)` 向整个进程组发 SIGTERM；ESESCH（进程已退出）
///   视为成功（幂等）。SIGTERM 而非 SIGKILL，给 tsx 一个清理端口的机会。
/// - Windows：`taskkill /PID <pid> /T /F`，`/T` 杀整树。
///
/// 返回 `Ok(())` 表示已尽力发送信号；不保证子进程已退出（调用方如需确认应额外等 wait）。
pub fn kill_tree(child: &std::process::Child) -> anyhow::Result<()> {
    let pid = child.id();
    #[cfg(unix)]
    {
        // 负号 = 向整个进程组发送；pgid = pid（child 是 leader）。
        // ESRCH = 进程组已不存在（child 已被 reap），属正常情况。
        let rc = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGTERM) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ESRCH) {
                anyhow::bail!("kill(-pgid={pid}) 失败: {err}");
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .map_err(|e| anyhow::anyhow!("taskkill 启动失败: {e}"))?;
        if !status.success() {
            anyhow::bail!("taskkill /PID {pid} /T /F 失败 (exit={status})");
        }
        Ok(())
    }
}

/// 轮询 `http://127.0.0.1:{port}/` 直到返回 2xx 或超时。
///
/// 单次请求 timeout=2s，避免 reqwest 默认行为把整个 `timeout` 预算耗在一个连不上的端口。
/// 间隔由 [`crate::config::HEALTH_PROBE_INTERVAL`] 决定（默认 200ms）。
/// 任何 reqwest 错误都按"未就绪"处理（连接拒绝、TLS 失败、解析失败等），返回 false 继续轮询。
pub async fn health_probe(port: u16, timeout: std::time::Duration) -> bool {
    let url = format!("http://127.0.0.1:{port}/");
    let deadline = std::time::Instant::now() + timeout;
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        // ClientBuilder 失败几乎只在 TLS 后端不可用时发生；按"探测不可用"处理。
        Err(_) => return false,
    };
    while std::time::Instant::now() < deadline {
        let ok = client
            .get(&url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ok {
            return true;
        }
        tokio::time::sleep(crate::config::HEALTH_PROBE_INTERVAL).await;
    }
    false
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
