//! R32 会话压缩（553 号）：轮间 threshold 压缩 + 溢出 compact-and-retry。
//!
//! 移植对齐 `packages/core/src/agent/session-compaction.ts`（与上游
//! pi-agent-core 0.87 harness structural.js/compaction.js 语义同构）：
//! - 触发 = usage 优先估算 > contextWindow − reserveTokens（threshold）；
//!   overflow 触发不做阈值检查（上游 prepareOverflowCompaction 同）
//! - 切点 = find_valid_cut_points/find_turn_start_index/find_cut_point 三函数
//!   移植（user 边界回落 + keepRecentTokens 尾部累积估算）
//! - 摘要 = 三 prompt 码点副本（golden r32-compaction-vectors.json 三方锁定）
//! - 落盘 = transcript Compaction 事件（firstKeptUuid 定位保留段），
//!   恢复端 session_restore::restore_committed_dialogue_scan 应用窗口
//!
//! 与上游的结构差异（备案，TS 侧同）：恢复产物 text-only，isSplitTurn 的
//! 轮前缀（turnPrefix）并入主摘要一次生成；estimateTokens 对 system role
//! 记 0（上游语义，恢复产物头部合成消息体量可忽略）。
//!
//! 非热路径：压缩是低频维护操作，估算 O(n) 字符遍历仅在有压缩事件发生时执行，
//! 不在 bench:gate 门禁面。

use std::path::Path;

use serde_json::Value;

use crate::interaction::session_restore::restore_committed_dialogue_scan;
use crate::interaction::session_transcript::{append_transcript_events, read_transcript_events, TranscriptEvent};
use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::LLMMessage;

/// 上游 `DEFAULT_COMPACTION_SETTINGS`（16384/20000，R32c 双端常量统一，
/// golden settings 键锁定）。
pub const DEFAULT_COMPACTION_SETTINGS: CompactionSettings = CompactionSettings {
    enabled: true,
    reserve_tokens: 16_384,
    keep_recent_tokens: 20_000,
};

/// 上游 `CompactionSettings`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompactionSettings {
    pub enabled: bool,
    pub reserve_tokens: u64,
    pub keep_recent_tokens: u64,
}

/// 摘要输出预算 = 0.8 × reserveTokens（上游 generateSummaryWithRequest 公式，
/// golden summaryMaxTokens 键锁定）。
pub const SUMMARY_MAX_TOKENS: u64 = 13_107;

/// TS 侧 COMPACTION_PROMPTS 的码点副本（机械提取自上游 dist 源码非手抄）。
pub const SUMMARIZATION_SYSTEM_PROMPT: &str = r####"You are a context summarization assistant. Your task is to read a conversation between a user and an AI assistant, then produce a structured summary following the exact format specified.

Do NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary."####;
pub const SUMMARIZATION_PROMPT: &str = r####"The messages above are a conversation to summarize. Create a structured context checkpoint summary that another LLM will use to continue the work.

Use this EXACT format:

## Goal
[What is the user trying to accomplish? Can be multiple items if the session covers different tasks.]

## Constraints & Preferences
- [Any constraints, preferences, or requirements mentioned by user]
- [Or "(none)" if none were mentioned]

## Progress
### Done
- [x] [Completed tasks/changes]

### In Progress
- [ ] [Current work]

### Blocked
- [Issues preventing progress, if any]

## Key Decisions
- **[Decision]**: [Brief rationale]

## Next Steps
1. [Ordered list of what should happen next]

## Critical Context
- [Any data, examples, or references needed to continue]
- [Or "(none)" if not applicable]

Keep each section concise. Preserve exact file paths, function names, and error messages."####;
pub const UPDATE_SUMMARIZATION_PROMPT: &str = r####"The messages above are NEW conversation messages to incorporate into the existing summary provided in <previous-summary> tags.

Update the existing structured summary with new information. RULES:
- PRESERVE all existing information from the previous summary
- ADD new progress, decisions, and context from the new messages
- UPDATE the Progress section: move items from "In Progress" to "Done" when completed
- UPDATE "Next Steps" based on what was accomplished
- PRESERVE exact file paths, function names, and error messages
- If something is no longer relevant, you may remove it

Use this EXACT format:

## Goal
[Preserve existing goals, add new ones if the task expanded]

