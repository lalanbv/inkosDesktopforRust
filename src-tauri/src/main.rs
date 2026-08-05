//! inkosDesktop 二进制入口：拉起 Tauri 窗口 → spawn sidecar → 健康探测 →
//! 接入 observer（SSE 路由 → 通知 + 托盘角标）+ lifecycle（托盘 + 信号钩子）。
//!
//! 设计要点：
//! - 不用 `#[tokio::main]`：Tauri 2 自带事件循环，异步用 `tauri::async_runtime::spawn`。
//! - sidecar `Child` 存进 `SidecarState`（Tauri managed state），在 `RunEvent::Exit`
//!   触发 `cleanup_sidecar`，保证关窗即清进程组（不留孤儿占端口）。
//! - loopback 加固：setup 中 spawn **之前** `LoopbackGuard::lock(port)`，失败仅 log
//!   警告、不阻塞启动（M1 不假设 app 有 root 权限；运行时强制待 M2/M3 特权 helper）；
//!   `RunEvent::Exit` 时 `release(port)` best-effort。guard 句柄与 port 一并存入
//!   `LoopbackGuardState`，避免 Exit 处无法取 port。
//! - WebView 加载方案：setup 异步块内 `health_probe` 通过后，用
//!   `WebviewWindow::eval("window.location.replace('http://127.0.0.1:<port>/')")`
//!   在 webview 内做客户端导航。CSP 已在 `tauri.conf.json` 设 null；Tauri 2 默认不拦
//!   顶层 location 导航（区别于 fetch/XHR 受 CORS 限制）。
//! - **M2a Task 6 接线**：health_probe 通过后构建 `TrayController`（注册为 managed
//!   state 供 badge 闭包访问）、`NativeNotifier`/`TrayBadge`（注入真实 is_unfocused/
//!   notify/inc_badge 闭包），由 `Router::default_table` 组装；observer 任务用 watcher
//!   循环包裹 `SseClient::run`，断线/异常重启；信号钩子走 `install_signal_hooks`
//!   注入 cleanup 闭包（take SidecarState + cleanup_sidecar + shutdown observer +
//!   release loopback + set ExitingFlag）。
//! - **关窗→隐藏保活**：`on_window_event` 拦截 `CloseRequested`，除非 `ExitingFlag`
//!   已置位（来自托盘退出菜单 / 信号钩子），否则 `prevent_close` + `window.hide()`。
//!
//! Tauri 版本：2.11.5（见 `Cargo.lock`）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Context;
// Manager trait 在作用域里才能用 `app.get_webview_window` 与 `app_handle.try_state`。
// Tauri 2 把这些方法放在 Manager trait 上（而非 inherent），必须显式导入。
use tauri::{Manager, RunEvent, WindowEvent};
// NotificationExt 才能用 `app.notification()`；它在 tauri-plugin-notification 上。
use tauri_plugin_notification::NotificationExt;

use inkos_desktop::config;
use inkos_desktop::isolation::{platform_guard, LoopbackGuard};
use inkos_desktop::lifecycle::{cleanup_sidecar, install_signal_hooks, ExitingFlag, SidecarState, TrayController};
use inkos_desktop::observer::notifier::NativeNotifier;
use inkos_desktop::observer::router::Router;
use inkos_desktop::observer::sse::SseClient;
use inkos_desktop::observer::tray_badge::TrayBadge;
use inkos_desktop::paths::AppPaths;
use inkos_desktop::supervisor;

/// loopback guard 句柄 + 锁定的端口，存入 Tauri managed state。
/// `RunEvent::Exit` 时取出，对端口 release（best-effort）。
///
/// `Mutex` 而非 `OnceCell`：与 `SidecarState` 一致，简化 Send + Sync 契约。
/// `port: u16`：guard 仅锁一个端口；端口 0 表示「未 lock」（release 时跳过）。
#[derive(Default)]
struct LoopbackGuardState {
    guard: std::sync::Mutex<Option<Box<dyn LoopbackGuard>>>,
    port: std::sync::Mutex<u16>,
}

