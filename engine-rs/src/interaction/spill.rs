//! 结果 spill（R42，557 号）——dsh spill-policy 五条边界语义的本仓落地：
//! 超阈值工具结果不整块塞 LLM 消息，改为「头尾保留 + 全文落盘 + 可恢复
//! 通知」，防上下文烧穿与破坏性截断（agent_loop 8000 字符截断为兜底，
//! 阈值 6000 字节 < 8000 保证 spill 先行接管）。
//!
//! 对齐 dsh `spill-policy`/`spill-local`（544 号施工图验收规格）：
//! - **豁免防回环**：read/ls/grep 是读回通道工具，其结果永不 spill
//!   （read 读 spill 文件 → 结果超大 → 再 spill → 回环）。
//! - **通知预留进预算**：保留段 + GAP + 通知总长受阈值约束。
//! - **尽力而为**：落盘失败保留原结果并记日志，绝不因 spill 丢结果。
//! - **装载期校验**：阈值/保留量是编译期常量（上游为运行时配置校验）。
//! - 委托先行（PTC dispatch log）本仓无 PTC 面，不适用（备案）。
//!
//! 读回通道：spill 文件落 `<root>/.inkos/spills/session-<hash>/`（.inkos/
//! 已 gitignore、工件树惯例），read 工具对 `.inkos/spills/` 前缀放行
//! （`project_tools::tool_read_book` 分支，schema 面不变、差分器哈希不漂移）；
//! dsh 的 read offset/limit 分页本仓未立（备案后续专项，读回 8000 字符截断内
//! 有效）。

use sha2::{Digest, Sha256};

use crate::interaction::project_tools::ToolResult;
use crate::interaction::registry::ToolCtx;

/// 结果文本超过该 UTF-8 字节数触发 spill（< 8000 字符截断的最小字节量，
/// 保证任何场景下 spill 先于 agent_loop 破坏性截断接管）。
pub const SPILL_THRESHOLD_BYTES: usize = 6000;
/// 头部保留字节（字符边界切；开头信息密度最高）。
pub const SPILL_HEAD_BYTES: usize = 2800;
/// 尾部保留字节。
pub const SPILL_TAIL_BYTES: usize = 1600;
/// 读回通道工具豁免（防回环；dsh read 豁免同语义，ls/grep 同族扩展）。
pub const SPILL_EXEMPT_TOOLS: [&str; 3] = ["read", "ls", "grep"];

const GAP: &str = "\n\n[...]\n\n";

/// spill 判定（豁免/错误结果/阈值三维）。
pub fn should_spill(name: &str, res: &ToolResult) -> bool {
    if res.is_error || SPILL_EXEMPT_TOOLS.contains(&name) {
        return false;
    }
    res.text.len() > SPILL_THRESHOLD_BYTES
}

/// 会话作用域目录名（dsh sessionDir 同构：sha256 前 12 hex；无会话上下文
/// 落 `session-adhoc` 兜底目录）。
pub fn session_dir_name(session_id: Option<&str>) -> String {
    match session_id {
        Some(id) if !id.is_empty() => {
            let hex: String = Sha256::digest(id.as_bytes())[..6].iter().map(|b| format!("{b:02x}")).collect();
            format!("session-{hex}")
        }
        _ => "session-adhoc".to_string(),
    }
}

/// 字符边界安全切分（防 UTF-8 多字节劈半；dsh textSlice 的 surrogate 保护
/// 在 UTF-8 下等价于 char 边界对齐）。
fn split_at_char_boundary(text: &str, index: usize, from_start: bool) -> (&str, &str) {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    if from_start {
        (&text[..index], &text[index..])
    } else {
        let start = text.len() - index;
        let mut start = start;
        while start < text.len() && !text.is_char_boundary(start) {
            start += 1;
        }
        (&text[..start], &text[start..])
    }
}