## Constraints & Preferences
- [Preserve existing, add new ones discovered]

## Progress
### Done
- [x] [Include previously done items AND newly completed items]

### In Progress
- [ ] [Current work - update based on progress]

### Blocked
- [Current blockers - remove if resolved]

## Key Decisions
- **[Decision]**: [Brief rationale] (preserve all previous, add new)

## Next Steps
1. [Update based on current state]

## Critical Context
- [Preserve important context, add new if needed]

Keep each section concise. Preserve exact file paths, function names, and error messages."####;

/// 上下文 token 估算结果（usage 优先口径，上游 estimateContextTokens 同构）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextUsageEstimate {
    pub tokens: u64,
    pub usage_tokens: u64,
    pub trailing_tokens: u64,
    /// 提供 usage 的消息下标（无则 None）。
    pub last_usage_index: Option<usize>,
}

/// 单消息 token 估算（上游 estimateTokens 同构：content UTF-16 码元数 / 4，
/// system 记 0；本仓扫描消息只有 user/assistant 纯文本）。
pub fn estimate_tokens(message: &LLMMessage) -> u64 {
    // 对齐 JS `Math.ceil(chars / 4)`；chars 用 UTF-16 码元数（TS .length 口径，
    // crate::llm::provider::estimate_text_tokens 同源）。
    crate::llm::provider::estimate_text_tokens(&message.content) as u64
}

/// usage 有效性判定（上游 getAssistantUsage：stopReason 非 aborted/error 且
/// usage>0）。从 transcript message JSON 提取（TS 落盘完整 AssistantMessage）。
fn assistant_usage_from_raw(raw: &Value) -> Option<u64> {
    let stop_reason = raw.get("stopReason").and_then(Value::as_str)?;
    if stop_reason == "aborted" || stop_reason == "error" {
        return None;
    }
    let usage = raw.get("usage")?;
    let total = usage.get("totalTokens").and_then(Value::as_u64).unwrap_or(0);
    if total > 0 {
        return Some(total);
    }
    let sum: u64 = ["input", "output", "cacheRead", "cacheWrite"]
        .iter()
        .filter_map(|key| usage.get(*key).and_then(Value::as_u64))
        .sum();
    (sum > 0).then_some(sum)
}

/// usage 优先上下文估算（上游 estimateContextTokens 同构：最近有效 assistant
/// usage 记总量，其后消息退化估算）。
pub fn estimate_context_tokens(messages: &[LLMMessage], raws: &[Value]) -> ContextUsageEstimate {
    // raws 与 messages 平行（None 处记 Null——合成消息无 usage）。
    let mut last_usage_index = None;
    for (index, raw) in raws.iter().enumerate().rev() {
        if raw.is_null() {
            continue;
        }
        // 平行数组只对 assistant 携带原始 JSON。
        if messages.get(index).map(|m| m.role) == Some(crate::llm::provider::LLMRole::Assistant)
            && assistant_usage_from_raw(raw).is_some()
        {
            last_usage_index = Some(index);
            break;
        }
    }
    let Some(index) = last_usage_index else {
        let estimated: u64 = messages.iter().map(estimate_tokens).sum();
        return ContextUsageEstimate {
            tokens: estimated,
            usage_tokens: 0,
            trailing_tokens: estimated,
            last_usage_index: None,
        };
    };
    let usage_tokens =
        assistant_usage_from_raw(&raws[index]).expect("usage index validated above");
    let trailing_tokens: u64 = messages[index + 1..].iter().map(estimate_tokens).sum();
    ContextUsageEstimate {
        tokens: usage_tokens + trailing_tokens,
        usage_tokens,
        trailing_tokens,
        last_usage_index: Some(index),
    }
}

/// 上游 `shouldCompact`：`contextTokens > contextWindow − reserveTokens`。
pub fn should_compact(context_tokens: u64, context_window: u64, settings: &CompactionSettings) -> bool {
    if !settings.enabled {
        return false;
    }
    // 无效窗口不判定（对齐 TS shouldCompactSession 的短路防御）。
    if context_window == 0 {
        return false;
    }
    context_tokens > context_window.saturating_sub(settings.reserve_tokens)
}

