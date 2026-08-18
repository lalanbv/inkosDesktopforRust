//! state-validator —— settler 输出的真相文件一致性校验。
//!
//! 移植自 `packages/core/src/agents/state-validator.ts`（310 行）。对比新旧
//! truth files（状态卡 + 伏笔池）的行级 diff，经 LLM(0.1) 检查六类矛盾
//! （无叙事支撑的状态变更 / 漏记 / 时间不可能 / hook 异常 / 追溯编辑 /
//! 跨真相键位冲突）。
//!
//! 最小裁决协议（非 JSON）：首行 `PASS`/`FAIL`，后续行可选 `[category]` 前缀
//! 警告；也接受精确 JSON（`{passed, warnings[]}`）与**平衡对象提取**
//! （字符串感知的花括号配对 + 尾随内容拒收）。
//!
//! ## 移植纪律
//! - 系统提示词逐字（含 FAIL 仅限硬矛盾的 IMPORTANT 段）
//! - diff 是行集合差（非顺序敏感）；空 diff（文本相等或增删均空）跳过校验
//!   直接通过
//! - 警告行解析三分支：`[cat] desc` / `- desc`（`* ` 同）/ 长度 >5 的裸行

use async_trait::async_trait;

use crate::agents::continuity::ChatOutcome;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::utils::language::WritingLanguage;

/// agent 名。对齐 TS `StateValidatorAgent.name`。
pub const STATE_VALIDATOR_NAME: &str = "state-validator";

/// 校验警告。对齐 TS `ValidationWarning`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationWarning {
    pub category: String,
    pub description: String,
}

/// 校验结果。对齐 TS `ValidationResult`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub warnings: Vec<ValidationWarning>,
    pub passed: bool,
    /// 131 号（TS repairRequired）：首行裁决 REPAIR → true；JSON 路径显式
    /// `repairRequired === true` 才 true；跳过校验面恒 false。
    pub repair_required: bool,
}

/// 权威上下文（story_frame / book_rules / 章节摘要节选）。
#[derive(Debug, Clone, Default)]
pub struct StateValidationAuthorityContext {
    pub story_frame: Option<String>,
    pub book_rules: Option<String>,
    pub chapter_summaries: Option<String>,
}

/// LLM 聊天端口（temperature 0.1）。
#[async_trait]
pub trait StateValidatorChat: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String>;
}

#[derive(Debug, thiserror::Error)]
pub enum StateValidationError {
    #[error("LLM chat failed: {0}")]
    Chat(String),
    #[error("LLM returned empty response")]
    EmptyResponse,
    #[error("State validator returned invalid response")]
    InvalidResponse,
}

/// validate 入参。
pub struct ValidateParams<'a> {
    pub chapter_content: &'a str,
    pub chapter_number: u32,
    pub old_state: &'a str,
    pub new_state: &'a str,
    pub old_hooks: &'a str,
    pub new_hooks: &'a str,
    pub language: WritingLanguage,
    pub authority_context: Option<&'a StateValidationAuthorityContext>,
}

/// 校验主入口：diff → （空 diff 直接通过）→ LLM → 解析。
pub async fn validate(
    chat: &dyn StateValidatorChat,
    params: &ValidateParams<'_>,
) -> Result<ValidationResult, StateValidationError> {
    let state_diff = compute_diff(params.old_state, params.new_state, "State Card");
    let hooks_diff = compute_diff(params.old_hooks, params.new_hooks, "Hooks Pool");

    // 无变化跳过校验。
    if state_diff.is_none() && hooks_diff.is_none() {
        return Ok(ValidationResult { warnings: Vec::new(), passed: true, repair_required: false });
    }

    let lang_instruction = if params.language == WritingLanguage::En {
        "Respond in English."
    } else {
        "用中文回答。"
    };

    let system_prompt = build_system_prompt(lang_instruction);
    let user_prompt = build_user_prompt(
        params.chapter_number,
        state_diff.as_deref(),
        hooks_diff.as_deref(),
        params.authority_context,
        params.chapter_content,
    );

    let response = chat
        .chat(
            vec![
                LLMMessage { role: LLMRole::System, content: system_prompt, tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user_prompt, tool_calls: None, tool_call_id: None },
            ],
            0.1,
        )
        .await
        .map_err(StateValidationError::Chat)?;

    parse_result(&response.content)
}

