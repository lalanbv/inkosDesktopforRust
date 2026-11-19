use crate::config::CLI_ENTRY_REL;
#[cfg(test)]
use crate::config::DEFAULT_STUDIO_PORT; // Task 5 spawn 默认端口将引用此常量
use crate::paths::PathResolver;
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::PathBuf;

// spawn 让子进程成新进程组 leader，跨平台分别通过 CommandExt 的不同方法实现：
// - Unix：`process_group(0)` → fork 后 setpgid(0,0)，pgid=child.pid
// - Windows：`creation_flags(CREATE_NEW_PROCESS_GROUP=0x00000200)` → 新进程组
// 两者的 trait 都叫 `CommandExt`，但分别在 `std::os::unix::process` 与
// `std::os::windows::process`，需要按平台 cfg-gate 导入；其上的方法调用也必须
// 在对应 cfg 块内（方法本身仅在 trait 导入时才存在）。
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
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
/// 成功即说明端口当前空闲。返回 `Some(port)` 表示该端口在**探测瞬间**可用，
/// **不保证后续仍可用**——这是经典的 TOCTOU（time-of-check/time-of-use）
/// 缝隙：`bind` 探测成功后 `TcpListener` 立即 drop 释放端口，到调用方
/// 真正用它（如 sidecar spawn）之间，另一并发进程可能抢先把同一端口 bind 走。
///
/// 调用方必须容忍这种竞争：
/// - 直接消费方（`main.rs` setup）在 spawn 后用 [`health_probe`] 兜底，
///   端口被抢则 sidecar 起不来、健康探测失败、UI 显式报错。
/// - 单元测试**不得**再 `TcpListener::bind` 同一端口做"复核"——并发测试
///   下两个 `pick_free_port` 几乎必然抢同一端口，使测试 flaky。
///   正确断言是「返回 `Some` 且端口落在探测区间内」。
pub fn pick_free_port(start: u16) -> Option<u16> {
    (start..start.saturating_add(1000)).find(|p| TcpListener::bind(("127.0.0.1", *p)).is_ok())
}

