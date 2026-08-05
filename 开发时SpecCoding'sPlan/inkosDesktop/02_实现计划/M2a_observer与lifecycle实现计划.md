# inkosDesktop M2a 实现计划：observer + lifecycle + 信号钩子

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 实现。Steps 用 `- [ ]` 跟踪。

**Goal:** 在 M1 可运行壳上加原生桌面体验与进程韧性：observer 旁路 SSE→原生通知+托盘角标；托盘保活后台写作；SIGINT/SIGTERM→graceful kill_tree；顺带 M1 清理。

**Architecture:** γ 旁路增强派（架构 v1.2）。observer 为 Tauri 异步任务，只读订阅 inkos SSE（零写 inkos/零改 SPA）；lifecycle 扩 M1 的 SidecarState/cleanup_sidecar，加托盘与信号钩子。

**Tech Stack:** Rust + Tauri 2（tray-icon）、reqwest（SSE 流）、tauri-plugin-notification（原生通知）、tokio::signal。

## Global Constraints（架构 v1.2 + M2a spec）

- 零修改 inkos `packages/`；mono-repo 零交叉（壳只进 `src-tauri/`，永不改 inkos 根文件）
- observer **只读** SSE，零写入 inkos，零修改 SPA
- Rust：不可变、单一职责<400 行、无静默吞错、显式错误传播
- 端口默认 4567（`INKOS_STUDIO_PORT`，M1 已实现）
- 复用 M1：`supervisor::{spawn, kill_tree}`、`lifecycle::{SidecarState, cleanup_sidecar}`
- 测试覆盖≥80%；命名禁空格

### 前置（M1 已就位，master @ c6f854cd）
- `supervisor`（spawn/health_probe/kill_tree/pick_free_port/build_launch）
- `lifecycle::SidecarState` + `cleanup_sidecar`
- `isolation`（loopback 模块）
- `paths`/`config`

---

## File Structure

| 路径 | 职责 |
|---|---|
| `src-tauri/src/observer/mod.rs` | re-export |
| `src-tauri/src/observer/sse.rs` | `SseEvent`、`parse_sse_frame`、`SseClient`（订阅+退避重连） |
| `src-tauri/src/observer/router.rs` | `EventHandler` trait、`Router`、默认路由表 |
| `src-tauri/src/observer/notifier.rs` | `NativeNotifier`（tauri-plugin-notification） |
| `src-tauri/src/observer/tray_badge.rs` | `TrayBadge`（角标计数） |
| `src-tauri/src/lifecycle.rs`（扩 M1） | `TrayController`、`install_signal_hooks` |
| `src-tauri/src/main.rs`（改） | 接 observer 任务+watcher、信号钩子、托盘 |
| `src-tauri/Cargo.toml`（改） | +`tauri-plugin-notification`、tauri "tray-icon" feature |
| `src-tauri/tests/observer_integration.rs` | observer 集成测（mock SSE server） |

---

## Task 1: observer SSE 解析 + SseEvent（纯函数 TDD）

**Files:** Create `src-tauri/src/observer/mod.rs`、`src-tauri/src/observer/sse.rs`；Modify `src-tauri/src/lib.rs`（`pub mod observer;`）。Test: 内联单测。

**Interfaces:** Produces `struct SseEvent { event: String, data: String }`、`fn parse_sse_frame(buf: &str) -> Vec<SseEvent>`（按 `\n\n` 分帧，每帧 `event:`/`data:` 行组装）。

- [ ] **Step 1: 写失败测试**