/// 系统提示词。逐字移植（六类矛盾 + 输出协议 + FAIL 仅限硬矛盾）。
pub fn build_system_prompt(lang_instruction: &str) -> String {
    format!(
        "You are a continuity validator for a novel writing system. {lang_instruction}\n\nGiven the chapter text and the CHANGES made to truth files (state card + hooks pool), check for contradictions:\n\n1. State change without narrative support — truth file says something changed but the chapter text doesn't describe it\n2. Missing state change — chapter text describes something happening but the truth file didn't capture it\n3. Temporal impossibility — character moves locations without transition, injury heals without time passing\n4. Hook anomaly — a hook disappeared without being marked resolved, or a new hook has no basis in the chapter\n5. Retroactive edit — truth file change implies something happened in a PREVIOUS chapter, not the current one\n6. Cross-truth key-setting conflict — numbered rules, named laws, ranks, identities, locations, or relationship labels in the new truth files contradict the chapter text or the authority context\n\nOutput format (simple, NOT JSON):\n- First line: exactly PASS or FAIL (nothing else on this line)\n- Following lines: one warning per line, optionally prefixed with [category]\n- If no issues at all, just output: PASS\n\nExample:\nPASS\n[unsupported_change] State card says character moved to the forest, but text only shows intent\n[minor] Hook H03 advanced but text mention is brief\n\nOr if there are hard contradictions:\nFAIL\n[contradiction] State says character is dead but chapter text shows them speaking\n[unsupported_change] New location not mentioned anywhere in chapter text\n\nIMPORTANT: Output FAIL ONLY for hard contradictions — facts that directly conflict with the chapter text. Do NOT fail for:\n- Slightly ahead-of-text inferences\n- Missing details that the state card didn't capture\n- Reasonable extrapolations from text\n- Hook management differences that don't contradict text\nThese should be warnings with PASS, not FAIL."
    )
}

/// 用户提示词（权威块 + 双 diff + 正文参照）。golden 守门。
pub fn build_user_prompt(
    chapter_number: u32,
    state_diff: Option<&str>,
    hooks_diff: Option<&str>,
    authority_context: Option<&StateValidationAuthorityContext>,
    chapter_content: &str,
) -> String {
    let authority_block = build_authority_context_block(authority_context);
    format!(
        "Chapter {chapter_number} validation:\n\n{authority_block}\n\n## State Card Changes\n{}\n\n## Hooks Pool Changes\n{}\n\n## Chapter Text (for reference)\n{chapter_content}",
        state_diff.unwrap_or("(no changes)"),
        hooks_diff.unwrap_or("(no changes)"),
    )
}

/// 行级 diff：新增/删除行集合（非顺序敏感）。文本相等或增删均空 → None。
pub fn compute_diff(old_text: &str, new_text: &str, label: &str) -> Option<String> {
    if old_text == new_text {
        return None;
    }

    let old_lines: Vec<&str> = old_text.split('\n').filter(|l| !l.trim().is_empty()).collect();
    let new_lines: Vec<&str> = new_text.split('\n').filter(|l| !l.trim().is_empty()).collect();

    let added: Vec<&str> = new_lines
        .iter()
        .filter(|l| !old_lines.contains(l))
        .copied()
        .collect();
    let removed: Vec<&str> = old_lines
        .iter()
        .filter(|l| !new_lines.contains(l))
        .copied()
        .collect();

    if added.is_empty() && removed.is_empty() {
        return None;
    }

    let mut parts = vec![format!("### {label}")];
    if !removed.is_empty() {
        parts.push(format!(
            "Removed:\n{}",
            removed.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n")
        ));
    }
    if !added.is_empty() {
        parts.push(format!(
            "Added:\n{}",
            added.iter().map(|l| format!("+ {l}")).collect::<Vec<_>>().join("\n")
        ));
    }
    Some(parts.join("\n"))
}

/// 权威上下文块（优先级声明 + 三节选）。golden 守门。
pub fn build_authority_context_block(
    authority_context: Option<&StateValidationAuthorityContext>,
) -> String {
    let Some(context) = authority_context else {
        return "## Authority / Cross-Truth Context\n(no authority context provided)".to_string();
    };

    let story_frame = context.story_frame.as_deref().unwrap_or("").trim();
    let book_rules = context.book_rules.as_deref().unwrap_or("").trim();
    let chapter_summaries = context.chapter_summaries.as_deref().unwrap_or("").trim();

    [
        "## Authority / Cross-Truth Context",
        "Authority priority: current chapter text > runtime truth files/current summaries > story_frame/book_rules > legacy story_bible intro or marketing-style prose. If the current chapter establishes a numbered/name mapping, new truth files must follow that mapping instead of preserving an older intro-only version.",
        "",
        "### story_frame / legacy story_bible excerpt",
        if story_frame.is_empty() { "(empty)" } else { story_frame },
        "",
        "### book_rules excerpt",
        if book_rules.is_empty() { "(empty)" } else { book_rules },
        "",
        "### recent chapter_summaries excerpt",
        if chapter_summaries.is_empty() { "(empty)" } else { chapter_summaries },
    ]
    .join("\n")
}