impl LoopbackGuardState {
    fn new() -> Self {
        Self::default()
    }

    /// 存入 guard 句柄；port 在 `record_port` 单独记录。
    fn set_guard(&self, guard: Box<dyn LoopbackGuard>) {
        let mut g = self.guard.lock().expect("LoopbackGuardState guard mutex 中毒");
        *g = Some(guard);
    }

    /// 记录 lock 成功的端口；Exit 时按此端口 release。
    fn record_port(&self, port: u16) {
        let mut p = self.port.lock().expect("LoopbackGuardState port mutex 中毒");
        *p = port;
    }

    /// 取出 guard 句柄消费（Exit 路径）。
    fn take_guard(&self) -> Option<Box<dyn LoopbackGuard>> {
        let mut g = self.guard.lock().expect("LoopbackGuardState guard mutex 中毒");
        g.take()
    }

    /// 读取已 lock 的端口（未 lock 时返回 0）。
    fn current_port(&self) -> u16 {
        *self.port.lock().expect("LoopbackGuardState port mutex 中毒")
    }
}

/// 把任意 `Display` 错误序列化为 JS 字符串字面量（含两侧引号）。
///
/// 用 `serde_json::to_string` 做规范 JSON 字符串转义，自动处理引号/反斜杠/控制字符，
/// 替代手写 `.replace('\n',..).replace('\'',...)` 链。失败时退化为手动转义单引号
/// （serde_json 对 &str 几乎不可能失败，fallback 仅作 defensive 编译期保证）。
fn to_js_string_literal(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| {
        // 兜底：serde_json 对 &str 失败极少见（理论只在非 UTF-8 边界）；
        // 若真发生，手动转义单引号 + 用单引号包裹，保持 JS 字面量合法。
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    })
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(SidecarState::new())
        .manage(LoopbackGuardState::new())
        .manage(ExitingFlag::new())
        .on_window_event(|window, event| {
            // 关窗 → 隐藏保活（除非来自"退出"意图）。
            // - 用户点窗口 X / Cmd+W / Alt+F4：CloseRequested 触发；exiting=false
            //   → prevent_close + hide，窗口入托盘。
            // - 托盘"退出"菜单 / SIGINT/SIGTERM：先 set ExitingFlag → 后 app.exit(0)
            //   或 process::exit(0)；后者直接绕过 CloseRequested。前者走 ExitRequested
            //   → Exit（不触发 CloseRequested）。故 exiting=true 路径仅作"安全网"。
            if let WindowEvent::CloseRequested { api, .. } = event {
                let exiting = window
                    .app_handle()
                    .try_state::<ExitingFlag>()
                    .map(|f| f.is_set())
                    .unwrap_or(false);
                if !exiting {
                    api.prevent_close();
                    if let Err(e) = window.hide() {
                        eprintln!("[main] window.hide 失败: {e:#}");
                    }
                }
            }
        })
        .setup(|app| {
            // 复制一份句柄进异步块，用于把 spawn 后的 Child 写入 managed state。
            let app_handle = app.handle().clone();
            // 主窗口由 tauri.conf.json 声明，这里取已存在的实例用于 eval 导航。
            let window = app
                .get_webview_window("main")
                .context("tauri.conf.json 声明的 main 窗口未找到")?;

            tauri::async_runtime::spawn(async move {
                let outcome = async {
                    // mono-repo：submodule_root = 仓库根（packages/cli/dist 在此）。
                    // 用 `CARGO_MANIFEST_DIR`（编译期烘焙的 src-tauri/ 绝对路径）的 parent 解析，
                    // 与 Task 5 集成测运行时 `std::env::var("CARGO_MANIFEST_DIR")` 等价。
                    // 不能用 `std::env::current_dir()`——`cargo run` 会把 cwd 设为 src-tauri/
                    // 而非仓库根，导致 packages/cli/dist/index.js 路径错位（实测踩过）。
                    let manifest_dir = env!("CARGO_MANIFEST_DIR");
                    let submodule_root = std::path::Path::new(manifest_dir)
                        .parent()
                        .context("CARGO_MANIFEST_DIR 应有父目录（仓库根）")?;

                    // project_root 用 temp 目录，inkos studio 会自动初始化 minimal project。
                    let project_root = std::env::temp_dir().join("inkos-m1-demo");
                    // inkos studio 启动时 cwd 必须存在（Command::current_dir 在 dir 缺失时
                    // 直接返回 NotFound，不会进入子进程）。先确保目录在。
                    std::fs::create_dir_all(&project_root)
                        .with_context(|| format!("创建 project_root 失败: {}", project_root.display()))?;
                    let paths = AppPaths::new(
                        project_root.to_path_buf(),
                        submodule_root.to_path_buf(),
                    )
                    .context("解析 AppPaths 失败")?;

                    let port = supervisor::pick_free_port(config::DEFAULT_STUDIO_PORT)
                        .context("pick_free_port 在 [4567, 5567) 区间无空闲端口")?;

                    // loopback 加固：spawn **之前** lock，把外部入站挡在 port 之外。
                    // 失败（权限不足等）只 log 警告、继续启动——M1 不假设 app 有 root；
                    // 完整运行时强制待 M2/M3 特权 helper（SMJOP/launchd/setuid）。
                    let guard = platform_guard();
                    match guard.lock(port) {
                        Ok(()) => {
                            eprintln!(
                                "[main] loopback guard: 已 lock port={port}（外部入站被挡）"
                            );
                            if let Some(state) = app_handle.try_state::<LoopbackGuardState>() {
                                state.record_port(port);
                            }
                        }
                        Err(e) => {
                            // ⚠️ 降级：警告但绝不 panic；sidecar 仍会 spawn 但监听 0.0.0.0，
                            // 同网段可访问（架构 §8 已证）。M1 范围声明此风险。
                            eprintln!(
                                "[main] loopback guard: lock port={port} 失败（多半缺权限），降级继续: {e:#}"
                            );
                        }
                    }
                    // guard 句柄无论 lock 是否成功都存入 state——release() 对未 lock
                    // 的 anchor/rules 是幂等的，便于 Exit 统一调用。
                    if let Some(state) = app_handle.try_state::<LoopbackGuardState>() {
                        state.set_guard(guard);
                    }

                    let spec = supervisor::build_launch(&paths, port, "node");
                    let child = supervisor::spawn(&spec).context("spawn sidecar 失败")?;

                    Ok::<(u16, std::process::Child), anyhow::Error>((port, child))
                }
                .await;

                match outcome {
                    Ok((port, child)) => {
                        // 把 Child 存入 managed state，供 RunEvent::Exit 时清理。
                        if let Some(state) = app_handle.try_state::<SidecarState>() {
                            state.insert(child);
                        } else {
                            // 理论上 .manage 注册了必然能取出；走不到这条分支。
                            eprintln!("[main] 警告: SidecarState 未注册，无法清理 child");
                        }

                        let healthy = supervisor::health_probe(
                            port,
                            config::HEALTH_PROBE_TIMEOUT,
                        )
                        .await;

                        if healthy {
                            let url = format!("http://127.0.0.1:{}/", port);
                            // 客户端导航：在 webview 内替换 location。失败仅打 stderr，
                            // 不阻塞——用户会看到 stub 页面，可手动刷新。
                            if let Err(e) = window.eval(format!(
                                "window.location.replace('{}')",
                                url
                            )) {
                                eprintln!("[main] eval navigate 失败: {e}");
                            }
                            // =====================================================
                            // M2a Task 6 接线：observer + lifecycle（health_probe 通过后）
                            // =====================================================
                            wire_observer_and_lifecycle(&app_handle, port);
                        } else {
                            let secs = config::HEALTH_PROBE_TIMEOUT.as_secs();
                            let _ = window.eval(format!(
                                "document.title='inkos 启动超时 ({}s)，见日志'",
                                secs
                            ));
                            eprintln!(
                                "[main] sidecar 在 {}s 内未健康，未导航 webview（observer 未接线）",
                                secs
                            );
                        }
                    }
                    Err(e) => {
                        // 启动失败的错误回显到窗口 title，便于用户看到根因。
                        // 用 serde_json::to_string 规范转义为 JS 字符串字面量
                        // （含两侧引号），避免手写 .replace 链对引号/反斜杠/控制字符的遗漏。
                        let raw = format!("inkos 启动失败: {:#}", e).replace(['\n', '\r'], " ");
                        let literal = to_js_string_literal(&raw);
                        // literal 已含两侧双引号，直接赋给 document.title 即合法 JS。
                        let _ = window.eval(format!("document.title={}", literal));
                        eprintln!("[main] sidecar 启动失败: {:#}", e);
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app_handle, event| {
            // Exit：进程即将退出，确保 sidecar 进程组被清理 + loopback guard release +
            // observer shutdown 置真（信号钩子路径已置，正常退出路径兜底置一次）。
            // 用 Exit 而非 ExitRequested：后者可被拒绝/deref，前者必然触发。
            if let RunEvent::Exit = event {
                // 标记退出意图：观察者 watcher 循环看到 shutdown 会跳出。
                if let Some(flag) = app_handle.try_state::<ExitingFlag>() {
                    flag.set();
                }
                if let Some(state) = app_handle.try_state::<SidecarState>() {
                    let child = state.take();
                    cleanup_sidecar(child);
                }
                // 兜底：如果 managed state 因任何原因没拿到 child（例如 setup
                // 异步块还没跑完用户就关窗），无副作用——此时无 sidecar 可清。

                // loopback release：best-effort。失败仅 log，不让退出路径抛错。
                if let Some(guard_state) = app_handle.try_state::<LoopbackGuardState>() {
                    let port = guard_state.current_port();
                    if let Some(guard) = guard_state.take_guard() {
                        if let Err(e) = guard.release(port) {
                            eprintln!(
                                "[main] loopback guard: release port={port} 失败: {e:#}"
                            );
                        }
                    }
                }
            }
        });
}

/// M2a Task 6 接线：在 health_probe 通过后构建 tray + observer + 信号钩子。
///
/// 所有失败仅 `eprintln!`：接线失败不应阻塞主流程（用户已能看见 SPA），
/// 标注「集成验证待 GUI/真机」——这些路径在 CI/headless 环境下不可达。
fn wire_observer_and_lifecycle(app_handle: &tauri::AppHandle, port: u16) {
    // ---- 1. TrayController（注册为 managed state；TrayBadge 闭包通过 state 访问）----
    let tray = TrayController::build(app_handle);
    app_handle.manage(tray);

    // ---- 2. NativeNotifier（is_unfocused 查主窗口聚焦态；notify 调 notification 插件）----
    let is_unfocused_app = app_handle.clone();
    let is_unfocused: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        // 窗口隐藏 / 失焦 / 取不到 → 视为"未在前台"，发通知。
        // unwrap_or(true)：保守默认为"失焦"——拿不到窗口时宁可多发通知也不漏发。
        is_unfocused_app
            .get_webview_window("main")
            .map(|w| !w.is_focused().unwrap_or(false))
            .unwrap_or(true)
    });

    let notify_app = app_handle.clone();
    let notify: Arc<dyn Fn(&str, &str) + Send + Sync> = Arc::new(move |title, body| {
        // 通知发送失败仅 log：通知是 UX 增强，失败不应让 handler 抛错（router 会重试下一次事件）。
        if let Err(e) = notify_app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
        {
            eprintln!("[main] notification.show 失败: {e:#}");
        }
    });

    let notifier = NativeNotifier { is_unfocused, notify };

    // ---- 3. TrayBadge（inc_badge 透传到 TrayController::inc_badge，通过 managed state）----
    let badge_app = app_handle.clone();
    let inc_badge: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        // 找不到 TrayController 时仅 log：托盘可能在 GUI 不可达环境未注册。
        if let Some(tc) = badge_app.try_state::<TrayController>() {
            tc.inc_badge();
        } else {
            eprintln!("[main] TrayBadge.inc_badge: TrayController 未注册（仅 log）");
        }
    });
    let badge = TrayBadge { inc_badge };

    // ---- 4. Router + observer watcher（shutdown AtomicBool 控制生命周期）----
    let router = Arc::new(Router::default_table(
        Arc::new(notifier),
        Arc::new(badge),
    ));
    let shutdown = Arc::new(AtomicBool::new(false));
    let events_url = format!("http://127.0.0.1:{}/api/v1/events", port);

    // 先把 ObserverShutdown 注册为 managed state——cleanup 闭包在
    // install_signal_hooks 之后任何时刻都可能被信号触发调用，必须保证 manage
    // 已生效（避免 take→None 的竞态）。clone 一份给 watcher，state 留一份。
    let observer_shutdown = ObserverShutdown(Arc::clone(&shutdown));
    app_handle.manage(observer_shutdown);

    // watcher 任务：循环调用 SseClient::run；run 仅在 shutdown 时返回 Ok，
    // 但若将来实现改变导致 run 提前返回（非 shutdown），watcher 重启 observer。
    // shutdown 置真时跳出循环。这是 spec §2.1 的"observer 任务级韧性"——
    // SseClient 自带指数退避重连（连接级），watcher 是外层任务级兜底。
    let watcher_router = Arc::clone(&router);
    let watcher_shutdown = Arc::clone(&shutdown);
    let watcher_url = events_url.clone();
    tauri::async_runtime::spawn(async move {
        eprintln!("[main] observer watcher 启动: {}", watcher_url);
        while !watcher_shutdown.load(Ordering::Relaxed) {
            let client = SseClient::new(watcher_url.clone());
            let result = client
                .run(Arc::clone(&watcher_router), Arc::clone(&watcher_shutdown))
                .await;
            if watcher_shutdown.load(Ordering::Relaxed) {
                break;
            }
            match result {
                Ok(()) => {
                    // run 在非 shutdown 路径下返回 Ok——重新建 client 重连。
                    eprintln!("[main] observer watcher: run 退出（Ok），500ms 后重启");
                }
                Err(e) => {
                    // run 不应走到这（内部已捕获 IO 错误做退避）；防御性 log。
                    eprintln!("[main] observer watcher: run 异常: {e:#}，500ms 后重启");
                }
            }
            // 短暂退避避免 hot-loop（SseClient::run 内已做退避，这里只是兜底）。
            // 分段 sleep 让 shutdown 触发时立即跳出，不卡 500ms。
            for _ in 0..5 {
                if watcher_shutdown.load(Ordering::Relaxed) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
        eprintln!("[main] observer watcher 退出（shutdown=true）");
    });

    // ---- 5. install_signal_hooks（cleanup 闭包）----
    // cleanup 在 SIGINT/SIGTERM 收到时调用，之后 process::exit(0)（绕过 RunEvent::Exit），
    // 故必须自包含：set ExitingFlag + shutdown observer + take SidecarState +
    // cleanup_sidecar + release loopback。与 RunEvent::Exit 路径 idempotent 冗余——
    // 两边都做、take/AtomicBool/store 都是幂等。
    let sig_app = app_handle.clone();
    let cleanup: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        // 1. 退出意图 + observer shutdown：两个状态独立，但 cleanup 同时置。
        if let Some(flag) = sig_app.try_state::<ExitingFlag>() {
            flag.set();
        }
        if let Some(obs_shutdown) = sig_app.try_state::<ObserverShutdown>() {
            obs_shutdown.0.store(true, Ordering::SeqCst);
        }

        // 2. sidecar 清理：take Child + kill_tree + wait（idempotent）。
        if let Some(state) = sig_app.try_state::<SidecarState>() {
            let child = state.take();
            cleanup_sidecar(child);
        }

        // 3. loopback release：best-effort。
        if let Some(guard_state) = sig_app.try_state::<LoopbackGuardState>() {
            let port = guard_state.current_port();
            if let Some(guard) = guard_state.take_guard() {
                if let Err(e) = guard.release(port) {
                    eprintln!("[main] signal cleanup: loopback release port={port} 失败: {e:#}");
                }
            }
        }
    });
    install_signal_hooks(cleanup);
}