```rust
// src-tauri/src/observer/sse.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent { pub event: String, pub data: String }

/// 解析 SSE 文本缓冲为事件列表。按空行(\n\n)分帧；每帧 event:/data: 行组装。
/// 不完整末帧（无 \n\n 结尾）返回 None 残余由调用方缓存——本函数只返回完整帧。
pub fn parse_sse_frame(buf: &str) -> (Vec<SseEvent>, String) {
    let mut out = Vec::new();
    let mut rest = buf.to_string();
    while let Some(idx) = rest.find("\n\n") {
        let frame = rest[..idx].to_string();
        rest = rest[idx+2..].to_string();
        let mut ev = String::new(); let mut data = String::new();
        for line in frame.lines() {
            if let Some(v) = line.strip_prefix("event:") { ev = v.trim().to_string(); }
            else if let Some(v) = line.strip_prefix("data:") { data = v.trim().to_string(); }
        }
        if !ev.is_empty() { out.push(SseEvent { event: ev, data }); }
    }
    (out, rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_one_complete_frame() {
        let (evs, rest) = parse_sse_frame("event: write:complete\ndata: {\"id\":1}\n\n");
        assert_eq!(evs, vec![SseEvent{event:"write:complete".into(),data:"{\"id\":1}".into()}]);
        assert!(rest.is_empty());
    }
    #[test]
    fn keeps_partial_frame_as_rest() {
        let (evs, rest) = parse_sse_frame("event: ping\ndata: \n\nevent: write:start\ndata: x");
        assert_eq!(evs.len(), 1);
        assert_eq!(rest, "event: write:start\ndata: x");
    }
    #[test]
    fn skips_frames_without_event_field() {
        let (evs, _) = parse_sse_frame("data: noevent\n\n");
        assert!(evs.is_empty());
    }
}
```

- [ ] **Step 2: 跑测试** — `cd src-tauri && cargo test observer::sse` → PASS(3)。
- [ ] **Step 3: 注册模块**

```rust
// src-tauri/src/observer/mod.rs
pub mod sse; pub mod router; pub mod notifier; pub mod tray_badge;
```
（router/notifier/tray_badge 在后续 task 建；先注释占位或建空文件避免编译错——本 task 只建 `mod.rs` 含 `pub mod sse;`，其余 task 加。）

```rust
// src-tauri/src/lib.rs 追加
pub mod observer;
```

- [ ] **Step 4: Commit** — `git add src-tauri/src/observer src-tauri/src/lib.rs && git commit -m "feat(observer): SSE frame parser + SseEvent"`

---

## Task 2: observer Router + EventHandler + 默认路由表（纯逻辑 TDD）

**Files:** Create `src-tauri/src/observer/router.rs`。Test: 内联单测（spy handler）。

**Interfaces:** Consumes `SseEvent`。Produces `trait EventHandler { fn handle(&self, ev: &SseEvent) -> anyhow::Result<()>; }`、`Router { handlers_by_event: HashMap<String, Vec<Arc<dyn EventHandler>>> }`、`Router::default_table(notifier: Arc<dyn EventHandler>, badge: Arc<dyn EventHandler>) -> Router`。

- [ ] **Step 1: 写失败测试（spy handler 记录调用）**

```rust
// src-tauri/src/observer/router.rs
use super::sse::SseEvent;
use std::sync::{Arc, Mutex};
use anyhow::Result;

pub trait EventHandler: Send + Sync { fn handle(&self, ev: &SseEvent) -> Result<()>; }

pub struct Router { table: std::collections::HashMap<String, Vec<Arc<dyn EventHandler>>> }

impl Router {
    pub fn new() -> Self { Self { table: Default::default() } }
    pub fn register(&mut self, event: &str, h: Arc<dyn EventHandler>) {
        self.table.entry(event.to_string()).or_default().push(h);
    }
    pub fn dispatch(&self, ev: &SseEvent) {
        if let Some(hs) = self.table.get(&ev.event) {
            for h in hs { if let Err(e) = h.handle(ev) { eprintln!("[observer] handler error on {}: {e:#}", ev.event); } }
        }
        // 未知事件：忽略（架构 §6.2 容错）
    }
    /// 默认路由表（M2a spec §2.1）。notifier/badge 由调用方注入（便于测试 spy）。
    pub fn default_table(notifier: Arc<dyn EventHandler>, badge: Arc<dyn EventHandler>) -> Self {
        let mut r = Self::new();
        for e in ["write:complete","draft:complete","book:created","agent:complete"] {
            r.register(e, notifier.clone()); r.register(e, badge.clone());
        }
        r.register("daemon:chapter", badge);  // 仅角标
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Spy(Mutex<Vec<String>>);
    impl EventHandler for Spy { fn handle(&self, ev: &SseEvent) -> Result<()> { self.0.lock().unwrap().push(ev.event.clone()); Ok(()) } }

    #[test]
    fn default_table_routes_write_complete_to_both() {
        let n = Arc::new(Spy(Mutex::new(vec![]))) as Arc<dyn EventHandler>;
        let n_cap = n.clone();
        // 通过 downcast Arc 捕获不便；改用 wrapper 计数
        struct Count(Mutex<u32>); impl EventHandler for Count { fn handle(&self,_:&SseEvent)->Result<()>{ *self.0.lock().unwrap()+=1; Ok(()) } }
        let notifier = Arc::new(Count(Mutex::new(0)));
        let badge = Arc::new(Count(Mutex::new(0)));
        let n2 = notifier.clone(); let b2 = badge.clone();
        let r = Router::default_table(notifier, badge);
        r.dispatch(&SseEvent{event:"write:complete".into(), data:"{}".into()});
        r.dispatch(&SseEvent{event:"daemon:chapter".into(), data:"{}".into()});
        r.dispatch(&SseEvent{event:"unknown:event".into(), data:"{}".into()});
        assert_eq!(*n2.0.lock().unwrap(), 1); // write:complete → notifier；daemon:chapter → 不通知
        assert_eq!(*b2.0.lock().unwrap(), 2); // write:complete + daemon:chapter → badge
    }
}
```

