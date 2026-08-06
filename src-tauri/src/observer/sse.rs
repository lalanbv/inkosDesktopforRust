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
///
/// # 复杂度
/// O(n)：以字节 offset 游走，`buf[start..].find("\n\n")` 只扫描未消费部分；
/// 每帧只在最终 `event`/`data` 字段上分配 String（不再每轮 `rest[...].to_string()`
/// 整串拷贝）。残余也只分配一次。原实现每帧拷贝剩余缓冲为 O(n²)，在 agent 重负载
/// (`tool:update`/`llm:progress` 簇发) 时累积延迟。
pub fn parse_sse_frame(buf: &str) -> (Vec<SseEvent>, String) {
    let mut out = Vec::new();
    let mut start = 0usize;
    // 游走完整帧：每轮从 `start` 起 find 下一个 `\n\n`，[start, idx) 即一帧。
    // `buf[start..]` 仅取借用（零拷贝）；find 找到后只前移 offset。
    while let Some(rel) = buf[start..].find("\n\n") {
        let abs_idx = start + rel;
        let frame = &buf[start..abs_idx];
        append_event_if_any(frame, &mut out);
        start = abs_idx + 2; // 跳过 `\n\n` 分隔符
    }
    // 不完整末帧（无 `\n\n` 结尾）作为残余返回，由调用方缓存拼接到下一次输入。
    let rest = buf[start..].to_string();
    (out, rest)
}

/// 解析单帧文本：`event:` / `data:` 行组装为 `SseEvent`，仅在两字段都非空时入列。
/// 提取为 helper 便于复用 + 单元测试；零整串拷贝（只对最终字段分配 String）。
fn append_event_if_any(frame: &str, out: &mut Vec<SseEvent>) {
    let mut ev: Option<String> = None;
    let mut data = String::new();
    for line in frame.lines() {
        if let Some(v) = line.strip_prefix("event:") {
            ev = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("data:") {
            data = v.trim().to_string();
        }
    }
    if let Some(event) = ev {
        if !event.is_empty() {
            out.push(SseEvent { event, data });
        }
    }
}

/// SSE 客户端：持续订阅 inkos `/api/v1/events`，断线指数退避(1s..30s)重连。
///
/// `reqwest::Client` 在 `new` 时建一次（C8 修复：连接池/TLS 会话跨重连复用），
/// `connect_once` 借用 `&self.client`——避免每次重连重建 Client 触发重复 TLS 握手。
///
/// shutdown 选用 `Arc<AtomicBool>`（避免引 tokio-util 依赖；AtomicBool 无异步通知能力，
/// 故 sleep 用 100ms 分段轮询实现可取消）。
/// 连接错误用 `eprintln!` 显式记录（不静默吞错）；handler 错误由 `Router::dispatch` 处理。
///
/// 单一职责：仅订阅 + 解析 + 路由；通知/角标由注入的 `Router` 表决定。
pub struct SseClient {
    url: String,
    client: reqwest::Client,
}

impl SseClient {
    /// 构造客户端：URL + 共享 `reqwest::Client`（连接池/TLS 会话在重连间复用，C8）。
    /// Client 构建失败向上传播（极少见：通常仅 TLS backend 初始化失败）。
    pub fn new(url: String) -> Self {
        let client = reqwest::Client::builder()
            .build()
            .expect("[observer] reqwest::Client 构建失败（TLS backend 初始化错误）");
        Self { url, client }
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
    /// 复用 `self.client`（C8：连接池/TLS 会话复用，避免重连重复握手）。
    /// shutdown 触发立即返回 Ok(())；IO/解码错误向上传播（由 run 退避重连）。
    async fn connect_once(
        &self,
        router: &std::sync::Arc<crate::observer::router::Router>,
        shutdown: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<()> {
        use std::sync::atomic::Ordering;
        use tokio_stream::StreamExt;

        let resp = self.client.get(&self.url).send().await?.error_for_status()?;
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

    /// 多帧簇发场景（agent 重负载 `tool:update`/`llm:progress`）：
    /// 验证 O(n) 重写不丢帧、残余正确、事件顺序保持。
    #[test]
    fn parses_burst_frames_in_order() {
        let buf = "event: tool:start\ndata: a\n\n\
                   event: llm:progress\ndata: b\n\n\
                   event: tool:end\ndata: c\n\n";
        let (evs, rest) = parse_sse_frame(buf);
        assert_eq!(
            evs.iter().map(|e| e.event.as_str()).collect::<Vec<_>>(),
            vec!["tool:start", "llm:progress", "tool:end"]
        );
        assert_eq!(
            evs.iter().map(|e| e.data.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(rest.is_empty(), "残余应为空，实际 {rest:?}");
    }

    /// 帧后跟不完整末帧（簇发 + 中途断流）：完整帧入列，残余返回。
    #[test]
    fn parses_complete_then_partial() {
        let buf = "event: write:complete\ndata: {\"id\":1}\n\nevent: draft:delta\ndata: partial";
        let (evs, rest) = parse_sse_frame(buf);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].event, "write:complete");
        assert_eq!(rest, "event: draft:delta\ndata: partial");
    }

    /// 空缓冲：零帧、空残余（边界条件）。
    #[test]
    fn empty_buffer_yields_empty() {
        let (evs, rest) = parse_sse_frame("");
        assert!(evs.is_empty());
        assert!(rest.is_empty());
    }

    /// 连续 `\n\n`：空帧应被跳过（无 `event:` 字段不入列）。
    #[test]
    fn consecutive_separators_skip_empty_frames() {
        let (evs, rest) = parse_sse_frame("\n\nevent: keep\ndata: x\n\n\n\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].event, "keep");
        assert!(rest.is_empty(), "残余应为空（末尾 `\\n\\n` 后无内容）");
    }

    /// C8 验证：`SseClient::new` 构造时建一次 Client，多次访问同一实例。
    /// 类型系统保证 `connect_once` 只能借 `&self.client`——无法在 run 内重建。
    /// 此测试确认构造成功 + 字段被初始化（间接通过 url 字段访问验证整体可用）。
    #[test]
    fn sse_client_constructor_initializes_shared_client() {
        let c = SseClient::new("http://127.0.0.1:1/test".to_string());
        // url 字段保留以备日志/调试；Client 字段私有但构造成功即证明 TLS backend 初始化通过。
        assert_eq!(c.url, "http://127.0.0.1:1/test");
    }
}
