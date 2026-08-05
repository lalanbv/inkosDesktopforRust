/// SSE 帧解析（纯函数，无 IO）。
///
/// 解析器按 SSE 规范将文本缓冲切分为完整帧（以空行 `\n\n` 分隔），
/// 并将不完整的末帧作为残余字符串返回，由调用方缓存拼接到下一次输入。
/// 本模块只负责解析 + SseClient 订阅；网络 IO 隔离在 `SseClient` 内。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

/// 解析 SSE 文本缓冲为事件列表。按空行(`\n\n`)分帧；每帧 `event:`/`data:` 行组装。
/// 不完整末帧（无 `\n\n` 结尾）作为残余字符串返回由调用方缓存——本函数只返回完整帧。
pub fn parse_sse_frame(buf: &str) -> (Vec<SseEvent>, String) {
    let mut out = Vec::new();
    let mut rest = buf.to_string();
    while let Some(idx) = rest.find("\n\n") {
        let frame = rest[..idx].to_string();
        rest = rest[idx + 2..].to_string();
        let mut ev = String::new();
        let mut data = String::new();
        for line in frame.lines() {
            if let Some(v) = line.strip_prefix("event:") {
                ev = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("data:") {
                data = v.trim().to_string();
            }
        }
        if !ev.is_empty() {
            out.push(SseEvent { event: ev, data });
        }
    }
    (out, rest)
}

/// SSE 客户端：持续订阅 inkos `/api/v1/events`，断线指数退避(1s..30s)重连。
///
/// shutdown 选用 `Arc<AtomicBool>`（避免引 tokio-util 依赖；AtomicBool 无异步通知能力，
/// 故 sleep 用 100ms 分段轮询实现可取消）。
/// 连接错误用 `eprintln!` 显式记录（不静默吞错）；handler 错误由 `Router::dispatch` 处理。
///
/// 单一职责：仅订阅 + 解析 + 路由；通知/角标由注入的 `Router` 表决定。
pub struct SseClient {
    url: String,
}

impl SseClient {
    pub fn new(url: String) -> Self {
        Self { url }
    }

    /// 持续订阅；连接错误指数退避(1s..30s)重连，直到 shutdown。
    /// 正常结束（stream 自然结束或 shutdown）重置 backoff 为 1s；错误时 backoff 倍增上限 30s。
    pub async fn run(
        &self,
        router: std::sync::Arc<crate::observer::router::Router>,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<()> {
        use std::sync::atomic::Ordering;
        use std::time::Duration;

        let mut backoff = Duration::from_secs(1);
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return Ok(());
            }
            let result = self.connect_once(&router, &shutdown).await;
            let was_error = result.is_err();
            if let Err(ref e) = result {
                eprintln!(
                    "[observer] SSE 连接错误 ({e:#}), {:?} 后重连",
                    backoff
                );
            }
            if shutdown.load(Ordering::Relaxed) {
                return Ok(());
            }
            // 可取消 sleep：AtomicBool 无异步通知，用 100ms 分段轮询 shutdown。
            cancellable_sleep(backoff, &shutdown).await;
            if shutdown.load(Ordering::Relaxed) {
                return Ok(());
            }
            // 仅错误路径倍增 backoff；Ok 路径重置为 1s 避免对优雅关闭过度惩罚。
            if was_error {
                backoff = (backoff * 2).min(Duration::from_secs(30));
            } else {
                backoff = Duration::from_secs(1);
            }
        }
    }

    /// 单次连接：建立 HTTP、读流、累积残余、解析分派。
    /// shutdown 触发立即返回 Ok(())；IO/解码错误向上传播（由 run 退避重连）。
    async fn connect_once(
        &self,
        router: &std::sync::Arc<crate::observer::router::Router>,
        shutdown: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<()> {
        use std::sync::atomic::Ordering;
        use tokio_stream::StreamExt;

        let client = reqwest::Client::builder().build()?;
        let resp = client.get(&self.url).send().await?.error_for_status()?;
        let mut bytes = resp.bytes_stream();
        let mut buf = String::new();
        while let Some(chunk) = bytes.next().await {
            if shutdown.load(Ordering::Relaxed) {
                return Ok(());
            }
            let chunk = chunk?;
            buf.push_str(std::str::from_utf8(&chunk)?);
            let (events, rest) = parse_sse_frame(&buf);
            buf = rest;
            for ev in events {
                router.dispatch(&ev);
            }
        }
        Ok(())
    }
}

/// 可取消 sleep：按 100ms 分段轮询 shutdown，触发则立即返回。
/// AtomicBool 无 `cancelled()` 类异步原语，分段轮询是规避 busy-poll 的标准做法。
fn cancellable_sleep(
    total: std::time::Duration,
    shutdown: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    Box::pin(async move {
        let chunk = Duration::from_millis(100);
        let mut elapsed = Duration::from_millis(0);
        while elapsed < total {
            if shutdown.load(Ordering::Relaxed) {
                return;
            }
            let step = chunk.min(total - elapsed);
            tokio::time::sleep(step).await;
            elapsed += step;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_complete_frame() {
        let (evs, rest) = parse_sse_frame("event: write:complete\ndata: {\"id\":1}\n\n");
        assert_eq!(
            evs,
            vec![SseEvent {
                event: "write:complete".into(),
                data: "{\"id\":1}".into()
            }]
        );
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