/// 解析 LLM 裁决：JSON（精确/平衡提取）优先，否则 PASS/FAIL 行协议。
pub fn parse_result(content: &str) -> Result<ValidationResult, StateValidationError> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(StateValidationError::EmptyResponse);
    }

    if let Some(json_result) = try_parse_json_result(trimmed) {
        return Ok(json_result);
    }

    let lines: Vec<&str> = trimmed
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return Err(StateValidationError::EmptyResponse);
    }

    let verdict_line = lines[0];
    if !verdict_re().is_match(verdict_line) {
        return Err(StateValidationError::InvalidResponse);
    }
    let passed = pass_re().is_match(verdict_line);
    let repair_required = repair_re().is_match(verdict_line);

    let mut warnings: Vec<ValidationWarning> = Vec::new();
    for line in lines.iter().skip(1) {
        if verdict_re().is_match(line) {
            continue;
        }
        if let Some(captures) = category_re().captures(line) {
            warnings.push(ValidationWarning {
                category: captures.get(1).map(|m| m.as_str().trim()).unwrap_or("").to_string(),
                description: captures.get(2).map(|m| m.as_str().trim()).unwrap_or("").to_string(),
            });
        } else if line.starts_with("- ") || line.starts_with("* ") {
            warnings.push(ValidationWarning {
                category: "general".to_string(),
                description: line[2..].trim().to_string(),
            });
        } else if line.chars().count() > 5 {
            warnings.push(ValidationWarning {
                category: "general".to_string(),
                description: line.to_string(),
            });
        }
    }

    Ok(ValidationResult { warnings, passed, repair_required })
}

fn try_parse_json_result(text: &str) -> Option<ValidationResult> {
    if let Some(direct) = try_parse_exact_json_result(text) {
        return Some(direct);
    }
    let candidate = extract_balanced_json_object(text)?;
    try_parse_exact_json_result(&candidate)
}

fn try_parse_exact_json_result(text: &str) -> Option<ValidationResult> {
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    let passed = parsed.get("passed")?.as_bool()?;
    let repair_required = parsed.get("repairRequired").and_then(|v| v.as_bool()).unwrap_or(false);
    let warnings: Vec<ValidationWarning> = parsed
        .get("warnings")
        .and_then(|value| value.as_array().cloned())
        .map(|items| {
            items
                .iter()
                .map(|item| ValidationWarning {
                    category: item
                        .get("category")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    description: item
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(ValidationResult { warnings, passed, repair_required })
}

/// 字符串感知的平衡 JSON 对象提取；闭合括号后仅允许空白/结构终结符
/// （拒收 `{...} more text` 形态）。
pub fn extract_balanced_json_object(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let start = chars.iter().position(|&c| c == '{')?;

    let mut depth: i64 = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut end_index: Option<usize> = None;

    for (offset, &ch) in chars.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            continue;
        }
        if ch == '{' {
            depth += 1;
            continue;
        }
        if ch == '}' {
            depth -= 1;
            if depth == 0 {
                end_index = Some(offset);
                break;
            }
            if depth < 0 {
                return None;
            }
        }
    }
    let _ = end_index?;

    let end = end_index? + 1;
    if let Some(&ch) = chars.get(end) {
        if !matches!(ch, '\n' | '\r' | '\t' | ' ' | ',' | ']' | '}') {
            return None;
        }
    }

    Some(chars[start..end].iter().collect())
}

fn verdict_re() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)^(PASS|REPAIR|FAIL)$").unwrap())
}

fn pass_re() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)^PASS$").unwrap())
}

fn repair_re() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)^REPAIR$").unwrap())
}