- [ ] **Step 2: 跑测试** — `cargo test observer::router` → PASS(1)。
- [ ] **Step 3: 注册** — `observer/mod.rs` 加 `pub mod router;`。
- [ ] **Step 4: Commit** — `feat(observer): router + default routing table`

---

## Task 3: NativeNotifier + TrayBadge handler

**Files:** Create `src-tauri/src/observer/notifier.rs`、`src-tauri/src/observer/tray_badge.rs`；Modify `Cargo.toml`（+`tauri-plugin-notification`）。

**Interfaces:** Consumes `SseEvent`。Produces `NativeNotifier`（实现 EventHandler，失焦时发系统通知）、`TrayBadge`（实现 EventHandler，角标 +1，通过 `TrayController::set_badge`）。

- [ ] **Step 1: Cargo 加依赖** — `[dependencies] tauri-plugin-notification = "2"`；tauri features 加 `"tray-icon"`。
- [ ] **Step 2: NativeNotifier（失焦才通知——焦点状态由 AppHandle 取，注入避免硬依赖）**

```rust
// src-tauri/src/observer/notifier.rs
use super::router::EventHandler; use super::sse::SseEvent; use anyhow::Result;
use std::sync::Arc;

/// is_unfocused: 调用方注入的"窗口是否失焦"判定（便于测试）。
/// notify: 调用方注入的通知发送闭包（便于测试；生产用 tauri-plugin-notification）。
pub struct NativeNotifier {
    pub is_unfocused: Arc<dyn Fn() -> bool + Send + Sync>,
    pub notify: Arc<dyn Fn(&str, &str) + Send + Sync>,
}
impl EventHandler for NativeNotifier {
    fn handle(&self, ev: &SseEvent) -> Result<()> {
        if (self.is_unfocused)() {
            let title = match ev.event.as_str() {
                "write:complete" | "draft:complete" => "inkos：章节写完",
                "book:created" => "inkos：已创建新书",
                "agent:complete" => "inkos：agent 完成",
                _ => "inkos",
            };
            (self.notify)(title, &ev.data);
        }
        Ok(())
    }
}
```

- [ ] **Step 3: TrayBadge（角标计数，委托 TrayController）**

```rust
// src-tauri/src/observer/tray_badge.rs
use super::router::EventHandler; use super::sse::SseEvent; use anyhow::Result;
use std::sync::Arc;

/// inc_badge: 调用方注入（生产=TrayController::inc_badge；测试=spy 计数）。
pub struct TrayBadge { pub inc_badge: Arc<dyn Fn() + Send + Sync> }
impl EventHandler for TrayBadge {
    fn handle(&self, _ev: &SseEvent) -> Result<()> { (self.inc_badge)(); Ok(()) }
}
```