/// 上游 `find_valid_cut_points`：合法切点 = user/assistant 消息（toolResult/
/// 其他不可切；本仓扫描消息仅 user/assistant/system，system 不合法同上游）。
fn find_valid_cut_points(raw_roles: &[Option<&str>], start_index: usize, end_index: usize) -> Vec<usize> {
    let mut cut_points = Vec::new();
    for index in start_index..end_index {
        if matches!(
            raw_roles.get(index).copied().flatten(),
            Some("user" | "assistant" | "bashExecution" | "custom" | "branchSummary" | "compactionSummary")
        ) {
            cut_points.push(index);
        }
    }
    cut_points
}

/// 上游 `find_turn_start_index`：回溯到包含该条目的轮次起始 user 消息。
fn find_turn_start_index(raw_roles: &[Option<&str>], entry_index: usize, start_index: usize) -> Option<usize> {
    (start_index..=entry_index)
        .rev()
        .find(|&index| raw_roles.get(index).copied().flatten() == Some("user"))
}

/// 切点结果（上游 CutPointResult）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutPointResult {
    pub first_kept_entry_index: usize,
    /// 切点分裂轮次时的轮首下标（否则 None）。
    pub turn_start_index: Option<usize>,
    pub is_split_turn: bool,
}

/// 上游 `findCutPoint` 移植：自尾累积估算 token 至 ≥ keepRecentTokens，
/// 取首个 ≥ 当前下标的合法切点（约 0GC：单次 O(n) 遍历，索引向量按需建）。
pub fn find_cut_point(
    raw_roles: &[Option<&str>],
    messages: &[LLMMessage],
    start_index: usize,
    end_index: usize,
    keep_recent_tokens: u64,
) -> CutPointResult {
    let cut_points = find_valid_cut_points(raw_roles, start_index, end_index);
    if cut_points.is_empty() {
        return CutPointResult {
            first_kept_entry_index: start_index,
            turn_start_index: None,
            is_split_turn: false,
        };
    }
    let mut accumulated_tokens: u64 = 0;
    let mut cut_index = cut_points[0];
    for index in (start_index..end_index).rev() {
        let Some(message) = messages.get(index) else { continue };
        // 上游只对 type:"message" 条目累积；本仓全部条目皆消息。
        accumulated_tokens += estimate_tokens(message);
        if accumulated_tokens >= keep_recent_tokens {
            for &point in &cut_points {
                if point >= index {
                    cut_index = point;
                    break;
                }
            }
            break;
        }
    }
    let is_user = raw_roles.get(cut_index).copied().flatten() == Some("user");
    let turn_start_index = if is_user {
        None
    } else {
        find_turn_start_index(raw_roles, cut_index, start_index)
    };
    CutPointResult {
        first_kept_entry_index: cut_index,
        turn_start_index,
        is_split_turn: !is_user && turn_start_index.is_some(),
    }
}

/// 上游 `serializeConversation` 码点同构（text-only 子集：user/assistant 文本块）。
pub fn serialize_conversation(messages: &[LLMMessage]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for message in messages {
        match message.role {
            crate::llm::provider::LLMRole::User => {
                if !message.content.is_empty() {
                    parts.push(format!("[User]: {}", message.content));
                }
            }
            crate::llm::provider::LLMRole::Assistant => {
                if !message.content.is_empty() {
                    parts.push(format!("[Assistant]: {}", message.content));
                }
            }
            _ => {}
        }
    }
    parts.join("\n\n")
}

/// 摘要 prompt 组装（上游 generateSummaryWithRequest 内联模板镜像，
/// golden promptAssemblyVectors 锁定）。
pub fn build_summary_prompt(serialized_conversation: &str, previous_summary: Option<&str>) -> String {
    let mut prompt_text = format!("<conversation>\n{serialized_conversation}\n</conversation>\n\n");
    if let Some(previous) = previous_summary {
        prompt_text.push_str(&format!("<previous-summary>\n{previous}\n</previous-summary>\n\n"));
    }
    prompt_text.push_str(if previous_summary.is_some() {
        UPDATE_SUMMARIZATION_PROMPT
    } else {
        SUMMARIZATION_PROMPT
    });
    prompt_text
}

