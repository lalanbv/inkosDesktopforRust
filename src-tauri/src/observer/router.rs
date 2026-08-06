/// observer 事件路由（纯逻辑，无 IO）。
///
/// Router 维护 event 名 → handler 列表的映射，`dispatch` 时按注册顺序调用。
/// 默认路由表按 M2a spec §2.1：
/// - `write:complete` / `draft:complete` / `book:created` / `agent:complete` → notifier + badge
/// - `daemon:chapter` → 仅 badge（角标更新，不弹通知）
/// - 其他事件 → 忽略（架构 §6.2 容错：未知事件不应使 observer 崩溃）
///
/// handler 错误用 `eprintln!` 显式记录（不静默吞错），并继续执行同事件后续 handler。
use super::sse::SseEvent;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

/// 事件处理器 trait。`Send + Sync` 以便跨线程（Tauri 命令 / 异步任务）以 `Arc` 共享。
pub trait EventHandler: Send + Sync {
    fn handle(&self, ev: &SseEvent) -> Result<()>;
}

/// 事件路由器：事件名 → handler 列表。不可变借用即可分派（`register` 需 `&mut`）。
pub struct Router {
    table: HashMap<String, Vec<Arc<dyn EventHandler>>>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            table: HashMap::new(),
        }
    }

    /// 注册 handler 到指定事件名（追加到现有列表，`dispatch` 时按注册顺序调用）。
    pub fn register(&mut self, event: &str, h: Arc<dyn EventHandler>) {
        self.table.entry(event.to_string()).or_default().push(h);
    }

    /// 分派事件：查表 → 顺序调用。handler 错误用 `eprintln!` 显式记录，不中断后续 handler。
    /// 未知事件：忽略（架构 §6.2 容错）。
    pub fn dispatch(&self, ev: &SseEvent) {
        if let Some(hs) = self.table.get(&ev.event) {
            for h in hs {
                if let Err(e) = h.handle(ev) {
                    eprintln!("[observer] handler error on {}: {e:#}", ev.event);
                }
            }
        }
    }

    /// 已注册的事件名集合（C9：契约测断言 default_table 的键 ⊆ inkos broadcast 事件）。
    /// 返回排序后的副本（非 `&str` 因 `HashMap` key 生命周期与 `self` 绑定，借引用即可）。
    /// 公开仅为 `dispatch`/`register` 的只读视图——不暴露 handler 列表。
    pub fn events(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = self.table.keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        keys
    }

    /// 默认路由表（M2a spec §2.1）。notifier / badge 由调用方注入便于测试以 spy 替换。
    pub fn default_table(notifier: Arc<dyn EventHandler>, badge: Arc<dyn EventHandler>) -> Self {
        let mut r = Self::new();
        for e in [
            "write:complete",
            "draft:complete",
            "book:created",
            "agent:complete",
        ] {
            r.register(e, notifier.clone());
            r.register(e, badge.clone());
        }
        r.register("daemon:chapter", badge); // 仅角标
        r
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 计数 handler——内部计数器公开，测试侧用 `Arc::clone` 后直接读取（避免 `downcast`）。
    struct Count(Mutex<u32>);
    impl EventHandler for Count {
        fn handle(&self, _ev: &SseEvent) -> Result<()> {
            *self.0.lock().unwrap() += 1;
            Ok(())
        }
    }

    /// 总是失败的 handler——验证 handler 错误不静默吞、且不中断同事件后续 handler。
    struct Failing;
    impl EventHandler for Failing {
        fn handle(&self, _ev: &SseEvent) -> Result<()> {
            anyhow::bail!("intentional test failure")
        }
    }

    fn ev(name: &str) -> SseEvent {
        SseEvent {
            event: name.into(),
            data: "{}".into(),
        }
    }

    #[test]
    fn default_table_routes_user_events_to_both_handlers() {
        let notifier = Arc::new(Count(Mutex::new(0)));
        let badge = Arc::new(Count(Mutex::new(0)));
        let r = Router::default_table(notifier.clone(), badge.clone());

        for e in ["write:complete", "draft:complete", "book:created", "agent:complete"] {
            r.dispatch(&ev(e));
        }
        r.dispatch(&ev("daemon:chapter"));

        assert_eq!(*notifier.0.lock().unwrap(), 4); // 4 个用户事件双投递
        assert_eq!(*badge.0.lock().unwrap(), 5); // 4 用户事件 + daemon:chapter
    }

    #[test]
    fn daemon_chapter_routes_only_to_badge() {
        let notifier = Arc::new(Count(Mutex::new(0)));
        let badge = Arc::new(Count(Mutex::new(0)));
        let r = Router::default_table(notifier.clone(), badge.clone());

        r.dispatch(&ev("daemon:chapter"));

        assert_eq!(*notifier.0.lock().unwrap(), 0);
        assert_eq!(*badge.0.lock().unwrap(), 1);
    }

    #[test]
    fn unknown_event_is_ignored() {
        let notifier = Arc::new(Count(Mutex::new(0)));
        let badge = Arc::new(Count(Mutex::new(0)));
        let r = Router::default_table(notifier.clone(), badge.clone());

        r.dispatch(&ev("unknown:event"));
        r.dispatch(&ev(""));
        r.dispatch(&ev("write:start"));

        assert_eq!(*notifier.0.lock().unwrap(), 0);
        assert_eq!(*badge.0.lock().unwrap(), 0);
    }

    #[test]
    fn register_adds_handler_for_new_event() {
        let count = Arc::new(Count(Mutex::new(0)));
        let mut r = Router::new();
        r.register("custom:event", count.clone());
        r.dispatch(&ev("custom:event"));
        r.dispatch(&ev("custom:event"));

        assert_eq!(*count.0.lock().unwrap(), 2);
    }

    #[test]
    fn failing_handler_does_not_break_chain() {
        // 同一事件先注册 Failing 再注册 Count——Failing 失败后 Count 仍应被调用。
        let count = Arc::new(Count(Mutex::new(0)));
        let mut r = Router::new();
        r.register("flaky:event", Arc::new(Failing) as Arc<dyn EventHandler>);
        r.register("flaky:event", count.clone());

        r.dispatch(&ev("flaky:event"));

        assert_eq!(*count.0.lock().unwrap(), 1);
    }

    #[test]
    fn dispatch_on_empty_router_does_not_panic() {
        let r = Router::new();
        r.dispatch(&ev("anything"));
        r.dispatch(&ev(""));
    }
}