- [ ] **Step 4: 单测（注入 spy 闭包断言调用）** — 各写 1 个测试：unfocused→notify 调用 / focused→不调用；badge→inc 调用。
- [ ] **Step 5: 跑测试** — `cargo test observer::{notifier,tray_badge}` → PASS。
- [ ] **Step 6: 注册 + Commit** — `mod.rs` 加 `pub mod notifier; pub mod tray_badge;` → `feat(observer): native notifier + tray badge handlers`

---

## Task 4: SseClient（订阅 + 退避重连，mock server 集成测）

**Files:** Modify `src-tauri/src/observer/sse.rs`（加 SseClient）；Create `src-tauri/tests/observer_integration.rs`。

**Interfaces:** Consumes `Router`。Produces `SseClient::new(url)`、`async fn run(&self, router: Arc<Router>, shutdown: CancellationToken) -> anyhow::Result<()>`（断线退避重连，上限 30s）。

- [ ] **Step 1: SseClient 实现（reqwest 流式 + parse_sse_frame + 退避）**

```rust
// 追加 src-tauri/src/observer/sse.rs
use std::time::Duration;
use tokio_util::sync::CancellationToken;  // 或自建 AtomicBool shutdown

pub struct SseClient { url: String }
impl SseClient {
    pub fn new(url: String) -> Self { Self { url } }
    /// 持续订阅；连接错误指数退避(1s..30s)重连，直到 shutdown。
    pub async fn run(&self, router: Arc<crate::observer::router::Router>, shutdown: CancellationToken) -> anyhow::Result<()> {
        let mut backoff = Duration::from_secs(1);
        loop {
            if shutdown.is_cancelled() { return Ok(()); }
            match self.connect_once(&router, &shutdown).await {
                Ok(()) => { backoff = Duration::from_secs(1); }      // 正常结束（shutdown）
                Err(e) => { eprintln!("[observer] SSE 断开({e:#}), {:?} 后重连", backoff); }
            }
            if shutdown.is_cancelled() { return Ok(()); }
            tokio::select! {
                _ = tokio::time::sleep(backoff) => { backoff = (backoff*2).min(Duration::from_secs(30)); }
                _ = shutdown.cancelled() => return Ok(()),
            }
        }
    }
    async fn connect_once(&self, router: &Arc<crate::observer::router::Router>, shutdown: &CancellationToken) -> anyhow::Result<()> {
        let client = reqwest::Client::builder().build()?;
        let resp = client.get(&self.url).send().await?.error_for_status()?;
        let mut bytes = resp.bytes_stream();
        use tokio_stream::StreamExt;
        let mut buf = String::new();
        while let Some(chunk) = bytes.next().await {
            if shutdown.is_cancelled() { return Ok(()); }
            let chunk = chunk?;
            buf.push_str(std::str::from_utf8(&chunk)?);
            let (events, rest) = parse_sse_frame(&buf);
            buf = rest;
            for ev in events { router.dispatch(&ev); }
        }
        Ok(())
    }
}
```

> 依赖：`tokio-stream`、`tokio-util`（CancellationToken）。Cargo 加 `tokio-stream = "0.1"`、`tokio-util = { version = "0.7", features = ["rt"] }`。若不想引 tokio-util，shutdown 用 `Arc<AtomicBool>`。

- [ ] **Step 2: mock server 集成测（发一帧后断开 → 断言 router 收到 + 重连不 panic）**

```rust
// src-tauri/tests/observer_integration.rs
use inkos_desktop::observer::{router::Router, sse::SseClient};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn sse_client_dispatches_frame_and_reconnects() {
    // 起 mock SSE server：接受连接，发一帧 write:complete，然后关闭（触发重连）
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            if let Ok((mut s,_)) = listener.accept().await {
                use tokio::io::AsyncWriteExt;
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\nevent: write:complete\ndata: {}\n\n").await;
                let _ = s.shutdown().await; // 断开，触发客户端重连
            }
        }
    });
    struct Count(Mutex<u32>); impl inkos_desktop::observer::router::EventHandler for Count { fn handle(&self,_:&inkos_desktop::observer::sse::SseEvent)->anyhow::Result<()>{ *self.0.lock().unwrap()+=1; Ok(()) } }
    let badge = Arc::new(Count(Mutex::new(0))) as Arc<dyn inkos_desktop::observer::router::EventHandler>;
    let router = Arc::new(Router::default_table(Arc::new(Count(Mutex::new(0))), badge.clone()));
    let shutdown = CancellationToken::new();
    let url = format!("http://127.0.0.1:{port}/api/v1/events");
    let client = SseClient::new(url);
    // 跑 500ms 让它连一次、收一帧、断开、重连一次
    let jh = tokio::spawn(client.run(router, shutdown.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    shutdown.cancel();
    let _ = jh.await;
    // 至少收到 1 次（badge 计数 ≥1）——重连后又会收，断言 ≥1
    assert!(*badge.downcast_ref::<Count>().unwrap().0.lock().unwrap() >= 1, "应至少派发一次 write:complete 到 badge");
}
```

