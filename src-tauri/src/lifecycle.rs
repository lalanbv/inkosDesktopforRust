//! sidecar 进程生命周期管理：持锁存储 Child、退出时清理整组。
//!
//! 设计动机：Task 3/5 实测发现 sidecar 进程会被 init 收养成为孤儿，
//! 单纯 `Child::kill()` 无效。`supervisor::spawn` 让子进程成新进程组 leader，
//! 配合 `supervisor::kill_tree` 可整组终结。本模块把"持有 Child + 退出清理"
//! 抽到独立单元，便于：
//! 1. Tauri managed state（`Send + Sync`）跨异步任务与 RunEvent::Exit 共享；
//! 2. 单元测试无需触发真实 Tauri 事件循环即可验证清理逻辑。

use std::process::Child;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use tauri::{AppHandle, Manager};

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

// =====================================================================
// TrayController（M2a Task 5）—— 计数器独立成 BadgeCounter 便于单测。
// =====================================================================

/// 角标计数器：原子 u32，无 Tauri 依赖。
///
/// `TrayController` 持一份；也可独立用于测试。`Clone` 共享底层
/// `Arc<AtomicU32>`——多个持有者看到同一计数。
#[derive(Debug, Default, Clone)]
pub struct BadgeCounter(Arc<AtomicU32>);

impl BadgeCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 自增并返回新值。`SeqCst` 保证与 `clear` 不重排。
    pub fn inc(&self) -> u32 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn clear(&self) {
        self.0.store(0, Ordering::SeqCst);
    }

    pub fn current(&self) -> u32 {
        self.0.load(Ordering::SeqCst)
    }
}

/// 托盘控制器：`AppHandle` + `TrayIcon` + `BadgeCounter`。
///
/// 菜单固定三项：`未读:N`（不可点） / `显示窗口` / `退出`。
/// `inc_badge`/`clear_badge` 改计数后 best-effort 重建菜单（失败仅 log）。
/// `build`/`refresh` 走真实 Tauri tray API（GUI 路径），单测不可达——
/// 标注「集成验证待 GUI/真机」；计数逻辑在 BadgeCounter 上单测。
/// `Send + Sync` 满足，可作 Tauri managed state（Task 6 注册）。
pub struct TrayController {
    badge: BadgeCounter,
    app: AppHandle,
    tray: tauri::tray::TrayIcon,
}

impl TrayController {
    /// 构建托盘 + 菜单。失败 panic（GUI 启动期无降级路径）。
    pub fn build(app: &AppHandle) -> Self {
        let badge = BadgeCounter::new();
        let menu = build_menu(app, 0).expect("TrayController::build: 初始菜单构建失败");
        let tray = tauri::tray::TrayIconBuilder::with_id("main")
            .tooltip("inkosDesktop")
            .menu(&menu)
            .on_menu_event(handle_menu_event)
            .build(app)
            .expect("TrayController::build: TrayIconBuilder 失败");
        Self {
            badge,
            app: app.clone(),
            tray,
        }
    }

    /// 角标 +1，刷新菜单文案（best-effort，失败仅 log 不影响计数）。
    pub fn inc_badge(&self) {
        self.badge.inc();
        if let Err(e) = self.refresh() {
            eprintln!("[lifecycle] inc_badge: refresh 失败: {e:#}");
        }
    }

    /// 角标清零，刷新菜单文案（best-effort）。
    pub fn clear_badge(&self) {
        self.badge.clear();
        if let Err(e) = self.refresh() {
            eprintln!("[lifecycle] clear_badge: refresh 失败: {e:#}");
        }
    }

    /// 当前角标值（透传计数器）。
    pub fn badge(&self) -> u32 {
        self.badge.current()
    }

    /// best-effort 重建菜单并挂回托盘。Linux 上菜单一旦设置无法替换——
    /// 错误以 log 形式上报，不让计数操作抛错。
    fn refresh(&self) -> anyhow::Result<()> {
        let menu = build_menu(&self.app, self.badge.current())?;
        self.tray
            .set_menu(Some(menu))
            .context("TrayIcon::set_menu 失败")?;
        Ok(())
    }
}

