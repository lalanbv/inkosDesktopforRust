/// SSE 帧解析（纯函数，无 IO）。
///
/// 解析器按 SSE 规范将文本缓冲切分为完整帧（以空行 `\n\n` 分隔），
/// 并将不完整的末帧作为残余字符串返回，由调用方缓存拼接到下一次输入。
/// 本模块只负责解析，不涉及任何网络/文件 IO——SseClient 在 Task 4 实现。
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