/// observer 任务的 shutdown 标志，独立于 `ExitingFlag`（语义不同：
/// ExitingFlag = "用户/信号想退出整个 app"；ObserverShutdown = "observer watcher 应停止"）。
/// 之所以分开：未来若 app 不退出但需暂停 observer（M3 reload 配置），可独立控制。
#[derive(Default, Clone)]
struct ObserverShutdown(Arc<AtomicBool>);

/// 编译期断言：传给 `.manage()` 的类型必须是 `Send + Sync`（Tauri managed state 契约）。
/// 类型一旦破坏约束，本条 const 在编译期失败，早于运行期。
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SidecarState>();
    assert_send_sync::<LoopbackGuardState>();
    assert_send_sync::<ExitingFlag>();
    assert_send_sync::<ObserverShutdown>();
    assert_send_sync::<TrayController>();
};

#[cfg(test)]
mod tests {
    use super::to_js_string_literal;

    // to_js_string_literal 用 serde_json::to_string 做规范 JS 字符串字面量转义，
    // 替代手写 .replace 链——后者对反斜杠/控制字符的遗漏是 Task 6 LOW 修复点。
    // 仅断言 happy path（serde_json 失败兜底分支触发条件极端，不可达，不测）。
    #[test]
    fn to_js_string_literal_escapes_quotes_backslash_and_control() {
        // 普通 ASCII：原样包裹双引号。
        assert_eq!(to_js_string_literal("hello"), r#""hello""#);

        // 单引号（旧手写转义的关注点）：serde_json 不转义单引号，保留即可。
        assert_eq!(
            to_js_string_literal("inkos 启动失败: can't open"),
            r#""inkos 启动失败: can't open""#
        );

        // 双引号：必须转义为 \"。
        assert_eq!(to_js_string_literal(r#"a"b"#), r#""a\"b""#);

        // 反斜杠：必须转义为 \\（旧手写 .replace 链漏掉，是引入 serde_json 的关键原因）。
        assert_eq!(to_js_string_literal(r"a\b"), r#""a\\b""#);

        // 换行（调用方已 .replace 掉，但即便漏掉 serde_json 也会转义为 \n）。
        assert_eq!(to_js_string_literal("a\nb"), "\"a\\nb\"");

        // 空串与中文（serde_json 对非 ASCII 不转义，保留可读性）。
        assert_eq!(to_js_string_literal(""), "\"\"");
        assert_eq!(to_js_string_literal("中文测试"), "\"中文测试\"");

        // 组合：中文 + 双引号 + 反斜杠 + 单引号（启动失败消息的真实形态）。
        // 验证产生的字面量是合法 JS：以 " 包裹、内部 " 与 \ 均被转义。
        let raw = "inkos 启动失败: path 'C:\\foo\\bar' 不存在";
        let literal = to_js_string_literal(raw);
        assert!(literal.starts_with('"') && literal.ends_with('"'));
        // 原串含两个单 `\`（C:\foo 与 \bar），转义后每个变 `\\`——literal 应有 4 个反斜杠。
        assert_eq!(
            literal.chars().filter(|&c| c == '\\').count(),
            4,
            "反斜杠应每个转义为两个: {}",
            literal
        );
        // 双引号序列化后变 \"，但本例 raw 无双引号；改单独构造断言。
        assert!(to_js_string_literal(r#"a"b"#).contains(r#"\""#));
    }
}
