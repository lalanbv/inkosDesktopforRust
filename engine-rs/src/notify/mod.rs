//! 通知域（格式化 + 通道派发）。
//!
//! 移植自 `packages/core/src/notify/`：
//! - `format`：stripMarkdownMarks（markdown→纯文本净化）
//! - [`NotifyMessage`] 类型（dispatcher 的消息形状）
//!
//! 111 号补齐 dispatcher 与四通道发送器（telegram/feishu/wechat-work/webhook
//! ——webhook 含 HMAC-SHA256 签名与事件订阅过滤）。

pub mod dispatcher;
pub mod format;

pub use dispatcher::{dispatch_notification, dispatch_webhook_event, parse_notify_channels, WebhookPayload};
pub use crate::models::project::{NotifyChannel, NotifyFormat};

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 通知消息（标题 + 正文）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct NotifyMessage {
    pub title: String,
    pub body: String,
}
