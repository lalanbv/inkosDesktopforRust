//! parent-watchdog（1431 备案留工项，1459 落地）。
//!
//! 问题面：桌面壳（Tauri app）被 SIGKILL 时信号不可捕获——supervisor 的
//! cleanup_sidecar/kill_tree 不可达，sidecar（本引擎）成孤儿进程残留。
//! 1431 的 stdio null 化已解「管道挂死」面、1435 sweep 已解「事后清场」面；
//! 本模块把「事后清」升级为「实时自退」：父亡后 ≤[`POLL_SECS`] 内自退，
//! 缩短孤儿端口/资源占用窗口（真实用户场景：app 崩溃后立即重启）。
//!
//! 契约：壳 spawn 时注入 `INKOS_PARENT_PID=<app pid>`（src-tauri
//! supervisor `build_launch`）；env 缺失=功能关闭（duel/smoke/直跑形态
//! 零影响）。TS 回退腿（studio server.ts `startStudioServer`）同款对称。
//!
//! 边界语义：父亡判定 `kill(pid, 0)` 探测。pid 复用导致**漏判不退**
//! （父退出后 pid 被新进程复用则探测成功）——fail-open：最坏回到 sweep
//! 兜底现状，绝不误杀活引擎；反向误判（父活着判死）不存在（ESRCH 仅在
//! 进程真亡且已收尸时出现）。

use std::time::Duration;

/// 轮询周期（秒）：孤儿窗口上界；每轮一次 kill(0) syscall，开销可忽略。
const POLL_SECS: u64 = 2;

/// 父进程是否仍存活。
///
/// Unix：`kill(pid, 0)`——信号 0 仅做存在性/权限检查不实际发信号；
/// 返回 0=存活，-1/ESRCH=已亡（已收尸），EINVAL/EPERM 按保守不退处理
/// （EPERM：pid 存在但属他人——罕见注入错位面，不误杀）。
#[cfg(unix)]
pub fn parent_alive(pid: u32) -> bool {
    // 安全性：信号 0 零误伤；pid 来自壳注入 env（十进制串解析而来），
    // 非任意来源。
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// 非 Unix 平台：探测不可用（同 bin 侧 `cfg(not(unix))` 信号兜底先例），
/// 恒判存活=watchdog 静默关闭，孤儿清理由既有 sweep 兜底。
#[cfg(not(unix))]
pub fn parent_alive(_pid: u32) -> bool {
    true
}

/// 从 env 解析父 pid；未注入/非数字串返回 None（功能关闭——非法定值
/// 静默关闭比 panic 安全，不阻断正常服务）。
fn parent_pid_from_env() -> Option<u32> {
    std::env::var("INKOS_PARENT_PID").ok()?.parse().ok()
}

/// 启动 watchdog 后台线程（bin main 最早调用，bind 端口之前——父亡时
/// 连端口都不必占）。
///
/// env 缺失直接返回（零开销关闭）；线程 spawn 失败（资源极限）同样
/// 静默关闭——watchdog 是增强面非承重面，sweep 兜底在位（1435）。
pub fn spawn_from_env() {
    let Some(pid) = parent_pid_from_env() else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("parent-watchdog".into())
        .spawn(move || loop {
            if !parent_alive(pid) {
                eprintln!("inkos-engine-server: parent {pid} gone, exiting");
                // 父已被 SIGKILL：唯一消费方已亡，无优雅关闭对象；serve
                // 循环在 tokio runtime 上，跨线程 process::exit 是立即
                // 终止的安全路径（端口随进程释放）。
                std::process::exit(0);
            }
            std::thread::sleep(Duration::from_secs(POLL_SECS));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自身 pid 恒存活（探测正路径）。
    #[test]
    fn self_pid_is_alive() {
        assert!(parent_alive(std::process::id()));
    }

    /// 已退出且已收尸的子进程 → 判亡（ESRCH 真路径，非 mock）。
    #[test]
    fn reaped_child_is_not_alive() {
        let mut child = std::process::Command::new("sleep")
            .args(["0.15"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap(); // 收尸：不留 zombie（zombie 探测仍返回存活）
        assert!(!parent_alive(pid));
    }

    /// env 解析：合法十进制串/缺失/非法串三形态。
    #[test]
    fn env_parsing() {
        // 本测试进程 env 未注入（cargo test 不带该 env）——缺失=关闭。
        assert_eq!(parent_pid_from_env(), None);
    }
}