/// 根据路径解析器与端口构造 `LaunchSpec`。
///
/// - `program` = `node_bin`
/// - `args` = `[<launch_engine>/dist/index.js, "studio", "--port", <port>]`
///   （`launch_engine_dir` 由 [`crate::engine::resolve_engine_dir`] 在 main 决定：
///   prod=resource_dir/engine，dev=repo_root/engine；CLI_ENTRY_REL="dist/index.js"）
/// - `env` 注入 `INKOS_STUDIO_PORT`；**不**注入 `INKOS_PROJECT_ROOT`——
///   该 env 与 cwd 在本路径下**等价**（见架构 §6.1/§14 Q4：studio 服务
///   `api/index.ts` 解析链 `argv[2] ?? process.env.INKOS_PROJECT_ROOT ?? process.cwd()`，
///   三者均可指定 root，env 端确有读取，**非死代码**）。supervisor 已通过
///   `cwd=project_root`（下行）注入相同值，再叠加 env 冗余、徒增状态面，故移除。
/// - `cwd` = `project_root`（CLI `findProjectRoot()`=`process.cwd()` 作 argv[2]
///   传 studio 服务，是当前解析链的实际承重路径）。
///
/// 本函数为纯逻辑：不执行 spawn、不做 I/O，所有副作用由调用方承担。
pub fn build_launch<R: PathResolver>(paths: &R, port: u16, node_bin: &str) -> LaunchSpec {
    let cli_entry = paths.launch_engine_dir().join(CLI_ENTRY_REL);
    let mut env = HashMap::new();
    env.insert("INKOS_STUDIO_PORT".to_string(), port.to_string());
    // 1459：parent-watchdog 契约——注入 app 自身 pid，sidecar（引擎直接形态
    // 或经 CLI 的 TS 回退形态）探测父亡即自退；env 缺失=功能关闭（duel/
    // smoke/直跑零影响）。值=十进制串（两侧 parse 契约）。
    env.insert(
        "INKOS_PARENT_PID".to_string(),
        std::process::id().to_string(),
    );
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
/// 关键设计：让子进程成为新进程组 leader，跨平台一致：
/// - Unix：`process_group(0)` → fork 后 `setpgid(0, 0)`，pgid=child.pid。
/// - Windows：`creation_flags(CREATE_NEW_PROCESS_GROUP = 0x00000200)` → 子进程
///   进入新进程组（[`kill_tree`] 的 Windows 分支用 `taskkill /T /F` 按进程树杀，
///   不读 pgid；这里设进程组是为了与 Unix 语义对齐，并允许将来用 Ctrl-Break
///   信号整组发送）。
///
/// 这是为了解决 Task 3 实测发现：inkos CLI（`node packages/cli/dist/index.js studio`）
/// 会 spawn 一个 tsx 孙子进程作为真正的 HTTP 服务；CLI 父进程退出后 tsx 被 init
/// （PID 1）收养，`child.kill()` 只杀 CLI 父进程、留下孤儿 tsx 继续占端口。
/// 新进程组让我们后续能用 [`kill_tree`] 一次性杀掉整组（CLI + tsx）。
///
/// 注意：`process_group` 仅在 `std::os::unix::process::CommandExt` 提供；Windows
/// 的同名 trait 没有该方法，故不能用单个链式 `.process_group(0)` 跨平台，必须
/// 按平台分别调用对应方法。
pub fn spawn(spec: &LaunchSpec) -> anyhow::Result<std::process::Child> {
    tracing::info!(
        program = %spec.program,
        port = spec.port,
        cwd = %spec.cwd.display(),
        "spawning sidecar"
    );

    let mut cmd = std::process::Command::new(&spec.program);
    cmd.args(&spec.args)
        .envs(spec.env.iter())
        .current_dir(&spec.cwd);

    // 1431 号：stdio 三面 null 化——sidecar 与 app 终端/管道解耦。
    // ①挂死根除：app 被 SIGKILL 时 RunEvent::Exit 的 cleanup_sidecar/kill_tree
    //   不可达（signal hooks 只盖 TERM/INT），sidecar 成孤儿且**继承的
    //   stdout/stderr 写端永不关闭**——任何等待管道 EOF 的消费方
    //   （assert_cmd output()：e2e_startup 冒烟 1200s 哨兵红实证 / launchd
    //   console 收集器）被孤儿写端挂死。null 化后 app 亡故即 EOF 必达。
    // ②污染根除：sidecar 的 eprintln（引擎 4 处低频诊断行）不再混进 app
    //   stdout（生产 launchd/console 噪音面）。
    // 边界备案：独立直跑 sidecar（dev 手动 `inkos-engine-server` / CLI）不经
    // 本路径，终端观测不受影响；孤儿**进程**残留仍可能（SIGKILL 不可捕获是
    // OS 边界）——三层兜底：sweep 清理（1435 自愈闭环）+ 下次启动
    // pick_free_port 容错 + parent-watchdog env 自退（1459 落地：build_launch
    // 注入 INKOS_PARENT_PID，引擎/TS 腿探测父亡 ≤2s 自退，env 缺失=关闭）。
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    // 子进程成新进程组 leader（跨平台分置，避免方法不存在导致 Windows 编译失败）。
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        // CREATE_NEW_PROCESS_GROUP = 0x00000200；winapi 常量，避免引入 winapi crate。
        cmd.creation_flags(0x0000_0200);
    }

    let child = cmd.spawn()?;
    tracing::info!(pid = child.id(), "sidecar spawned");
    Ok(child)
}

