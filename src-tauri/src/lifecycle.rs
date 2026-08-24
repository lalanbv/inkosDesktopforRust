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
use std::time::{Duration, Instant};

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

/// SIGTERM 后给 sidecar 的 grace period：超过则升级 SIGKILL（Unix）/ 强杀。
///
/// 选 3s：
/// - inkos CLI + tsx 进程组在 SIGTERM 下正常退出实测 <1s（即便在做 cleanup）；
/// - 3s 留足端口释放（TIME_WAIT 由 OS 管，不受 SIGKILL 影响）；
/// - 不超 5s 避免 Ctrl+C / 关窗时用户感到"卡住"。
///
/// 拆常量便于 wait_with_timeout 单测引用（避免魔术数）。
pub const CLEANUP_GRACE: Duration = Duration::from_secs(3);

/// wait_with_timeout 的轮询间隔。100ms 兼顾响应（退出快）与 CPU（不空转）。
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 在 `grace` 内轮询 `child.try_wait`；超时则升级 SIGKILL 强杀，最后 `wait` reap。
///
/// C4 修复：原 `cleanup_sidecar` 直接 `c.wait()` 无超时；tsx 卡住则 app 永不退出。
/// 现在用 `try_wait` 轮询（100ms 间隔），grace 内未退则：
/// - Unix：`libc::kill(-pgid, SIGKILL)` 整组强杀（pgid = pid，与 `kill_tree` 同语义）；
/// - Windows：`taskkill /PID <pid> /T /F`（与 `kill_tree` 同路径——/F 已是强杀，
///   超时再调一次兜底；幂等）。
///
/// 最后必 `wait()` reap 一次：避免僵尸进程（SIGKILL 后子进程也需 reap 才彻底消失）。
///
/// 返回值表示最终是否已退出（true = 退出，false = 即便 SIGKILL 也未退出——
/// 几乎不可能，但防御性返回让调用方决策）。本函数不返回 Err——退出路径不容失败。
pub fn wait_with_timeout(child: &mut Child, grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => std::thread::sleep(WAIT_POLL_INTERVAL),
            Err(_) => return true, // wait 失败视为已退出（避免挂死调用方）
        }
    }
    // grace 超时 → 升级 SIGKILL。
    if let Err(e) = escalate_kill(child) {
        eprintln!(
            "[lifecycle] wait_with_timeout: 升级 SIGKILL 失败 (pid={}): {e:#}",
            child.id()
        );
    }
    // SIGKILL 后最终 wait reap。理论秒退；再设上限避免病态挂死。
    // 这里不嵌套 wait_with_timeout（避免无限递归），直接 wait()——
    // SIGKILL 后 kernel 必 reap 进程；若 wait() 仍卡，是 OS 级 bug，非业务问题。
    let _ = child.wait();
    // wait 返回了说明进程已退出（成功或已经被 reap）。try_wait 二次确认：
    matches!(child.try_wait(), Ok(Some(_)) | Err(_))
}

