//! 通知派发（dispatcher + 四通道发送器）。
//!
//! 移植自 `packages/core/src/notify/dispatcher.ts`（102 行）与 telegram.ts /
//! feishu.ts / wechat-work.ts / webhook.ts 四发送器（111 号，72 号备案收口）。
//!
//! - [`dispatch_notification`]：通用文本通知（markdown/纯文本）按通道类型派发；
//! - [`dispatch_webhook_event`]：结构化事件只发 webhook 型通道（事件订阅过滤）。
//! - 失败**只记 stderr 不抛**（TS "notification failure shouldn't block pipeline"）。

use serde::Serialize;
use serde_json::Value;

use super::format::strip_markdown_marks;
use super::NotifyMessage;
use crate::models::project::{NotifyChannel, NotifyFormat};

/// 结构化 webhook 事件载荷（TS `WebhookPayload` 逐字）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookPayload {
    pub event: String,
    pub book_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_number: Option<u32>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// `sendTelegram`：POST api.telegram.org sendMessage；markdown 才带 parse_mode。
async fn send_telegram(config_bot_token: &str, chat_id: &str, message: &str, format: NotifyFormat) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{config_bot_token}/sendMessage");
    let mut body = serde_json::json!({ "chat_id": chat_id, "text": message });
    if format == NotifyFormat::Markdown {
        body["parse_mode"] = serde_json::json!("Markdown");
    }
    post_and_expect_ok(&url, body, "Telegram").await
}

/// `sendFeishu`：text → msg_type text；markdown → interactive 卡片（blue 模板）。
async fn send_feishu(webhook_url: &str, title: &str, content: &str, format: NotifyFormat) -> Result<(), String> {
    let payload = if format == NotifyFormat::Text {
        serde_json::json!({
            "msg_type": "text",
            "content": { "text": format!("{title}\n\n{content}") },
        })
    } else {
        serde_json::json!({
            "msg_type": "interactive",
            "card": {
                "header": { "title": { "tag": "plain_text", "content": title }, "template": "blue" },
                "elements": [{ "tag": "markdown", "content": content }],
            },
        })
    };
    post_and_expect_ok(webhook_url, payload, "Feishu").await
}

/// `sendWechatWork`：msgtype text/markdown。
async fn send_wechat_work(webhook_url: &str, content: &str, format: NotifyFormat) -> Result<(), String> {
    let payload = if format == NotifyFormat::Text {
        serde_json::json!({ "msgtype": "text", "text": { "content": content } })
    } else {
        serde_json::json!({ "msgtype": "markdown", "markdown": { "content": content } })
    };
    post_and_expect_ok(webhook_url, payload, "WeCom").await
}

/// `sendWebhook`：事件订阅过滤 + HMAC-SHA256 签名（`X-InkOS-Signature:
/// sha256={hex}`）+ POST JSON。
pub async fn send_webhook(url: &str, secret: Option<&str>, events: &[String], payload: &WebhookPayload) -> Result<(), String> {
    if !events.is_empty() && !events.iter().any(|event| event == &payload.event) {
        return Ok(());
    }
    let body = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();
    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.clone());
    if let Some(secret) = secret {
        let signature = hmac_sha256_hex(secret.as_bytes(), body.as_bytes());
        request = request.header("X-InkOS-Signature", format!("sha256={signature}"));
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Webhook POST to {url} failed: {} {body}", status.as_u16()));
    }
    Ok(())
}

/// HMAC-SHA256 → hex（TS `createHmac("sha256", secret).update(body).digest("hex")`）。
pub fn hmac_sha256_hex(secret: &[u8], body: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn post_and_expect_ok(url: &str, payload: Value, label: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("{label} send failed: {} {body}", status.as_u16()));
    }
    Ok(())
}