/// 构建菜单：`未读:N`（disabled） / `显示窗口` / `退出`。
fn build_menu(app: &AppHandle, badge: u32) -> anyhow::Result<tauri::menu::Menu<tauri::Wry>> {
    let badge_label = if badge > 0 {
        format!("未读: {badge}")
    } else {
        "未读: 0".to_owned()
    };
    // badge 项 disabled=true 表示灰显不可点（纯展示）；show/quit enabled。
    let badge_item = tauri::menu::MenuItem::new(app, &badge_label, false, None::<&str>)
        .context("badge MenuItem 构建失败")?;
    let show_item = tauri::menu::MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)
        .context("show MenuItem 构建失败")?;
    let quit_item = tauri::menu::MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
        .context("quit MenuItem 构建失败")?;
    tauri::menu::MenuBuilder::new(app)
        .items(&[&badge_item, &show_item, &quit_item])
        .build()
        .context("MenuBuilder build 失败")
}

/// 菜单事件：show → window.show + set_focus；quit → app.exit(0)。
/// quit 复用 main.rs RunEvent::Exit 钩子做 cleanup_sidecar + loopback release。
///
/// quit 路径会先 set `ExitingFlag`（managed state），让 `on_window_event` 的
/// CloseRequested 知道这是退出意图、不要 prevent_close 拦截（参见 main.rs）。
/// 若未注册 `ExitingFlag`（例如单测路径），按 `app.exit(0)` 直走，行为同前。
fn handle_menu_event(app: &AppHandle, ev: tauri::menu::MenuEvent) {
    match ev.id().as_ref() {
        "show" => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        "quit" => {
            // 标记"正在退出"，让 CloseRequested 处理器放行；然后触发退出。
            if let Some(flag) = app.try_state::<ExitingFlag>() {
                flag.set();
            }
            app.exit(0);
        }
        _ => {}
    }
}

/// 编译期断言：TrayController 满足 Send + Sync（Tauri managed state 契约）。
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TrayController>();
};

// =====================================================================
// ExitingFlag（M2a Task 6）—— 区分"用户点 X 关窗"与"主动退出"
// =====================================================================
//
// Tauri 2 关闭语义：用户点窗口 X 触发 `WindowEvent::CloseRequested`，
// `api.prevent_close()` 可拦截。但托盘"退出"菜单 / 信号钩子调用 `app.exit(0)`
// 时希望直接退出，不被 `prevent_close` 拦在窗口层。
// 解决：quit 菜单与信号 cleanup 先 set 此 flag，CloseRequested 处理器读到 true
// 则放行（不 prevent_close）；否则视为"用户想最小化到托盘"，prevent_close + hide。