/// 杀掉 [`spawn`] 拉起的整棵进程树。
///
/// 必须配合 `spawn`（用了 `process_group(0)`）使用——`child.id()` 同时也是进程组 ID。
///
/// - Unix：`libc::kill(-pgid, SIGTERM)` 向整个进程组发 SIGTERM；ESRCH（进程已退出）
///   视为成功（幂等）。SIGTERM 而非 SIGKILL，给 tsx 一个清理端口的机会。
/// - Windows：`taskkill /PID <pid> /T /F`，`/T` 杀整树。
///
/// 返回 `Ok(())` 表示已尽力发送信号；不保证子进程已退出（调用方如需确认应额外等 wait）。
pub fn kill_tree(child: &std::process::Child) -> anyhow::Result<()> {
    let pid = child.id();
    tracing::info!(pid, "killing sidecar tree");

    #[cfg(unix)]
    {
        // 负号 = 向整个进程组发送；pgid = pid（child 是 leader）。
        // ESRCH = 进程组已不存在（child 已被 reap），属正常情况。
        let rc = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGTERM) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ESRCH) {
                tracing::error!(pid, error = %err, "kill_tree failed");
                anyhow::bail!("kill(-pgid={pid}) 失败: {err}");
            }
        }
        tracing::info!(pid, "sidecar tree killed (SIGTERM)");
        Ok(())
    }
    #[cfg(windows)]
    {
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .map_err(|e| {
                tracing::error!(pid, error = %e, "taskkill spawn failed");
                anyhow::anyhow!("taskkill 启动失败: {e}")
            })?;
        if !status.success() {
            tracing::error!(pid, exit_code = ?status.code(), "taskkill failed");
            anyhow::bail!("taskkill /PID {pid} /T /F 失败 (exit={status})");
        }
        tracing::info!(pid, "sidecar tree killed (taskkill)");
        Ok(())
    }
}

/// 轮询 `http://127.0.0.1:{port}{probe_path}` 直到返回 2xx 或超时。
///
/// `probe_path` 按后端选择：Node sidecar = `/`（SPA 恒挂）；Rust 引擎 =
/// [`crate::engine::rustbin::HEALTH_PROBE_PATH`]（`/api/v1/health` 恒 200，
/// 不依赖静态面是否存在）。
///
/// 单次请求 timeout=2s，避免 reqwest 默认行为把整个 `timeout` 预算耗在一个连不上的端口。
/// 间隔由 [`crate::config::HEALTH_PROBE_INTERVAL`] 决定（默认 200ms）。
/// 任何 reqwest 错误都按"未就绪"处理（连接拒绝、TLS 失败、解析失败等），返回 false 继续轮询。
///
/// `.no_proxy()`：目标恒为本机回环 sidecar，绝不可经代理。reqwest 默认启用系统代理
/// （macOS 经 System Configuration 读全局代理）；当用户系统代理开启且豁免名单未含
/// `127.0.0.1`（只写 `localhost` 时不覆盖字面 IP），探活请求会被送进代理——代理无法
/// 回连本机临时端口 → 恒超时 → 启动门误判 sidecar 不健康（1572 号实证）。
pub async fn health_probe(port: u16, timeout: std::time::Duration, probe_path: &str) -> bool {
    let url = format!("http://127.0.0.1:{port}{probe_path}");
    let deadline = std::time::Instant::now() + timeout;
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        // ClientBuilder 失败几乎只在 TLS 后端不可用时发生；按"探测不可用"处理。
        Err(e) => {
            tracing::warn!("health_probe: reqwest Client 构建失败: {:#}", e);
            return false;
        }
    };

    let start = std::time::Instant::now();
    while std::time::Instant::now() < deadline {
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!(
                    "health_probe: 端口 {} 健康探测成功，耗时 {:?}",
                    port,
                    start.elapsed()
                );
                return true;
            }
            Ok(resp) => {
                tracing::debug!(
                    "health_probe: 端口 {} 返回非 2xx 状态码 {}，继续轮询",
                    port,
                    resp.status()
                );
            }
            Err(e) => {
                tracing::debug!("health_probe: 端口 {} 请求失败: {:#}，继续轮询", port, e);
            }
        }
        tokio::time::sleep(crate::config::HEALTH_PROBE_INTERVAL).await;
    }

    tracing::warn!(
        "health_probe: 端口 {} 超时（{:?}），探测失败",
        port,
        timeout
    );
    false
}