/// 压缩扫描结果。
pub struct CompactionScan {
    pub messages: Vec<LLMMessage>,
    pub uuids: Vec<String>,
    pub active_compaction: Option<TranscriptEvent>,
    /// 与 messages 平行的原始 JSON（usage 提取；合成消息处为 Null）。
    pub raws: Vec<Value>,
}

/// 会话压缩扫描（compaction 窗口已应用）。system 合成消息不参与（扫描只含
/// committed text-only 对话；与 TS scanSessionForCompaction 同口径）。
pub async fn scan_session_for_compaction(
    project_root: &Path,
    session_id: &str,
    session_kind: Option<&str>,
) -> CompactionScan {
    let events = read_transcript_events(project_root, session_id).await;
    let scan = restore_committed_dialogue_scan(&events, session_kind);
    // 平行 raws：从窗口化 committed 事件取 assistant 原始 JSON。scan.messages
    // 与 scan.uuids 由 text_only 产物构造——重建平行关系：重走一次 committed
    // 映射（纯内存）。
    let mut raws: Vec<Value> = Vec::with_capacity(scan.uuids.len());
    {
        // restore_committed_dialogue_scan 内部的 committed 顺序 = uuids 顺序；
        // 这里按 uuid 精确回查 message JSON。
        use std::collections::HashMap;
        let mut by_uuid: HashMap<&str, &Value> = HashMap::new();
        for event in &events {
            if let TranscriptEvent::Message { uuid, message, role, .. } = event {
                if role != "toolResult" {
                    by_uuid.insert(uuid.as_str(), message);
                }
            }
        }
        for uuid in &scan.uuids {
            raws.push(by_uuid.get(uuid.as_str()).copied().cloned().unwrap_or(Value::Null));
        }
    }
    CompactionScan {
        messages: scan.messages,
        uuids: scan.uuids,
        active_compaction: scan.active_compaction,
        raws,
    }
}

/// threshold 判定（无效窗口短路同 TS）。
pub fn should_compact_session(scan: &CompactionScan, context_window: u64, settings: &CompactionSettings) -> bool {
    if !settings.enabled || scan.messages.is_empty() {
        return false;
    }
    if context_window == 0 {
        return false;
    }
    let usage = estimate_context_tokens(&scan.messages, &scan.raws);
    usage.tokens > 0 && should_compact(usage.tokens, context_window, settings)
}

/// 切点规划（isSplitTurn 的轮前缀并入主摘要——text-only 形态下分拆无增量
/// 价值，保留段语义不变；备案与上游 prepareCompaction 的差异）。
pub struct CompactionPlan {
    pub messages_to_summarize: Vec<LLMMessage>,
    pub first_kept_index: usize,
    pub tokens_before: u64,
}

pub fn plan_compaction(scan: &CompactionScan, settings: &CompactionSettings) -> Option<CompactionPlan> {
    if scan.messages.is_empty() || scan.uuids.len() != scan.messages.len() {
        return None;
    }
    let raw_roles: Vec<Option<&str>> = scan
        .messages
        .iter()
        .map(|message| match message.role {
            crate::llm::provider::LLMRole::User => Some("user"),
            crate::llm::provider::LLMRole::Assistant => Some("assistant"),
            _ => Some("system"),
        })
        .collect();
    let cut = find_cut_point(
        &raw_roles,
        &scan.messages,
        0,
        scan.messages.len(),
        settings.keep_recent_tokens,
    );
    if cut.first_kept_entry_index == 0 {
        return None;
    }
    Some(CompactionPlan {
        messages_to_summarize: scan.messages[..cut.first_kept_entry_index].to_vec(),
        first_kept_index: cut.first_kept_entry_index,
        tokens_before: estimate_context_tokens(&scan.messages, &scan.raws).tokens,
    })
}

/// 压缩产出（落盘 Compaction 事件）。
pub struct CompactionOutcome {
    pub summary: String,
    pub first_kept_uuid: Option<String>,
    pub tokens_before: u64,
    pub trigger: &'static str,
}