> 注：`Arc<dyn EventHandler>` downcast 需 `Any`；测试可用 concrete 类型包装。若 downcast 不便，改用 `Arc<Mutex<u32>>` 共享计数直接注入。实现者按可测性调整 handler 注入方式。

- [ ] **Step 3: 跑测试** — `cargo test --test observer_integration` → PASS。
- [ ] **Step 4: Commit** — `feat(observer): SseClient with backoff reconnect`

---

## Task 5: lifecycle TrayController + install_signal_hooks

**Files:** Modify `src-tauri/src/lifecycle.rs`（M1 已有）；可能 `Cargo.toml` tauri "tray-icon" feature。

**Interfaces:** Consumes Tauri AppHandle。Produces `TrayController::build(app) -> Self`（建托盘+菜单）、`inc_badge/dec_badge/clear_badge`、`set_daemon_status`；`install_signal_hooks(cleanup: Arc<dyn Fn() + Send + Sync>)`。

- [ ] **Step 1: TrayController（角标计数纯逻辑可测 + 托盘构建）**

```rust
// src-tauri/src/lifecycle.rs 追加
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::{AppHandle, Manager};

pub struct TrayController { badge: Arc<AtomicU32>, app: AppHandle }

impl TrayController {
    pub fn build(app: AppHandle) -> Self {
        let tc = Self { badge: Arc::new(AtomicU32::new(0)), app: app.clone() };
        let badge = tc.badge.clone();
        let _tray = tauri::tray::TrayIconBuilder::with_id("main")
            .tooltip("inkosDesktop")
            .menu(&tc.build_menu(&app, 0))
            .on_menu_event(move |app, ev| {
                match ev.id().as_ref() {
                    "show" => { if let Some(w)=app.get_webview_window("main"){ let _=w.show(); let _=w.set_focus(); } }
                    "quit" => { /* 触发 cleanup_sidecar 后 exit */ app.exit(0); }
                    _ => {}
                }
            })
            .build().expect("托盘构建失败");
        tc
    }
    fn build_menu(&self, _app: &AppHandle, badge: u32) -> tauri::menu::Menu<tauri::Wry> {
        let badge_label = if badge>0 { format!("未读: {badge}") } else { "未读: 0".into() };
        tauri::menu::MenuBuilder::new(_app).items(&[&tauri::menu::MenuItem::new(_app,badge_label,true).unwrap(), &tauri::menu::MenuItem::with_id(_app,"show","显示窗口",true,None::<&str>).unwrap(), &tauri::menu::MenuItem::with_id(_app,"quit","退出",true,None::<&str>).unwrap()]).build().unwrap()
    }
    pub fn inc_badge(&self) { let b=self.badge.fetch_add(1,Ordering::SeqCst)+1; let _=self.refresh(b); }
    pub fn clear_badge(&self) { self.badge.store(0,Ordering::SeqCst); let _=self.refresh(0); }
    fn refresh(&self, b: u32) -> anyhow::Result<()> { /* 更新托盘菜单文案/角标——平台差异，best-effort */ Ok(()) }
}

/// 安装信号钩子：SIGINT/SIGTERM/SIGBREAK → cleanup()（kill_tree sidecar）。
pub fn install_signal_hooks(cleanup: Arc<dyn Fn() + Send + Sync>) {
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        cleanup();
        std::process::exit(0);
    });
    #[cfg(unix)]
    { let c = cleanup.clone(); tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut s) = signal(SignalKind::terminate()) { s.recv().await; cleanup(); std::process::exit(0); }
    }); }
}
```

