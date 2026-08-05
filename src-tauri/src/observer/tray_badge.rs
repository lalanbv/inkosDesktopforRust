/// TrayBadge：每次事件触发托盘角标 +1。
///
/// 仅委托 `inc_badge` 闭包——生产环境绑定 `TrayController::set_badge(current+1)`，
/// 测试环境以 spy 计数替换。角标逻辑与通知解耦：任何订阅事件都应增加角标，
/// 无论窗口是否聚焦（用户切回窗口前能看到未读数）。
use super::router::EventHandler;
use super::sse::SseEvent;
use anyhow::Result;
use std::sync::Arc;

/// inc_badge: 调用方注入（生产 = TrayController::inc_badge；测试 = spy 计数）。
pub struct TrayBadge {
    pub inc_badge: Arc<dyn Fn() + Send + Sync>,
}

impl EventHandler for TrayBadge {
    fn handle(&self, _ev: &SseEvent) -> Result<()> {
        (self.inc_badge)();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    /// 原子计数 spy——验证多次 handle 调用累积次数。
    #[derive(Default)]
    struct CountSpy(AtomicU32);

    /// Mutex 记录最近一次事件的 spy——验证 badge 不读事件内容（仅计数）。
    #[derive(Default)]
    struct LastEventSpy(Mutex<Option<String>>);

    fn ev(name: &str) -> SseEvent {
        SseEvent {
            event: name.into(),
            data: "{}".into(),
        }
    }

    #[test]
    fn inc_badge_called_once_per_event() {
        let count = Arc::new(CountSpy::default());
        let count_cap = Arc::clone(&count);
        let b = TrayBadge {
            inc_badge: Arc::new(move || {
                count_cap.0.fetch_add(1, Ordering::SeqCst);
            }),
        };

        b.handle(&ev("write:complete")).unwrap();
        b.handle(&ev("draft:complete")).unwrap();
        b.handle(&ev("daemon:chapter")).unwrap();

        assert_eq!(count.0.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn inc_badge_ignores_event_payload() {
        // badge handler 不应读 ev.event / ev.data——任何事件均仅触发一次 inc
        let last = Arc::new(LastEventSpy::default());
        let last_cap = Arc::clone(&last);
        let b = TrayBadge {
            inc_badge: Arc::new(move || {
                *last_cap.0.lock().unwrap() = Some("called".into());
            }),
        };

        b.handle(&ev("anything")).unwrap();

        assert_eq!(*last.0.lock().unwrap(), Some("called".into()));
    }

    #[test]
    fn handle_returns_ok() {
        let b = TrayBadge {
            inc_badge: Arc::new(|| {}),
        };
        assert!(b.handle(&ev("x")).is_ok());
    }
}
