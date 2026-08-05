//! sidecar 进程生命周期管理：持锁存储 Child、退出时清理整组。
//!
//! 设计动机：Task 3/5 实测发现 sidecar 进程会被 init 收养成为孤儿，
//! 单纯 `Child::kill()` 无效。`supervisor::spawn` 让子进程成新进程组 leader，
//! 配合 `supervisor::kill_tree` 可整组终结。本模块把"持有 Child + 退出清理"
//! 抽到独立单元，便于：
//! 1. Tauri managed state（`Send + Sync`）跨异步任务与 RunEvent::Exit 共享；
//! 2. 单元测试无需触发真实 Tauri 事件循环即可验证清理逻辑。

use std::process::Child;
use std::sync::Mutex;

use crate::supervisor;

/// Tauri managed state：持锁保存当前 sidecar 的 Child（同一时刻至多一个）。
///
/// 用 `Mutex` 而非 `RwLock`：仅在新 sidecar 启动（setup）和退出清理（Exit）时
/// 各写一次，没有并发读；RwLock 的读多写少优势在此无用，反而增加开销。
///
/// 字段为 `pub` 让 `main.rs` 在 RunEvent::Exit 时直接 `state.0.lock()`，
/// 避免 wrapper 方法把生命周期签名搞复杂（清理路径上要消费 Child）。
#[derive(Default)]
pub struct SidecarState(pub Mutex<Option<Child>>);

impl SidecarState {
    /// 创建空状态（无 sidecar）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 存入新 child；覆盖此前未清理的 child（调用方应确保已先 `take` 并清理）。
    pub fn insert(&self, child: Child) {
        let mut guard = self.0.lock().expect("SidecarState mutex 中毒");
        *guard = Some(child);
    }

    /// 取出当前 child（若存在）；此后 state 为空。
    pub fn take(&self) -> Option<Child> {
        let mut guard = self.0.lock().expect("SidecarState mutex 中毒");
        guard.take()
    }
}

/// 退出清理：发 SIGTERM/taskkill 整组，再 wait reap，避免僵尸。
///
/// 设计为幂等：传入 `None` 直接返回（无 sidecar 或已清理）。
/// `kill_tree` 失败不传播错误——退出路径上不容失败，只打 stderr 日志让进程退出继续走。
/// `wait` 即便 kill_tree 失败也尝试，最大化 reap 概率。
pub fn cleanup_sidecar(child: Option<Child>) {
    let Some(mut c) = child else {
        return;
    };
    if let Err(e) = supervisor::kill_tree(&c) {
        eprintln!("[lifecycle] cleanup_sidecar: kill_tree 失败: {e}");
    }
    let _ = c.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::PathResolver;
    use crate::supervisor::{build_launch, spawn};
    use std::path::PathBuf;
    use std::time::Duration;

    /// 幂等性：`None` 不应 panic。
    #[test]
    fn cleanup_sidecar_none_is_noop() {
        cleanup_sidecar(None);
    }

    /// 新建状态 `take` 应返回 `None`。
    #[test]
    fn sidecar_state_new_is_empty() {
        let state = SidecarState::new();
        assert!(state.take().is_none());
    }

    /// 用 supervisor::spawn 起一个真实新进程组子进程（sleep 30），
    /// 验证 SidecarState::insert + take 往返保留 pid，且 cleanup_sidecar 能整组杀掉。
    ///
    /// 用 supervisor::spawn（而非裸 std::process::Command）是为了让 child 成新进程组
    /// leader，否则 kill_tree 的 `kill(-pgid, SIGTERM)` 会发到测试进程自己的组里。
    #[cfg(unix)]
    #[test]
    fn sidecar_state_roundtrip_and_cleanup_kills_real_child() {
        // build_launch 默认 program=node、args=[cli.js, studio, --port]；为单测安全，
        // 覆盖 program 与 args 为 "sleep 30"，但保留 supervisor::spawn 的 process_group(0) 行为。
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp"),
            sub: PathBuf::from("/tmp"),
        };
        let mut spec = build_launch(&paths, 0, "sleep");
        spec.args = vec!["30".to_string()];

        let child = spawn(&spec).expect("spawn sleep 失败");
        let pid = child.id();

        let state = SidecarState::new();
        state.insert(child);
        let taken = state.take().expect("insert 后 take 应得 child");
        assert_eq!(taken.id(), pid, "pid 应保持一致");
        assert!(state.take().is_none(), "再 take 应为空");

        cleanup_sidecar(Some(taken));
        // cleanup_sidecar 内 wait() 阻塞到 child 退出；之后给 OS 一点时间回收
        std::thread::sleep(Duration::from_millis(50));
        // kill -0 验证进程已不存在：返回 -1 表示已退出
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        assert_eq!(rc, -1, "kill -0 应失败 (rc=-1) 说明进程已退出");
    }

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
}