/// 1460：单发健康探测（监督 watcher 用——非启动期轮询）。
///
/// 与 [`health_probe`] 共享判活语义（2xx=活；任何错误=死），但只发一次、
/// 2s 超时——watcher 周期（`RESUPERVISE_INTERVAL`）自带节律，无需内部轮询。
///
/// `.no_proxy()` 同 [`health_probe`]：回环目标被系统代理劫持时，watcher 会把
/// 健康 sidecar 恒判为死 → 误触发 kill/重启风暴。
pub async fn probe_once(port: u16, probe_path: &str) -> bool {
    let url = format!("http://127.0.0.1:{port}{probe_path}");
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("probe_once: reqwest Client 构建失败: {:#}", e);
            return false;
        }
    };
    match client.get(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// 1460：sidecar 死亡重启决策器（监督 watcher 用，纯逻辑便于单测）。
///
/// 策略：死亡 → 固定冷却 [`RESPAWN_COOLDOWN`] 后允许重启；连续死亡达到
/// `max_attempts` 停止重启（防崩溃风暴——引擎若因环境问题必崩，反复拉起
/// 只会持续烧 CPU/刷日志，停下+通知用户才是正确行为）；健康存活满
/// [`RESPAWN_HEALTHY_RESET`]（5 分钟）后计数清零（偶发崩溃≠持续故障）。
pub struct RespawnPolicy {
    attempts: u32,
    max_attempts: u32,
}

/// 重启冷却：覆盖典型端口 TIME_WAIT/僵尸收尾窗口，且不给用户「卡死」感。
pub const RESPAWN_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(3);

/// 监督探测周期（watcher 单发探测节律）。
pub const RESUPERVISE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// 连续探测失败判定阈值：3 次（≈6s 窗口）——既容忍引擎瞬态重启/请求排队，
/// 又在用户可感知前完成死亡判定+重启。
pub const RESUPERVISE_FAILURE_THRESHOLD: u32 = 3;

/// 连续重启上限（默认）：一个 app 会话内最多自动拉起 5 次。
pub const RESPAWN_MAX_ATTEMPTS: u32 = 5;

/// 健康存活多久后重置计数：5 分钟稳定运行=上一轮崩溃是偶发。
pub const RESPAWN_HEALTHY_RESET: std::time::Duration = std::time::Duration::from_secs(300);

impl Default for RespawnPolicy {
    fn default() -> Self {
        Self::new(RESPAWN_MAX_ATTEMPTS)
    }
}

impl RespawnPolicy {
    pub fn new(max_attempts: u32) -> Self {
        Self {
            attempts: 0,
            max_attempts,
        }
    }

    /// 当前已连续重启次数（诊断/测试用）。
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// 记录一次死亡：返回 `Some(cooldown)`=冷却后允许重启；`None`=已达上限。
    pub fn record_death(&mut self) -> Option<std::time::Duration> {
        if self.attempts >= self.max_attempts {
            return None;
        }
        self.attempts += 1;
        Some(RESPAWN_COOLDOWN)
    }

    /// 健康存活上报：uptime ≥ [`RESPAWN_HEALTHY_RESET`] 时计数清零。
    pub fn note_healthy_uptime(&mut self, uptime: std::time::Duration) {
        if uptime >= RESPAWN_HEALTHY_RESET {
            self.attempts = 0;
        }
    }
}

/// 1461：监督循环决策核心（watcher 每轮一步的纯状态机）。
///
/// 1460 的 watcher 本体（探测循环→判定→换代接力）此前是自由 async 任务
/// ——仅 RespawnPolicy 有单测，循环判定零验证（#[ignore] 用例=零测试路径
/// 记忆：未验证路径潜伏缺陷全在）。抽离决策面后，main.rs 只留接线（sleep/
/// 探测/通知/重跑），全部判定逻辑在此可测。
///
/// 状态分置（1461 编写中定案的关键语义）：core 只持**代内**连续失败计数
/// （判死→换代→新 core 自然归零）；[`RespawnPolicy`]（重启计数+上限）由
/// 调用方跨代持久持有（main.rs 的 `RespawnState` 托管）——若 policy 随
/// core 换代重建，上限永不触发=崩溃风暴防护失效。
#[derive(Default)]
pub struct SupervisorCore {
    failures: u32,
}

/// 每轮探测后的决策。
#[derive(Debug, PartialEq, Eq)]
pub enum SupervisionAction {
    /// 继续监督（探测活，或失败未达 [`RESUPERVISE_FAILURE_THRESHOLD`]）。
    Continue,
    /// 判死且允许重启：冷却后重跑启动链；`attempt`=已用重启次数（通知
    /// 「第 n 次」用）。
    Respawn {
        cooldown: std::time::Duration,
        attempt: u32,
    },
    /// 判死且达重启上限：停监督，用户重启 app 是唯一恢复路径。
    Halt,
}

impl SupervisorCore {
    /// 喂入本轮探测结果，产出决策。`policy` 为跨代持久的重启策略
    ///（判死时经它决策冷却/上限——见类型文档「状态分置」）。
    pub fn step(&mut self, alive: bool, policy: &mut RespawnPolicy) -> SupervisionAction {
        if alive {
            self.failures = 0;
            return SupervisionAction::Continue;
        }
        self.failures += 1;
        if self.failures < RESUPERVISE_FAILURE_THRESHOLD {
            return SupervisionAction::Continue;
        }
        // 达阈值=判死：本代 core 使命即将结束（Respawn/Halt 均终态），
        // failures 无需清——换代即新 core。
        match policy.record_death() {
            Some(cooldown) => SupervisionAction::Respawn {
                cooldown,
                attempt: policy.attempts(),
            },
            None => SupervisionAction::Halt,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyPaths {
        proj: PathBuf,
        engine: PathBuf,
    }
    impl PathResolver for DummyPaths {
        fn project_root(&self) -> &std::path::Path {
            &self.proj
        }
        fn launch_engine_dir(&self) -> &std::path::Path {
            &self.engine
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

    fn dummy() -> DummyPaths {
        DummyPaths {
            proj: PathBuf::from("/tmp/proj"),
            engine: PathBuf::from("/tmp/src-tauri/engine"),
        }
    }

    #[test]
    fn pick_free_port_returns_bindable_port() {
        // 注意：本测试**不**再对返回端口做 `TcpListener::bind` 复核。
        // 原因：`pick_free_port` 内部 bind→drop 留下 TOCTOU 缝隙，并发 `cargo test`
        // 下另一测试的 `pick_free_port` 完全可能抢同一端口；任何"再 bind 复核"
        // 都会偶发失败（实测约 1/3）。这里只断言函数契约：
        //   1. 返回 `Some(p)` —— 区间内确有空闲端口；
        //   2. p ∈ [start, start+1000) —— 在探测区间内。
        // 函数自身的"探测瞬间可用"语义已被 doc 充分说明，调用方容忍策略
        // 见 `pick_free_port` 文档与 `main.rs` setup 的 health_probe 兜底。
        let start = DEFAULT_STUDIO_PORT;
        let p = pick_free_port(start).expect("应找到空闲端口");
        assert!(
            (start..start.saturating_add(1000)).contains(&p),
            "返回端口 {p} 应落在探测区间 [{start}, {})",
            start.saturating_add(1000)
        );
    }

    #[test]
    fn build_launch_sets_studio_port_env_only() {
        // M1 清理：INKOS_PROJECT_ROOT env 已移除——它与 cwd 在 supervisor 路径下
        // 等价（见 build_launch 文档与架构 §6.1/§14 Q4），保留 cwd=project_root 即足够。
        // 验证 env 仅含 INKOS_STUDIO_PORT+INKOS_PARENT_PID（1459 watchdog 契约）；
        // cwd 仍 = project_root 由独立测试覆盖。
        let paths = dummy();
        let spec = build_launch(&paths, 4567, "/usr/bin/node");
        assert!(
            !spec.env.contains_key("INKOS_PROJECT_ROOT"),
            "INKOS_PROJECT_ROOT env 应已移除"
        );
        assert_eq!(spec.env.get("INKOS_STUDIO_PORT").unwrap(), "4567");
        // 1459：parent-watchdog 注入本 app pid（十进制串），sidecar 探测父亡自退。
        assert_eq!(
            spec.env.get("INKOS_PARENT_PID").unwrap(),
            &std::process::id().to_string()
        );
    }

    #[test]
    fn build_launch_invokes_studio_with_port() {
        let paths = dummy();
        let spec = build_launch(&paths, 4567, "/usr/bin/node");
        assert_eq!(spec.program, "/usr/bin/node");
        // M3a：CLI 入口相对 launch_engine_dir（dist/index.js = cli bin）。
        assert!(spec.args[0].ends_with("dist/index.js"));
        assert_eq!(&spec.args[1..], &["studio", "--port", "4567"]);
    }

    #[test]
    fn build_launch_cwd_is_project_root() {
        // 补充测试：cwd 应等于 project_root，覆盖 LaunchSpec.cwd 字段
        let paths = dummy();
        let spec = build_launch(&paths, 4567, "/usr/bin/node");
        assert_eq!(spec.cwd, PathBuf::from("/tmp/proj"));
    }

    #[test]
    fn build_launch_port_field_matches_input() {
        // 补充测试：LaunchSpec.port 应等于传入端口
        let paths = dummy();
        let spec = build_launch(&paths, 7654, "/usr/bin/node");
        assert_eq!(spec.port, 7654);
    }

    /// 1460：重启决策器——冷却允许/上限拒绝/健康清零三面。
    #[test]
    fn respawn_policy_allows_until_cap_then_resets_on_health() {
        let mut p = RespawnPolicy::new(2);
        assert_eq!(p.record_death(), Some(RESPAWN_COOLDOWN), "第 1 次死亡允许");
        assert_eq!(p.attempts(), 1);
        assert_eq!(p.record_death(), Some(RESPAWN_COOLDOWN), "第 2 次死亡允许");
        assert_eq!(p.record_death(), None, "第 3 次死亡达上限拒绝");
        assert_eq!(p.attempts(), 2, "达上限后计数不再涨");
        // 健康存活不足阈值：不清零。
        p.note_healthy_uptime(RESPAWN_HEALTHY_RESET - std::time::Duration::from_secs(1));
        assert_eq!(p.attempts(), 2);
        assert_eq!(p.record_death(), None, "未清零仍拒绝");
        // 健康存活满阈值：清零恢复。
        p.note_healthy_uptime(RESPAWN_HEALTHY_RESET);
        assert_eq!(p.attempts(), 0);
        assert_eq!(p.record_death(), Some(RESPAWN_COOLDOWN), "清零后恢复允许");
    }

    /// 1461：监督状态机——瞬态失败（未达阈值）恢复全程 Continue。
    #[test]
    fn supervision_core_tolerates_transient_failures() {
        let mut core = SupervisorCore::default();
        let mut policy = RespawnPolicy::default();
        // 阈值-1 次连续失败后恢复：永远 Continue（不误判死）。
        for _ in 0..(RESUPERVISE_FAILURE_THRESHOLD - 1) {
            assert_eq!(core.step(false, &mut policy), SupervisionAction::Continue);
        }
        assert_eq!(core.step(true, &mut policy), SupervisionAction::Continue);
        // 代内计数已清：再次阈值-1 次失败仍不判死，须满阈值。
        for _ in 0..(RESUPERVISE_FAILURE_THRESHOLD - 1) {
            assert_eq!(core.step(false, &mut policy), SupervisionAction::Continue);
        }
        assert_eq!(
            core.step(false, &mut policy),
            SupervisionAction::Respawn {
                cooldown: RESPAWN_COOLDOWN,
                attempt: 1
            },
            "恢复后代内计数清零，须再满阈值才判死"
        );
    }

    /// 1461：监督状态机——跨代接力（代内 failures 随换代归零 + policy 计数
    /// 跨代持久）直到上限 Halt。锁「状态分置」语义：若 policy 误随 core
    /// 换代重建，上限永不触发=崩溃风暴防护失效。
    ///
    /// 判死时机=第 [`RESUPERVISE_FAILURE_THRESHOLD`] **次**连续失败当轮
    ///（failures==阈值即判死，非第 N+1 轮——与 1460 watcher 原语义一致）。
    #[test]
    fn supervision_core_generations_until_halt() {
        let mut policy = RespawnPolicy::new(2); // 跨代持久（生产=RespawnState）
                                                // 第 1 代：阈值-1 次容忍 → 第阈值 次判死 Respawn(attempt=1)。
        let mut gen0 = SupervisorCore::default();
        for _ in 0..(RESUPERVISE_FAILURE_THRESHOLD - 1) {
            assert_eq!(gen0.step(false, &mut policy), SupervisionAction::Continue);
        }
        assert_eq!(
            gen0.step(false, &mut policy),
            SupervisionAction::Respawn {
                cooldown: RESPAWN_COOLDOWN,
                attempt: 1
            }
        );
        // 换代：新 core（failures 归零）+ 同一 policy（attempt 累计）。
        let mut gen1 = SupervisorCore::default();
        for _ in 0..(RESUPERVISE_FAILURE_THRESHOLD - 1) {
            assert_eq!(
                gen1.step(false, &mut policy),
                SupervisionAction::Continue,
                "新代 failures 归零，须重新满阈值（复用旧 failures 会即刻误判）"
            );
        }
        assert_eq!(
            gen1.step(false, &mut policy),
            SupervisionAction::Respawn {
                cooldown: RESPAWN_COOLDOWN,
                attempt: 2
            },
            "policy 跨代持久：第 2 次判死 attempt=2"
        );
        // 第 3 代：policy 已达上限 2 → 第阈值 次判死即 Halt。
        let mut gen2 = SupervisorCore::default();
        for _ in 0..(RESUPERVISE_FAILURE_THRESHOLD - 1) {
            assert_eq!(gen2.step(false, &mut policy), SupervisionAction::Continue);
        }
        assert_eq!(gen2.step(false, &mut policy), SupervisionAction::Halt);
    }

    /// 1461：probe_once 判活语义——真 TcpListener 2xx 服务=活；无服务端口=死。
    #[tokio::test]
    async fn probe_once_http_semantics() {
        use std::io::Write as _;
        // 真 2xx 服务（手工最小 HTTP 响应，不引依赖）。
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut conn, _)) = listener.accept() {
                let mut buf = [0u8; 256];
                let _ = std::io::Read::read(&mut conn, &mut buf);
                let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
            }
        });
        assert!(probe_once(port, "/api/v1/health").await, "2xx 服务应判活");
        server.join().unwrap();

        // 无服务端口（刚释放的 ephemeral）：连接拒绝=死。
        let dead_port = {
            let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            l.local_addr().unwrap().port()
        };
        assert!(
            !probe_once(dead_port, "/api/v1/health").await,
            "无服务端口应判死"
        );
    }

    #[test]
    fn launch_spec_is_cloneable_and_equal() {
        // 补充测试：LaunchSpec 派生 Clone/PartialEq/Eq，符合不可变快照契约
        let paths = dummy();
        let spec = build_launch(&paths, 4567, "/usr/bin/node");
        let cloned = spec.clone();
        assert_eq!(spec, cloned);
    }

    /// 1462：多连接 2xx 微服务（监督整链活体的被监督体——listener 线程
    /// 循环 accept，drop 句柄即「服务死亡」，端口判死真实可感）。返回
    /// (port, 停机句柄)：drop 停机句柄=杀服务。
    struct MiniServer {
        port: u16,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
        handle: std::thread::JoinHandle<()>,
    }

    impl MiniServer {
        fn start() -> Self {
            use std::io::Write as _;
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = std::sync::Arc::clone(&shutdown);
            let handle = std::thread::spawn(move || {
                listener.set_nonblocking(true).unwrap();
                while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut conn, _)) => {
                            let mut buf = [0u8; 256];
                            let _ = std::io::Read::read(&mut conn, &mut buf);
                            let _ =
                                conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(20));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                port,
                shutdown,
                handle,
            }
        }

        fn stop(self) {
            self.shutdown
                .store(true, std::sync::atomic::Ordering::Relaxed);
            // 非阻塞 accept 轮询 20ms 一拍：join 前多等一拍保证循环退出。
            std::thread::sleep(std::time::Duration::from_millis(30));
            let _ = self.handle.join();
        }
    }

    /// 1462：监督整链活体——真探测 × 真服务死亡 × 真状态机，多代接力
    /// 直到上限 Halt（1460 接线外的全链验证：生产形态=杀 sidecar 进程，
    /// 此处以「drop 服务句柄」等价模拟——监督面感知的是端口判死，被监督
    /// 体的进程身份不是被测面）。
    #[tokio::test]
    async fn supervision_full_cycle_alive_dead_respawn_halt() {
        let mut policy = RespawnPolicy::new(2); // 上限 2：两代 Respawn→Halt
        let mut server = MiniServer::start();

        // 第 1 代：健康起跑 → 判活 Continue。
        let mut core = SupervisorCore::default();
        assert!(probe_once(server.port, "/api/v1/health").await);
        assert_eq!(core.step(true, &mut policy), SupervisionAction::Continue);

        // 杀服务 → 真探测连续判死 → 阈值当轮 Respawn(attempt=1)。
        let dead_port = server.port;
        server.stop();
        for i in 0..(RESUPERVISE_FAILURE_THRESHOLD - 1) {
            assert!(
                !probe_once(dead_port, "/api/v1/health").await,
                "服务已停，探测应连续判死（第 {} 轮）",
                i + 1
            );
            assert_eq!(core.step(false, &mut policy), SupervisionAction::Continue);
        }
        assert!(!probe_once(dead_port, "/api/v1/health").await);
        assert_eq!(
            core.step(false, &mut policy),
            SupervisionAction::Respawn {
                cooldown: RESPAWN_COOLDOWN,
                attempt: 1
            }
        );

        // 换代重启：新服务（新端口）→ 新 core → 判活。
        server = MiniServer::start();
        let mut core = SupervisorCore::default();
        assert!(probe_once(server.port, "/api/v1/health").await);
        assert_eq!(core.step(true, &mut policy), SupervisionAction::Continue);

        // 第 2 代再死 → Respawn(attempt=2)；第 3 代死 → Halt（上限 2）。
        for generation in 2..=3 {
            let port = server.port;
            server.stop();
            let mut core_g = SupervisorCore::default();
            for i in 0..RESUPERVISE_FAILURE_THRESHOLD {
                assert!(!probe_once(port, "/api/v1/health").await);
                let action = core_g.step(false, &mut policy);
                // Halt 只在第阈值次出现（前阈值-1 次为 Continue 容忍段）。
                if generation == 3 && i == RESUPERVISE_FAILURE_THRESHOLD - 1 {
                    assert_eq!(action, SupervisionAction::Halt, "上限 2 后应 Halt");
                }
            }
            if generation == 2 {
                // 本轮最后一次 step 已在循环内判死；此处只断言 policy 计数。
                assert_eq!(policy.attempts(), 2);
            }
            // Halt 后无新代：循环结束前服务句柄已 stop，无泄漏。
            if generation == 3 {
                break;
            }
            server = MiniServer::start();
            // 健康探活（新代起跑在产线由 spawn 链负责，此处只验证可恢复）。
            assert!(probe_once(server.port, "/api/v1/health").await);
        }
    }
}