/// 单次压缩尝试（TS maybeCompactSession 同构）：扫描→判定→切点→摘要→落盘。
/// 任一步失败返回 None 放行现状（压缩是维护性优化，绝不阻断会话）。
pub async fn maybe_compact_session(
    project_root: &Path,
    session_id: &str,
    session_kind: Option<&str>,
    request_id: &str,
    router: &AgentRouter,
    trigger: &'static str,
) -> Option<CompactionOutcome> {
    let scan = scan_session_for_compaction(project_root, session_id, session_kind).await;
    if trigger == "threshold" && !should_compact_session(&scan, router.default_context_window(), &DEFAULT_COMPACTION_SETTINGS) {
        return None;
    }
    let plan = plan_compaction(&scan, &DEFAULT_COMPACTION_SETTINGS)?;
    let previous_summary = match &scan.active_compaction {
        Some(TranscriptEvent::Compaction { summary, .. }) => Some(summary.as_str()),
        _ => None,
    };

    // 摘要调用：router.chat 一次性请求（无工具），系统提示+组装模板走码点
    // 副本；maxTokens=SUMMARY_MAX_TOKENS（0.8×reserveTokens）。
    let serialized = serialize_conversation(&plan.messages_to_summarize);
    let user_prompt = build_summary_prompt(&serialized, previous_summary);
    let summary_messages = vec![
        LLMMessage { role: crate::llm::provider::LLMRole::System, content: SUMMARIZATION_SYSTEM_PROMPT.to_string(), tool_calls: None, tool_call_id: None },
        LLMMessage { role: crate::llm::provider::LLMRole::User, content: user_prompt, tool_calls: None, tool_call_id: None },
    ];
    let completion = router
        .chat_maintenance(summary_messages, SUMMARY_MAX_TOKENS as u32)
        .await
        .ok()?;
    let summary_text = completion.content.trim().to_string();
    if summary_text.is_empty() {
        return None;
    }

    let first_kept_uuid = scan.uuids.get(plan.first_kept_index).cloned();
    let outcome = CompactionOutcome {
        summary: summary_text,
        first_kept_uuid,
        tokens_before: plan.tokens_before,
        trigger,
    };
    let summary = outcome.summary.clone();
    let first_kept_uuid = outcome.first_kept_uuid.clone();
    let tokens_before = outcome.tokens_before;
    let trigger = outcome.trigger;
    let session_id_owned = session_id.to_string();
    let events = append_transcript_events(project_root, session_id, {
        let request_id = request_id.to_string();
        move |_events, next_seq| {
        vec![TranscriptEvent::Compaction {
            parent_seq: None,
            version: 1,
            session_id: session_id_owned,
            request_id,
            seq: next_seq,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            summary,
            first_kept_uuid,
            tokens_before,
            trigger: trigger.to_string(),
        }]
        }
    })
    .await;
    events
        .iter()
        .find_map(|event| match event {
            TranscriptEvent::Compaction { summary, first_kept_uuid, tokens_before, trigger, .. } => {
                Some(CompactionOutcome {
                    summary: summary.clone(),
                    first_kept_uuid: first_kept_uuid.clone(),
                    tokens_before: *tokens_before,
                    trigger: match trigger.as_str() {
                        "overflow" => "overflow",
                        _ => "threshold",
                    },
                })
            }
            _ => None,
        })
        .or(Some(outcome))
}

#[cfg(test)]
mod compaction_tests {
    use super::*;

    fn user_msg(content: &str) -> LLMMessage {
        LLMMessage { role: crate::llm::provider::LLMRole::User, content: content.into(), tool_calls: None, tool_call_id: None }
    }

    fn assistant_msg(content: &str) -> LLMMessage {
        LLMMessage { role: crate::llm::provider::LLMRole::Assistant, content: content.into(), tool_calls: None, tool_call_id: None }
    }

    #[test]
    fn settings_match_ts_constants() {
        assert_eq!(
            DEFAULT_COMPACTION_SETTINGS,
            CompactionSettings { enabled: true, reserve_tokens: 16_384, keep_recent_tokens: 20_000 }
        );
        assert_eq!(SUMMARY_MAX_TOKENS, 13_107);
    }

    #[test]
    fn estimate_tokens_utf16_ascii() {
        assert_eq!(estimate_tokens(&user_msg("tail-of-context")), 4);
        assert_eq!(estimate_tokens(&user_msg("u1")), 1);
    }

