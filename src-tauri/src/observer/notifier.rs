/// NativeNotifier：失焦时发送系统通知。
///
/// 仅当窗口失焦（`is_unfocused` 返回 true）时才调用 `notify` 闭包发送通知，
/// 避免用户正在前台阅读时被打扰。两个依赖均以闭包注入——
/// 生产环境分别绑定 `tauri-plugin-notification` 与 `AppHandle::get_focused_window`，
/// 测试环境以 spy 闭包替换，零硬依赖 Tauri API。
use super::router::EventHandler;
use super::sse::SseEvent;
use anyhow::Result;
use std::sync::Arc;

/// 调用方注入的“窗口是否失焦”判定闭包类型（生产 = 查主窗口聚焦态；测试 = spy）。
pub type IsUnfocusedFn = Arc<dyn Fn() -> bool + Send + Sync>;
/// 调用方注入的通知发送闭包类型（生产 = tauri-plugin-notification；测试 = spy）。
/// 签名为 `Fn(title, body)`——title 来自事件名映射，body 直接透传 `ev.data`。
pub type NotifyFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// is_unfocused: 调用方注入的“窗口是否失焦”判定（便于测试）。
/// notify: 调用方注入的通知发送闭包（便于测试；生产用 tauri-plugin-notification）。
/// notify 签名为 `Fn(title, body)`——title 来自事件名映射，body 直接透传 `ev.data`。
pub struct NativeNotifier {
    pub is_unfocused: IsUnfocusedFn,
    pub notify: NotifyFn,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 记录所有通知调用的 spy——`(title, body)` 列表供测试断言。
    #[derive(Default)]
    struct NotifySpy(Mutex<Vec<(String, String)>>);
    impl NotifySpy {
        fn captures(&self) -> Vec<(String, String)> {
            self.0.lock().unwrap().clone()
        }
    }

    fn ev(name: &str, data: &str) -> SseEvent {
        SseEvent {
            event: name.into(),
            data: data.into(),
        }
    }

    #[test]
    fn notifies_when_unfocused() {
        let spy = Arc::new(NotifySpy::default());
        let spy_cap = Arc::clone(&spy);
        let n = NativeNotifier {
            is_unfocused: Arc::new(|| true),
            notify: Arc::new(move |t: &str, b: &str| {
                spy_cap.0.lock().unwrap().push((t.into(), b.into()))
            }),
        };

        n.handle(&ev("write:complete", "{\"id\":12}")).unwrap();

        let caps = spy.captures();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].0, "inkos：章节写完");
        assert_eq!(caps[0].1, "{\"id\":12}");
    }

    #[test]
    fn does_not_notify_when_focused() {
        let spy = Arc::new(NotifySpy::default());
        let spy_cap = Arc::clone(&spy);
        let n = NativeNotifier {
            is_unfocused: Arc::new(|| false),
            notify: Arc::new(move |t: &str, b: &str| {
                spy_cap.0.lock().unwrap().push((t.into(), b.into()))
            }),
        };

        n.handle(&ev("write:complete", "x")).unwrap();
        n.handle(&ev("agent:complete", "y")).unwrap();

        assert!(spy.captures().is_empty());
    }

    #[test]
    fn title_mapping_covers_known_events() {
        let spy = Arc::new(NotifySpy::default());
        let spy_cap = Arc::clone(&spy);
        let n = NativeNotifier {
            is_unfocused: Arc::new(|| true),
            notify: Arc::new(move |t: &str, b: &str| {
                spy_cap.0.lock().unwrap().push((t.into(), b.into()))
            }),
        };

        n.handle(&ev("draft:complete", "d1")).unwrap();
        n.handle(&ev("book:created", "b1")).unwrap();
        n.handle(&ev("agent:complete", "a1")).unwrap();

        let caps = spy.captures();
        assert_eq!(caps[0].0, "inkos：章节写完"); // draft:complete 与 write:complete 共用
        assert_eq!(caps[1].0, "inkos：已创建新书");
        assert_eq!(caps[2].0, "inkos：agent 完成");
    }

    #[test]
    fn unknown_event_falls_back_to_generic_title() {
        let spy = Arc::new(NotifySpy::default());
        let spy_cap = Arc::clone(&spy);
        let n = NativeNotifier {
            is_unfocused: Arc::new(|| true),
            notify: Arc::new(move |t: &str, b: &str| {
                spy_cap.0.lock().unwrap().push((t.into(), b.into()))
            }),
        };

        n.handle(&ev("daemon:chapter", "dc")).unwrap(); // notifier 通常不订阅，但若被分派需容错

        let caps = spy.captures();
        assert_eq!(caps[0].0, "inkos");
    }

    #[test]
    fn handle_always_returns_ok() {
        // handler 不应向 router 上抛错误（即便未发送通知也是正常路径）
        let n = NativeNotifier {
            is_unfocused: Arc::new(|| false),
            notify: Arc::new(|_, _| {}),
        };
        assert!(n.handle(&ev("any", "")).is_ok());
    }
}