/// 头尾保留切分：head 取前 `head_bytes` 字节（字符边界）、tail 取后
/// `tail_bytes` 字节。
pub fn split_head_tail(text: &str, head_bytes: usize, tail_bytes: usize) -> (String, String) {
    let (head, _) = split_at_char_boundary(text, head_bytes, true);
    let (_, tail) = split_at_char_boundary(text, tail_bytes, false);
    (head.to_string(), tail.to_string())
}

/// 落盘并返回通知文本（项目相对路径，read 工具前缀放行通道）。
/// 目录 0700、文件 wx+0600（独占写防碰撞；unix 权限面，windows 走缺省）。
async fn save_spill(root: &std::path::Path, session_id: Option<&str>, tool: &str, text: &str) -> Result<String, String> {
    let dir = root.join(".inkos").join("spills").join(session_dir_name(session_id));
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("create spill dir failed: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).await;
    }
    // 文件名：8 hex 随机 + 工具名（注册表白名单 [a-z_0-9]，防御性过滤）。
    let random = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let safe_tool: String = tool
        .chars()
        .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' { c } else { '_' })
        .collect();
    let file_name = format!("{random}-{safe_tool}.txt");
    let path = dir.join(&file_name);
    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .map_err(|e| format!("create spill file failed: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600)).await;
    }
    tokio::io::AsyncWriteExt::write_all(&mut tokio::io::BufWriter::new(file), text.as_bytes())
        .await
        .map_err(|e| format!("write spill file failed: {e}"))?;
    Ok(format!(
        ".inkos/spills/{}/{file_name}",
        session_dir_name(session_id)
    ))
}

/// spill 后的模型可见结果：头 + GAP + 尾 + 省略通知（可恢复引用）。
pub async fn spill_result(root: &std::path::Path, session_id: Option<&str>, tool: &str, res: ToolResult) -> ToolResult {
    let total = res.text.len();
    let (head, tail) = split_head_tail(&res.text, SPILL_HEAD_BYTES, SPILL_TAIL_BYTES);
    match save_spill(root, session_id, tool, &res.text).await {
        Ok(relative_path) => {
            let omitted = total - head.len() - tail.len();
            ToolResult {
                text: format!(
                    "{head}{GAP}{tail}\n\n(Omitted {omitted} bytes. Full result stored at: {relative_path}. \
Use the read tool on this path to view the full text.)"
                ),
                details: res.details,
                is_error: false,
            }
        }
        // 尽力而为：落盘失败保留原结果（dsh 降级语义——绝不因 spill 丢结果）。
        Err(reason) => {
            tracing::warn!(tool, reason = %reason, "spill save failed; keeping inline result");
            res
        }
    }
}

// ── post 钩子 ────────────────────────────────────────────────────────────

/// R42 spill 钩子（管线 post 段首消费者；豁免/错误/阈值判定不过即原样透传）。
pub struct SpillHook;

impl crate::interaction::pipeline::PostHook for SpillHook {
    fn post<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        res: ToolResult,
    ) -> futures_util::future::BoxFuture<'a, ToolResult> {
        Box::pin(async move {
            if !should_spill(name, &res) {
                return res;
            }
            spill_result(ctx.root, ctx.session_id, name, res).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::pipeline::PostHook;

    fn text_result(text: &str) -> ToolResult {
        ToolResult { text: text.to_string(), details: None, is_error: false }
    }

    #[test]
    fn should_spill_judges_threshold_exemption_and_errors() {
        let big = "x".repeat(SPILL_THRESHOLD_BYTES + 1);
        let small = "x".repeat(SPILL_THRESHOLD_BYTES);
        assert!(should_spill("research_web", &text_result(&big)));
        // 恰在阈值上不 spill。
        assert!(!should_spill("research_web", &text_result(&small)));
        // 豁免三件（读回通道）。
        assert!(!should_spill("read", &text_result(&big)));
        assert!(!should_spill("ls", &text_result(&big)));
        assert!(!should_spill("grep", &text_result(&big)));
        // 错误结果不 spill（排障信息保持内联）。
        let mut error = text_result(&big);
        error.is_error = true;
        assert!(!should_spill("research_web", &error));
    }

    #[test]
    fn session_dir_name_hashes_and_falls_back() {
        let name = session_dir_name(Some("sess-abc"));
        assert_eq!(name.len(), "session-0123456789ab".len());
        assert_eq!(name, session_dir_name(Some("sess-abc")), "同 id 同目录（确定性）");
        assert_ne!(name, session_dir_name(Some("sess-other")));
        assert_eq!(session_dir_name(None), "session-adhoc");
        assert_eq!(session_dir_name(Some("")), "session-adhoc");
    }

    #[test]
    fn split_head_tail_respects_char_boundaries() {
        let text = "汉".repeat(3000); // 每字 3 字节 = 9000 字节
        let (head, tail) = split_head_tail(&text, SPILL_HEAD_BYTES, SPILL_TAIL_BYTES);
        assert_eq!(head.chars().count(), SPILL_HEAD_BYTES / 3, "2800 字节 → 933 汉字（字符边界回退）");
        assert_eq!(tail.chars().count(), SPILL_TAIL_BYTES / 3);
        assert!(text.starts_with(&head));
        assert!(text.ends_with(&tail));
        // ASCII 场景精确切分。
        let ascii = "a".repeat(5000);
        let (head, tail) = split_head_tail(&ascii, SPILL_HEAD_BYTES, SPILL_TAIL_BYTES);
        assert_eq!(head.len(), SPILL_HEAD_BYTES);
        assert_eq!(tail.len(), SPILL_TAIL_BYTES);
    }

    #[tokio::test]
    async fn spill_result_writes_file_and_replaces_text() {
        let dir = tempfile::tempdir().unwrap();
        let big = "汉".repeat(3000); // 9000 字节 > 6000 阈值
        let spilled = spill_result(dir.path(), Some("sess-abc"), "research_web", text_result(&big)).await;
        assert!(!spilled.is_error);
        // 头尾保留 + GAP + 通知。
        assert!(spilled.text.starts_with("汉"));
        assert!(spilled.text.contains(GAP));
        assert!(spilled.text.contains("(Omitted "));
        assert!(spilled.text.contains("bytes. Full result stored at: .inkos/spills/"));
        assert!(spilled.text.contains("Use the read tool on this path"));
        // 文件存在且全文一致；相对路径可解析回真实文件。
        let anchored = spilled.text.find(".inkos/spills/").unwrap();
        let relative = spilled.text[anchored..]
            .split(". Use the read tool")
            .next()
            .unwrap();
        assert!(relative.ends_with("-research_web.txt"), "{relative}");
        let full = tokio::fs::read_to_string(dir.path().join(relative)).await.unwrap();
        assert_eq!(full, big, "spill 文件存全文（逐字）");
    }

    #[tokio::test]
    async fn spill_save_failure_keeps_inline_result() {
        // root 是常规文件 → create_dir_all 失败 → 降级保留原结果。
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("not-a-dir");
        std::fs::write(&root, "i am a file").unwrap();
        let big = "x".repeat(SPILL_THRESHOLD_BYTES + 100);
        let original = text_result(&big);
        let spilled = spill_result(&root, None, "research_web", original.clone()).await;
        assert_eq!(spilled.text, big, "落盘失败保留原文（尽力而为）");
        assert!(!spilled.is_error);
    }

    #[tokio::test]
    async fn spill_hook_end_to_end_via_post_trait() {
        let dir = tempfile::tempdir().unwrap();
        let root: &'static std::path::Path = Box::leak(Box::new(dir.path().to_path_buf()));
        let ctx = ToolCtx::root_only(root);
        let hook = SpillHook;
        let big = "x".repeat(SPILL_THRESHOLD_BYTES + 1);
        let out = hook.post(&ctx, "research_web", text_result(&big)).await;
        assert!(out.text.contains("(Omitted"), "钩子路径生效");
        let small = hook.post(&ctx, "read", text_result(&big)).await;
        assert_eq!(small.text, big, "豁免件透传原文");
    }
}