    #[test]
    fn estimate_context_usage_priority() {
        let messages = vec![
            user_msg("u1"),
            assistant_msg("a1"),
            user_msg("tail-of-context"),
        ];
        let raws = vec![
            Value::Null,
            serde_json::json!({ "stopReason": "stop", "usage": { "totalTokens": 120000 } }),
            Value::Null,
        ];
        let usage = estimate_context_tokens(&messages, &raws);
        assert_eq!(usage.tokens, 120_004);
        assert_eq!(usage.usage_tokens, 120_000);
        assert_eq!(usage.trailing_tokens, 4);
        assert_eq!(usage.last_usage_index, Some(1));
    }

    #[test]
    fn estimate_context_falls_back_to_estimate() {
        let messages = vec![user_msg("u1x"), assistant_msg("resp"), user_msg("tail msg here")];
        let raws = vec![
            Value::Null,
            serde_json::json!({ "stopReason": "stop", "usage": { "totalTokens": 0, "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 } }),
            Value::Null,
        ];
        let usage = estimate_context_tokens(&messages, &raws);
        assert_eq!(usage.tokens, 6);
        assert_eq!(usage.usage_tokens, 0);
        assert_eq!(usage.last_usage_index, None);
    }

    #[test]
    fn should_compact_matches_ts() {
        assert!(should_compact(120_004, 128_000, &DEFAULT_COMPACTION_SETTINGS));
        assert!(!should_compact(90_004, 128_000, &DEFAULT_COMPACTION_SETTINGS));
        assert!(!should_compact(999_999, 128_000, &CompactionSettings { enabled: false, ..DEFAULT_COMPACTION_SETTINGS }));
        assert!(!should_compact(999_999, 0, &DEFAULT_COMPACTION_SETTINGS));
    }

    fn roles_of(messages: &[LLMMessage]) -> Vec<Option<&'static str>> {
        messages
            .iter()
            .map(|m| match m.role {
                crate::llm::provider::LLMRole::User => Some("user"),
                crate::llm::provider::LLMRole::Assistant => Some("assistant"),
                _ => Some("system"),
            })
            .collect()
    }

    #[test]
    fn cut_point_splits_turn_back_to_user() {
        let messages = vec![
            user_msg("aaaa"),
            assistant_msg(&"b".repeat(400)),
            user_msg("cccc"),
            assistant_msg(&"d".repeat(400)),
            user_msg("eeee"),
        ];
        let roles = roles_of(&messages);
        let cut = find_cut_point(&roles, &messages, 0, messages.len(), 150);
        assert_eq!(cut.first_kept_entry_index, 1);
        assert_eq!(cut.turn_start_index, Some(0));
        assert!(cut.is_split_turn);
    }

    #[test]
    fn cut_point_small_history_keeps_all() {
        let messages = vec![user_msg("a"), assistant_msg("b")];
        let roles = roles_of(&messages);
        let cut = find_cut_point(&roles, &messages, 0, messages.len(), 10);
        assert_eq!(cut.first_kept_entry_index, 0);
        assert!(!cut.is_split_turn);
    }

    #[test]
    fn cut_point_at_zero_keeps_all() {
        let messages = vec![assistant_msg(&"a".repeat(400)), assistant_msg("bb")];
        let roles = roles_of(&messages);
        let cut = find_cut_point(&roles, &messages, 0, messages.len(), 50);
        assert_eq!(cut.first_kept_entry_index, 0);
        assert_eq!(cut.turn_start_index, None);
    }

    #[test]
    fn serialize_matches_ts_format() {
        let messages = vec![
            user_msg("你好"),
            assistant_msg("World reply"),
            user_msg("Second question"),
        ];
        assert_eq!(
            serialize_conversation(&messages),
            "[User]: 你好\n\n[Assistant]: World reply\n\n[User]: Second question"
        );
    }

    #[test]
    fn summary_prompt_assembly() {
        let first = build_summary_prompt("[User]: u1\n\n[Assistant]: a1", None);
        assert!(first.starts_with("<conversation>\n[User]: u1\n\n[Assistant]: a1\n</conversation>\n\n"));
        assert!(first.ends_with(SUMMARIZATION_PROMPT));
        let iterative = build_summary_prompt("[User]: u2", Some("PREV"));
        assert!(iterative.contains("<previous-summary>\nPREV\n</previous-summary>\n\n"));
        assert!(iterative.ends_with(UPDATE_SUMMARIZATION_PROMPT));
    }
}