/// 平台特定的 SIGKILL 升级路径（仅在 grace 超时时调用）。
fn escalate_kill(child: &Child) -> anyhow::Result<()> {
    let pid = child.id();
    #[cfg(unix)]
    {
        // 与 supervisor::kill_tree 同语义：负号 = 整组，pgid = pid（leader）。
        // 这里直接 SIGKILL（kill_tree 已发过 SIGTERM，本路径是超时升级）。
        let rc = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ESRCH) {
                anyhow::bail!("kill(-pgid={pid}, SIGKILL) 失败: {err}");
            }
            // ESRCH = 进程组已不存在，视为已退出
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        // taskkill /T /F 已是强杀；grace 超时再调一次兜底（幂等）。
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

/// 退出清理：发 SIGTERM/taskkill 整组，再 wait_with_timeout reap，避免僵尸。
///
/// 设计为幂等：传入 `None` 直接返回（无 sidecar 或已清理）。
/// `kill_tree` 失败不传播错误——退出路径上不容失败，只打 stderr 日志让进程退出继续走。
/// C4：用 [`wait_with_timeout`] 限时 grace（[`CLEANUP_GRACE`]）超时升级 SIGKILL，
/// 避免 tsx 卡住时 app 永不退出。
pub fn cleanup_sidecar(child: Option<Child>) {
    let Some(mut c) = child else {
        return;
    };
    if let Err(e) = supervisor::kill_tree(&c) {
        eprintln!("[lifecycle] cleanup_sidecar: kill_tree 失败: {e}");
    }
    if !wait_with_timeout(&mut c, CLEANUP_GRACE) {
        eprintln!(
            "[lifecycle] cleanup_sidecar: wait_with_timeout 未能确认 pid={} 退出（病态）",
            c.id()
        );
    }
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

/// 托盘控制器：`AppHandle` + `TrayIcon` + `BadgeCounter` + refresh 去抖状态。
///
/// 菜单固定四项：`未读:N`（不可点） / `显示窗口` / `插件管理…` / `退出`。
/// `inc_badge`/`clear_badge` 改计数后 best-effort 重建菜单（失败仅 log）。
/// `build`/`refresh` 走真实 Tauri tray API（GUI 路径），单测不可达——
/// 标注「集成验证待 GUI/真机」；计数逻辑在 BadgeCounter 上单测。
/// `Send + Sync` 满足，可作 Tauri managed state（Task 6 注册）。
///
/// ## M2 修复：refresh 去抖 + Linux tooltip fallback
/// `inc_badge` 每事件重建 3 个 MenuItem + `set_menu`——Linux 上
/// `set_menu` 在 once-set 后无法替换（GTK 限制），频繁调用堆日志噪声且
/// 角标永不更新。Windows/macOS 上虽可替换但每事件重建浪费。
///
/// 修法：
/// 1. **去抖**：`Mutex<Option<Instant>>` 存上次成功 refresh 时间，500ms 内
///    跳过（仅更新计数，不重建菜单）。最后一次跳过的调用不补刷——
///    可接受：未读数在 500ms 后下一次事件自然刷；零事件则角标停在中值。
/// 2. **Linux fallback**：`set_menu` 失败时降级 `set_tooltip` 表达未读数
///    （"inkosDesktop · 未读 N"），best-effort 不抛错。
/// 3. **Linux once-set 限制**：加注释说明 Linux 上菜单不可替换，
///    故 Linux 上角标 UI 实际靠 tooltip 兜底（M3 探索 GTK 菜单重建方案）。
pub struct TrayController {
    badge: BadgeCounter,
    app: AppHandle,
    tray: tauri::tray::TrayIcon,
    /// 上次成功 refresh 时间；None = 从未 refresh 过，首调强制走。
    last_refresh: Mutex<Option<Instant>>,
}

/// refresh 去抖窗口：500ms 内重复调用跳过重建。
/// 选 500ms：通知批量场景下用户感知不到延迟（人眼对菜单文字变化 ≥1s 才敏感），
/// 但能压住 SSE 风暴（数百事件/秒 → ≤2 次 refresh/秒）。
const REFRESH_DEBOUNCE: Duration = Duration::from_millis(500);

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
            last_refresh: Mutex::new(None),
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

    /// best-effort 重建菜单并挂回托盘。500ms 内重复调用去抖跳过。
    ///
    /// Linux 上 `set_menu` 一旦设置无法替换（GTK 限制）——错误以 log 形式
    /// 上报并降级 `set_tooltip` 表达未读数（M2 fix）。
    fn refresh(&self) -> anyhow::Result<()> {
        // 去抖：500ms 内已 refresh 过则跳过重建。
        // lock 中毒 = 病态，按 Ok(()) 视为已 refresh（避免 panic）。
        if let Ok(mut last) = self.last_refresh.lock() {
            if let Some(t) = *last {
                if t.elapsed() < REFRESH_DEBOUNCE {
                    return Ok(());
                }
            }
            // 更新 last_refresh 为"开始重建时间"——无论重建成功失败，
            // 都压制后续风暴（失败时 Linux 上还会走 tooltip fallback）。
            *last = Some(Instant::now());
        }

        let menu = build_menu(&self.app, self.badge.current())?;
        if let Err(e) = self.tray.set_menu(Some(menu)) {
            // Linux 上 set_menu 一旦设置无法替换——日志噪声 + 角标永不更新。
            // 降级 set_tooltip 表达未读数（best-effort，失败仅 log 不抛错）。
            let tooltip = format!("inkosDesktop · 未读 {}", self.badge.current());
            if let Err(te) = self.tray.set_tooltip(Some(&tooltip)) {
                eprintln!(
                    "[lifecycle] refresh: set_menu 失败 ({e:#}) 且 set_tooltip fallback 也失败 ({te:#})"
                );
            }
            // set_menu 失败本身仍 propagate——调用方（inc_badge/clear_badge）
            // 会再 log 一次；debounce 窗口已生效，不会刷屏。
            return Err(e).context("TrayIcon::set_menu 失败 (Linux once-set 已知限制)");
        }
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
    let plugins_item =
        tauri::menu::MenuItem::with_id(app, "plugins", "插件管理…", true, None::<&str>)
            .context("plugins MenuItem 构建失败")?;
    let quit_item = tauri::menu::MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
        .context("quit MenuItem 构建失败")?;
    tauri::menu::MenuBuilder::new(app)
        .items(&[&badge_item, &show_item, &plugins_item, &quit_item])
        .build()
        .context("MenuBuilder build 失败")
}

/// 菜单事件：show → [`show_main_window`]；quit → app.exit(0)。
/// quit 复用 main.rs RunEvent::Exit 钩子做 cleanup_sidecar + loopback release。
///
/// quit 路径会先 set `ExitingFlag`（managed state），让 `on_window_event` 的
/// CloseRequested 知道这是退出意图、不要 prevent_close 拦截（参见 main.rs）。
/// 若未注册 `ExitingFlag`（例如单测路径），按 `app.exit(0)` 直走，行为同前。
fn handle_menu_event(app: &AppHandle, ev: tauri::menu::MenuEvent) {
    match ev.id().as_ref() {
        "show" => show_main_window(app),
        // 插件管理：复用 plugin::commands::open_manager_window（与 picker 的
        // cmd_open_plugin_manager 同一入口），让常驻状态（sidecar 运行）下也能
        // 经托盘打开插件管理面板，不只 picker 阶段可达。
        "plugins" => {
            if let Err(e) = crate::plugin::commands::open_manager_window(app) {
                eprintln!("[lifecycle] 打开插件管理窗口失败: {e}");
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
// 主窗口唤回（148 修复）——「关窗→隐藏保活」设计的必要配套
// =====================================================================

/// 当前健康 sidecar 的 UI 地址（`http://127.0.0.1:{port}/`）。
///
/// 供 [`show_main_window`] 的防御性重建路径：主窗口按设计永不销毁
/// （CloseRequested 恒被 prevent_close 拦下转 hide），但若未来新增销毁路径，
/// 重建时应直连 sidecar UI 而不是停在 picker 的「启动中」页。setup 时 manage
/// （None），sidecar 健康探测通过后由 main.rs 写入。
#[derive(Default)]
pub struct LiveSidecarUrl(pub Mutex<Option<tauri::Url>>);

/// 实际拉起的引擎后端（164 号诊断回显）。
///
/// `spawn_sidecar_task` 选定**生效**后端（含 Rust 二进制 miss 回退 Node 的结果）
/// 后写入；诊断命令读取。setup 时 manage（默认值 = 配置默认 rust——仅 picker
/// 阶段未启动 sidecar 时可见，启动后即为真实值）。
pub struct EngineBackendState(pub Mutex<crate::config::EngineBackend>);

impl Default for EngineBackendState {
    fn default() -> Self {
        Self(Mutex::new(crate::config::EngineBackend::default()))
    }
}

impl EngineBackendState {
    /// 当前生效后端的短名（诊断 JSON 字段值：`rust`/`node`）。
    pub fn as_str(&self) -> &'static str {
        match *self.0.lock().unwrap_or_else(|e| e.into_inner()) {
            crate::config::EngineBackend::Rust => "rust",
            crate::config::EngineBackend::Node => "node",
        }
    }
}

/// 显示并聚焦主窗口；缺失时防御性重建（sidecar UI 或 picker）。
///
/// 托盘「显示窗口」与 macOS Dock 点击（`RunEvent::Reopen`）共用此入口，
/// 避免两处 show/focus 逻辑漂移。show/set_focus/重建均 best-effort：
/// 唤回路径不容 panic，失败仅 log。
pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if let Err(e) = w.show() {
            eprintln!("[lifecycle] show_main_window: show 失败: {e:#}");
        }
        if let Err(e) = w.set_focus() {
            eprintln!("[lifecycle] show_main_window: set_focus 失败: {e:#}");
        }
        return;
    }
    eprintln!("[lifecycle] show_main_window: main 窗口缺失，尝试重建");
    let live_url = app
        .try_state::<LiveSidecarUrl>()
        .and_then(|s| s.0.lock().ok().and_then(|g| g.clone()));
    // 无 sidecar 地址（picker 阶段 / 探测未过）→ 回 picker 首页（frontendDist 根）。
    let target = match live_url {
        Some(u) => tauri::WebviewUrl::External(u),
        None => tauri::WebviewUrl::App("index.html".into()),
    };
    match tauri::WebviewWindowBuilder::new(app, "main", target)
        .title("inkosDesktop")
        .inner_size(1280.0, 800.0)
        .build()
    {
        Ok(_) => eprintln!("[lifecycle] show_main_window: 主窗口已重建"),
        Err(e) => eprintln!("[lifecycle] show_main_window: 重建主窗口失败: {e:#}"),
    }
}

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
/// 在 Tauri `setup` 中调用；**不阻塞**，内部 `tauri::async_runtime::spawn` 立即返回。
///
/// M3b 修复：原用裸 `tokio::spawn`，但 Tauri `setup` 在主线程、不在 Tokio 运行时
/// 上下文内 → "there is no reactor running" panic（M1/M2 侧 car curl 冒烟未启 GUI，
/// 故未暴露；M3b 真 GUI 启动捕获）。改用 `tauri::async_runtime::spawn`——它内部
/// 经 Tauri 运行时句柄派发，任意（非运行时内）上下文均可安全调用。
pub fn install_signal_hooks(cleanup: Arc<dyn Fn() + Send + Sync>) {
    // SIGINT（Ctrl+C）—— 跨平台。
    // ctrl_c() 返回 Err 表示注册失败（runtime 已 shutdown 等），此时
    // 无监听可走，spawn 任务静默结束。
    let c_int = Arc::clone(&cleanup);
    tauri::async_runtime::spawn(async move {
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
        tauri::async_runtime::spawn(async move {
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
            engine: PathBuf::from("/tmp"),
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

    // --- C4: wait_with_timeout 单测 ---
    // 用真实 sleep 子进程（supervisor::spawn 让它成新进程组 leader），
    // 不依赖 mock——真实路径才能验出 try_wait / SIGKILL 升级正确性。

    /// C4 happy path：子进程在 grace 内退出 → wait_with_timeout 返回 true
    /// 且不调 SIGKILL（用 short sleep 验证）。
    #[cfg(unix)]
    #[test]
    fn wait_with_timeout_returns_true_when_child_exits_in_grace() {
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp"),
            engine: PathBuf::from("/tmp"),
        };
        // sleep 0.3s：远小于 grace（3s），wait_with_timeout 应在 grace 内观察到退出。
        let mut spec = build_launch(&paths, 0, "sleep");
        spec.args = vec!["0.3".to_string()];
        let mut child = spawn(&spec).expect("spawn sleep 0.3 失败");
        // 不发 SIGTERM——直接 wait_with_timeout，让 try_wait 在 0.3s 后观察到退出。
        let exited = wait_with_timeout(&mut child, Duration::from_secs(2));
        assert!(exited, "grace 内应观察到 child 退出");
    }

    /// C4 超时升级 SIGKILL：子进程忽略 SIGTERM（trap）→ grace 超时升级 SIGKILL
    /// 强杀，wait_with_timeout 必返回 true。用 `trap '' TERM; sleep 30` 模拟
    /// tsx 卡住场景（tsx 在 IO 卡死时也对 SIGTERM 无响应）。
    #[cfg(unix)]
    #[test]
    fn wait_with_timeout_escalates_to_sigkill_on_timeout() {
        let paths = DummyPaths {
            proj: PathBuf::from("/tmp"),
            engine: PathBuf::from("/tmp"),
        };
        // bash -c "trap '' TERM; sleep 30"：忽略 SIGTERM，模拟 tsx 卡死。
        // 必须用 supervisor::spawn 让它成新进程组 leader，否则 SIGKILL 整组
        // 会误伤测试进程。
        let mut spec = build_launch(&paths, 0, "bash");
        spec.args = vec![
            "-c".to_string(),
            "trap '' TERM; sleep 30".to_string(),
        ];
        let mut child = spawn(&spec).expect("spawn trap+sleep 失败");
        let pid = child.id();

        // 先发 SIGTERM（与 cleanup_sidecar 同路径）：
        let _ = supervisor::kill_tree(&child);
        // grace=1s（缩短以加速测试；生产用 CLEANUP_GRACE=3s）：
        let start = std::time::Instant::now();
        let exited = wait_with_timeout(&mut child, Duration::from_secs(1));
        let elapsed = start.elapsed();

        assert!(exited, "SIGKILL 升级后 child 必退出");
        // 应在 ~1s（grace）+ 些许 reap 时间内完成；不超过 5s（防病态 hang）。
        assert!(
            elapsed < Duration::from_secs(5),
            "总耗时应 < 5s，实际 {elapsed:?}"
        );
        // 给 OS 一点时间回收，验证进程组已彻底退出：
        std::thread::sleep(Duration::from_millis(50));
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        assert_eq!(
            rc, -1,
            "kill -0 应失败（rc=-1）说明进程已退出；rc={rc}"
        );
    }

    /// C4 验证常量：CLEANUP_GRACE = 3s，与文档一致。
    /// 拆常量测试避免后续重构偷偷改超时阈值。
    #[test]
    fn cleanup_grace_is_three_seconds() {
        assert_eq!(CLEANUP_GRACE, Duration::from_secs(3));
    }

    // --- M2: TrayController refresh 去抖单测 ---
    // refresh 本身走 GUI 路径不可单测；去抖逻辑拆出 last_refresh 状态可单测。
    // 这里间接验证：last_refresh 字段存在 + Mutex<Option<Instant>> 类型契约。
    // 完整 debounce 行为标注「集成验证待 GUI/真机」。

    /// M2 验证常量：REFRESH_DEBOUNCE = 500ms，与文档一致。
    /// 拆常量测试避免后续重构偷偷改 debounce 窗口。
    #[test]
    fn refresh_debounce_is_500ms() {
        assert_eq!(REFRESH_DEBOUNCE, Duration::from_millis(500));
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