/// 退出意图标志：`Arc<AtomicBool>` 包装，作为 Tauri managed state 注册。
/// `Send + Sync`，多线程（菜单事件 / 信号钩子 / 窗口事件）共享读。
#[derive(Debug, Default, Clone)]
pub struct ExitingFlag(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl ExitingFlag {
    pub fn new() -> Self {
        Self::default()
    }

    /// 标记退出意图（SeqCst 保证跨线程可见）。
    pub fn set(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// 是否有退出意图。
    pub fn is_set(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// 编译期断言：ExitingFlag 满足 Send + Sync。
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ExitingFlag>();
};

// =====================================================================
// install_signal_hooks（M2a Task 5）
// =====================================================================
//
// SIGINT（Ctrl+C，跨平台）+ SIGTERM（仅 Unix）→ cleanup() → exit(0)。
// 信号送达本身难单测，单测仅验证 cleanup 闭包签名/可调用/Arc 共享；
// 信号 → cleanup → exit 链路标注「集成验证待 GUI/真机」。
// cleanup 应由调用方保证幂等（`cleanup_sidecar` 已是）。

/// 安装 SIGINT/SIGTERM 钩子：收到信号 → `cleanup()` → `exit(0)`。
/// 在 Tauri `setup` 中调用；**不阻塞**，内部 `tokio::spawn` 立即返回。
pub fn install_signal_hooks(cleanup: Arc<dyn Fn() + Send + Sync>) {
    // SIGINT（Ctrl+C）—— 跨平台。
    // ctrl_c() 返回 Err 表示注册失败（runtime 已 shutdown 等），此时
    // 无监听可走，spawn 任务静默结束。
    let c_int = Arc::clone(&cleanup);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("[lifecycle] 收到 SIGINT，触发 cleanup");
            c_int();
            std::process::exit(0);
        }
    });

    // SIGTERM —— 仅 Unix。Windows 无 SIGTERM 概念。
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let c_term = Arc::clone(&cleanup);
        tokio::spawn(async move {
            match signal(SignalKind::terminate()) {
                Ok(mut s) => {
                    s.recv().await;
                    eprintln!("[lifecycle] 收到 SIGTERM，触发 cleanup");
                    c_term();
                    std::process::exit(0);
                }
                Err(e) => {
                    // 注册失败不致命——SIGINT 路径仍在工作。
                    eprintln!("[lifecycle] 注册 SIGTERM 监听失败: {e}");
                }
            }
        });
    }

    // cleanup 已被 Arc::clone 共享；显式 drop 文档性表达，无运行期效果。
    drop(cleanup);
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

    // --- TrayController::BadgeCounter 单测（纯逻辑，无 Tauri 依赖）---
    // TrayController::build/refresh 走真实 Tauri tray API（GUI 路径），
    // 单测不可达——标注「集成验证待 GUI/真机」。

    #[test]
    fn badge_counter_new_starts_at_zero() {
        assert_eq!(BadgeCounter::new().current(), 0);
    }

    #[test]
    fn badge_counter_inc_increments_and_returns_new_value() {
        let b = BadgeCounter::new();
        assert_eq!(b.inc(), 1);
        assert_eq!(b.inc(), 2);
        assert_eq!(b.inc(), 3);
        assert_eq!(b.current(), 3);
    }

    #[test]
    fn badge_counter_clear_resets_to_zero() {
        let b = BadgeCounter::new();
        b.inc();
        b.inc();
        assert_eq!(b.current(), 2);
        b.clear();
        assert_eq!(b.current(), 0);
    }

    #[test]
    fn badge_counter_inc_after_clear_starts_from_one() {
        let b = BadgeCounter::new();
        b.inc();
        b.inc();
        b.clear();
        assert_eq!(b.inc(), 1);
    }

    #[test]
    fn badge_counter_clone_shares_state() {
        let a = BadgeCounter::new();
        let b = a.clone();
        a.inc();
        assert_eq!(b.current(), 1, "clone 应共享底层 Arc");
        b.inc();
        assert_eq!(a.current(), 2, "原持有者也应看到 clone 的 inc");
    }

    // --- install_signal_hooks：cleanup 闭包契约单测 ---
    // 信号送达本身难单测（需真实 OS 信号 + 进程级 handler 注册；tokio::spawn
    // 也需要 runtime）。这里仅验证 cleanup 闭包签名正确、可多次调用、Arc 共享。
    // 信号 → cleanup → exit 链路标注「集成验证待 GUI/真机」。

    #[test]
    fn cleanup_closure_callable_multiple_times() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let count = Arc::new(AtomicU32::new(0));
        let count_clone = Arc::clone(&count);
        let cleanup: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        cleanup();
        cleanup();
        cleanup();

        assert_eq!(count.load(Ordering::SeqCst), 3);
    }

    /// 验证 Arc::clone 路径可编译、可调用（验证 Send + Sync 契约）。
    #[test]
    fn cleanup_closure_arc_clone_compiles_and_calls() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let count = Arc::new(AtomicU32::new(0));
        let count_clone = Arc::clone(&count);
        let cleanup: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        let cleanup2 = Arc::clone(&cleanup);
        cleanup2();
        cleanup();

        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    // --- ExitingFlag 单测（纯逻辑，无 Tauri 依赖）---

    #[test]
    fn exiting_flag_new_is_false() {
        assert!(!ExitingFlag::new().is_set());
    }

    #[test]
    fn exiting_flag_set_marks_intent() {
        let flag = ExitingFlag::new();
        assert!(!flag.is_set());
        flag.set();
        assert!(flag.is_set());
    }

    #[test]
    fn exiting_flag_clone_shares_state() {
        let flag = ExitingFlag::new();
        let clone = flag.clone();
        flag.set();
        assert!(
            clone.is_set(),
            "clone 应共享底层 Arc，set 在原实例上对 clone 可见"
        );
    }
}