/// `dispatchNotification`：markdown/纯文本双形态按通道派发（webhook 型以
/// pipeline-complete 事件携带 title/body/format）。
pub async fn dispatch_notification(channels: &[NotifyChannel], message: &NotifyMessage) {
    let markdown_text = format!("**{}**\n\n{}", message.title, message.body);
    let plain_body = strip_markdown_marks(&message.body);
    let plain_text = format!("{}\n\n{}", message.title, plain_body);
    for channel in channels {
        let result = match channel {
            NotifyChannel::Telegram { bot_token, chat_id, format } => {
                let message_text = if *format == NotifyFormat::Text { &plain_text } else { &markdown_text };
                send_telegram(bot_token, chat_id, message_text, *format).await
            }
            NotifyChannel::Feishu { webhook_url, format } => {
                let content = if *format == NotifyFormat::Text { &plain_body } else { &message.body };
                send_feishu(webhook_url, &message.title, content, *format).await
            }
            NotifyChannel::WechatWork { webhook_url, format } => {
                let content = if *format == NotifyFormat::Text { &plain_text } else { &markdown_text };
                send_wechat_work(webhook_url, content, *format).await
            }
            NotifyChannel::Webhook { url, secret, events, format } => {
                send_webhook(
                    url,
                    secret.as_deref(),
                    events,
                    &WebhookPayload {
                        event: "pipeline-complete".to_string(),
                        book_id: String::new(),
                        chapter_number: None,
                        timestamp: crate::utils::utc_time::utc_now_iso(),
                        data: Some(serde_json::json!({
                            "title": message.title,
                            "body": message.body,
                            "format": format,
                        })),
                    },
                )
                .await
            }
        };
        if let Err(error) = result {
            eprintln!("[notify] failed: {error}");
        }
    }
}

/// `dispatchWebhookEvent`：结构化事件只发 webhook 型通道。
pub async fn dispatch_webhook_event(channels: &[NotifyChannel], payload: &WebhookPayload) {
    for channel in channels {
        if let NotifyChannel::Webhook { url, secret, events, .. } = channel {
            if let Err(error) = send_webhook(url, secret.as_deref(), events, payload).await {
                eprintln!("[webhook] {url} failed: {error}");
            }
        }
    }
}

/// 从 inkos.json 原始配置解析 notify 通道数组（非法条目整组忽略——保守侧）。
pub fn parse_notify_channels(raw: Option<&Value>) -> Vec<NotifyChannel> {
    raw.and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| serde_json::from_value::<NotifyChannel>(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_signature_matches_known_vector() {
        // RFC 4231 test case 1：key=0x0b*20, data="Hi There"。
        let key = vec![0x0bu8; 20];
        let signature = hmac_sha256_hex(&key, b"Hi There");
        assert_eq!(
            signature,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn notify_channel_parses_all_four_types() {
        let channels = parse_notify_channels(Some(&serde_json::json!([
            { "type": "telegram", "botToken": "t", "chatId": "c" },
            { "type": "feishu", "webhookUrl": "https://f.example/h" },
            { "type": "wechat-work", "webhookUrl": "https://w.example/h", "format": "text" },
            { "type": "webhook", "url": "https://h.example/cb", "secret": "s", "events": ["pipeline-complete"] },
        ])));
        assert_eq!(channels.len(), 4);
        assert!(matches!(&channels[0], NotifyChannel::Telegram { format: NotifyFormat::Markdown, .. }));
        assert!(matches!(&channels[2], NotifyChannel::WechatWork { format: NotifyFormat::Text, .. }));
        assert!(matches!(&channels[3], NotifyChannel::Webhook { secret: Some(_), events, .. } if events == &vec!["pipeline-complete".to_string()]));
        // 非法条目剔除、缺 notify → 空。
        assert!(parse_notify_channels(Some(&serde_json::json!([{ "type": "nope" }]))).is_empty());
        assert!(parse_notify_channels(None).is_empty());
    }

    #[test]
    fn webhook_payload_serializes_camel_case() {
        let payload = WebhookPayload {
            event: "pipeline-complete".to_string(),
            book_id: "b1".to_string(),
            chapter_number: Some(3),
            timestamp: "t".to_string(),
            data: None,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"chapterNumber\":3"), "{json}");
        assert!(!json.contains("data"), "{json}");
    }
}