fn category_re() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"^\[([^\]]+)\]\s*(.+)$").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockChat {
        content: String,
        calls: std::sync::Mutex<Vec<(String, f64)>>,
    }

    #[async_trait]
    impl StateValidatorChat for MockChat {
        async fn chat(&self, messages: Vec<LLMMessage>, temperature: f64) -> Result<ChatOutcome, String> {
            self.calls
                .lock()
                .unwrap()
                .push((messages[1].content.clone(), temperature));
            Ok(ChatOutcome { content: self.content.clone(), usage: None })
        }
    }

    #[tokio::test]
    async fn no_diff_skips_llm_and_passes() {
        let chat = MockChat { content: "FAIL".into(), calls: std::sync::Mutex::new(Vec::new()) };
        let result = validate(
            &chat,
            &ValidateParams {
                chapter_content: "正文",
                chapter_number: 3,
                old_state: "| a |",
                new_state: "| a |",
                old_hooks: "h",
                new_hooks: "h",
                language: WritingLanguage::Zh,
                authority_context: None,
            },
        )
        .await
        .unwrap();
        assert!(result.passed);
        assert!(result.warnings.is_empty());
        assert!(chat.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pass_protocol_with_categories() {
        let chat = MockChat {
            content: "PASS\n[unsupported_change] 状态卡说角色移动了，正文只有意图\n- 一般警告行\n短\nFAIL".into(),
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let result = validate(
            &chat,
            &ValidateParams {
                chapter_content: "正文",
                chapter_number: 3,
                old_state: "旧",
                new_state: "新",
                old_hooks: "h",
                new_hooks: "h",
                language: WritingLanguage::Zh,
                authority_context: None,
            },
        )
        .await
        .unwrap();
        assert!(result.passed);
        assert_eq!(result.warnings.len(), 2);
        assert_eq!(result.warnings[0].category, "unsupported_change");
        assert_eq!(result.warnings[1].category, "general");
        // 温度 0.1。
        assert_eq!(chat.calls.lock().unwrap()[0].1, 0.1);
    }

    #[tokio::test]
    async fn invalid_verdict_errors() {
        let chat = MockChat { content: "MAYBE".into(), calls: std::sync::Mutex::new(Vec::new()) };
        let error = validate(
            &chat,
            &ValidateParams {
                chapter_content: "x",
                chapter_number: 1,
                old_state: "a",
                new_state: "b",
                old_hooks: "c",
                new_hooks: "c",
                language: WritingLanguage::Zh,
                authority_context: None,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, StateValidationError::InvalidResponse));
    }

    #[test]
    fn compute_diff_line_set_semantics() {
        assert_eq!(compute_diff("a\nb", "a\nb", "L"), None);
        // 重排序（行集合相同）→ 无 diff。
        assert_eq!(compute_diff("a\nb", "b\na", "L"), None);
        let diff = compute_diff("a\nb", "a\nc", "State Card").unwrap();
        assert!(diff.starts_with("### State Card"));
        assert!(diff.contains("Removed:\n- b"));
        assert!(diff.contains("Added:\n+ c"));
        // 空白行不参与。
        assert_eq!(compute_diff("a\n\n", "a", "L"), None);
    }

    #[test]
    fn parse_json_variants() {
        let json = parse_result("{\"passed\":false,\"warnings\":[{\"category\":\"c1\",\"description\":\"d1\"}]}").unwrap();
        assert!(!json.passed);
        assert_eq!(json.warnings[0].category, "c1");

        // 嵌入文本中的平衡对象。
        let embedded = parse_result("前置说明\n{\"passed\":true,\"warnings\":[]}\n后置").unwrap();
        assert!(embedded.passed);

        // 尾随空格属结构终结符 → 接受（TS 同语义）。
        assert!(parse_result("{\"passed\":true} ").unwrap().passed);
        // 紧邻非结构字符 → 拒收 → 回退行协议 → 无效。
        assert!(parse_result("{\"passed\":true}more").is_err());
    }

    #[test]
    fn balanced_object_extraction_string_aware() {
        assert_eq!(
            extract_balanced_json_object("xx {\"a\":\"}{\"} yy"),
            Some("{\"a\":\"}{\"}".to_string())
        );
        assert_eq!(extract_balanced_json_object("no braces"), None);
        assert_eq!(extract_balanced_json_object("{unclosed"), None);
    }

    #[test]
    fn authority_block_matrix() {
        assert_eq!(
            build_authority_context_block(None),
            "## Authority / Cross-Truth Context\n(no authority context provided)"
        );
        let block = build_authority_context_block(Some(&StateValidationAuthorityContext::default()));
        assert!(block.contains("(empty)"));
        assert!(block.contains("Authority priority: current chapter text"));
        let block = build_authority_context_block(Some(&StateValidationAuthorityContext {
            story_frame: Some("框架".into()),
            book_rules: None,
            chapter_summaries: Some("摘要".into()),
        }));
        assert!(block.contains("### story_frame / legacy story_bible excerpt\n框架"));
        assert!(block.contains("### book_rules excerpt\n(empty)"));
        assert!(block.contains("### recent chapter_summaries excerpt\n摘要"));
    }
}