- [ ] **Step 2: 单测（inc/clear 计数；signal hook 用注入闭包——信号本身难测，测 cleanup 被调用）** — 计数纯逻辑测；signal hook 标注"集成验证待 GUI 环境"。
- [ ] **Step 3: 跑测试** — `cargo test lifecycle` → PASS。
- [ ] **Step 4: Commit** — `feat(lifecycle): tray controller + signal hooks`

---

## Task 6: 接线 + M1 清理

**Files:** Modify `src-tauri/src/main.rs`、`src-tauri/src/supervisor.rs`（删死 env）。

- [ ] **Step 1: main.rs 接 observer 任务 + watcher + 信号钩子 + 托盘** — setup 里 health_probe 通过后：建 `TrayController`；建 `NativeNotifier`/`TrayBadge`（注入真实 is_unfocused/notify/inc_badge）+ `Router::default_table`；`SseClient::new(url).run(router, shutdown)` 用 `tauri::async_runtime::spawn`（外包 watcher 重启 observer）；`install_signal_hooks` 注入 `cleanup_sidecar` 闭包。注册 `tauri-plugin-notification`。
- [ ] **Step 2: 窗口关闭→hide 保活** — `on_window_event`：`WindowEvent::CloseRequested` → `api.prevent_close()` + `window.hide()`（除非来自"退出"标志）。
- [ ] **Step 3: M1 清理-1** — `supervisor::build_launch` 移除 `INKOS_PROJECT_ROOT` env 注入（死代码）；更新 Task 4 测试断言（env 不含该键；cwd 仍=project_root）。
- [ ] **Step 4: M1 清理-2** — `main.rs` eval 的错误消息改 `serde_json::to_string(&msg)` 规范 JS 转义。
- [ ] **Step 5: 冒烟** — `cargo run`：窗口开→SPA 加载→托盘出现→关窗隐藏→（人工触发：可在窗口内跑写作；自动化仅验证编译+托盘构建无 panic）。若 GUI 不可用，至少 `cargo build` + 全量 `cargo test` 过。
- [ ] **Step 6: Commit** — `feat(app): wire observer/tray/signal hooks + M1 cleanup`

---

## Task 7: M2a 冒烟归档 + 覆盖率 + tag

- [ ] **Step 1** — 全量 `cargo test` 多次稳定（继承 M1 的 TOCTOU 已修）；`cargo llvm-cov` 覆盖率≥80%（observer 纯逻辑全覆盖；SSE/通知/托盘 IO 尽力）。
- [ ] **Step 2** — 归档 `变更记录文档/20260806/M2a冒烟验证.md`（observer 路由表验证、SSE 重连、托盘保活、信号钩子、清理项、parked: GUI 端到端通知可见性待真机）。
- [ ] **Step 3** — `git tag -a v0.2.0-m2a -m "M2a: observer + lifecycle + signal hooks"`。
- [ ] **Step 4** — Commit + 报告。

---

## Self-Review

- **Spec 覆盖**：observer(SSE 解析/router/notifier/badge/client) → T1-4；lifecycle(托盘/信号) → T5；接线+清理 → T6；冒烟 → T7。spec §2 全覆盖。
- **占位符**：T6 接线较概要（main.rs 改动描述性）——因接线依赖 T1-5 的具体 trait 形态，实现者按 spec §6 契约落地；非占位，是集成步骤的合理抽象。
- **类型一致**：`SseEvent`/`EventHandler`/`Router`/`TrayController` 跨任务签名一致；复用 M1 `cleanup_sidecar`/`SidecarState`。
- **YAGNI**：M2a 不做 keychain(M2b)、不做全局快捷键/通知配置(M3)、不做 loopback 强制(M3)。

## 后续

- **M2b**（secrets Route A）：keyring + keychain→secrets.json + 文件监听回写 + 防回环 + 首迁。独立 spec→plan。
- **M3**：loopback 强制、Windows、打包、updater、CI、签名、全局快捷键、通知配置。
