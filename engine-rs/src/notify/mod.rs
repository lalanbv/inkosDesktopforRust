//! 通知域（格式化 + 通道派发）。
//!
//! 移植自 `packages/core/src/notify/`：
//! - [`format`]：stripMarkdownMarks（markdown→纯文本净化）
//! - [`NotifyMessage`] 类型（dispatcher 的消息形状）
//!
//! ## 待移植（需 reqwest HTTP）
//! dispatcher.dispatchNotification（按通道类型派发）+ telegram/feishu/wechat-work/webhook
//! 各通道的 send*（实际 HTTP POST）。

pub mod format;

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
