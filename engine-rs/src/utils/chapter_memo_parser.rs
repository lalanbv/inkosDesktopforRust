//! 章节规划备忘录解析器。
//!
//! 移植自 `packages/core/src/utils/chapter-memo-parser.ts`（160 行）。
//!
//! 解析 LLM planner 产出的 markdown 备忘录：剥包裹代码栅栏与前导散文 → 提取目标/
//! 关联线索/各必备小节 → 严格校验（缺小节/空小节抛 [`PlannerParseError`]）→
//! 组装 [`ChapterMemo`]。
//!
//! ## 移植要点
//! - 5 个正则用 `regex` crate + `OnceLock` 编译一次（非热路径，可接受）
//! - thread-ref 的 `\b` 用 `(?-u)` 切回 ASCII 语义（对齐 JS 无 /u 标志的 \b）
//! - `minContentChars` 与 goal 显示截断按 **UTF-16 码元** 计数（对齐 JS `.length`/`slice`）

use crate::models::input_governance::ChapterMemo;
use crate::utils::language::utf16_len;
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

/// 解析失败错误。`0` 字段为可读消息（对齐 TS `PlannerParseError.message`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerParseError(pub String);

impl std::fmt::Display for PlannerParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PlannerParseError {}

/// 必备小节定义（zh/en 标题对 + 最小内容字符数）。逐字移植 TS `REQUIRED_SECTIONS`。
struct RequiredSection {
    zh: &'static str,
    en: &'static str,
    min_content_chars: usize,
}

const REQUIRED_SECTIONS: &[RequiredSection] = &[
    RequiredSection { zh: "## 当前任务", en: "## Current task", min_content_chars: 20 },
    RequiredSection { zh: "## 读者此刻在等什么", en: "## What the reader is waiting for right now", min_content_chars: 20 },
    RequiredSection { zh: "## 该兑现的 / 暂不掀的", en: "## To pay off / to keep buried", min_content_chars: 20 },
    RequiredSection { zh: "## 日常/过渡承担什么任务", en: "## What the slow / transitional beats carry", min_content_chars: 20 },
    RequiredSection { zh: "## 关键抉择过三连问", en: "## Three-question check on the key choice", min_content_chars: 20 },
    RequiredSection { zh: "## 章尾必须发生的改变", en: "## Required end-of-chapter change", min_content_chars: 20 },
    RequiredSection { zh: "## 本章 hook 账", en: "## Hook ledger for this chapter", min_content_chars: 20 },
    RequiredSection { zh: "## 不要做", en: "## Do not", min_content_chars: 1 },
];

const GOAL_HEADINGS: &[&str] = &["## 本章目标", "## Chapter goal"];
const THREAD_HEADINGS: &[&str] = &["## 关联线索", "## Thread refs", "## Related threads"];

// ---- 编译一次的正则（OnceLock 缓存）----

fn next_h2_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n##\s").unwrap())
}
fn ws_collapse_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\s+").unwrap())
}
fn fence_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^```(?:md|markdown)?\s*\n([\s\S]*?)\n```\s*$").unwrap())
}
fn goal_split_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n|。|\. ").unwrap())
}
fn thread_ref_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // (?-u)：切回 ASCII \b/\w 语义，对齐 JS 无 /u 标志的 \b
    R.get_or_init(|| Regex::new(r"(?-u)\b[A-Za-z][A-Za-z0-9_-]*\d[A-Za-z0-9_-]*\b").unwrap())
}
fn empty_marker_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(无|none|n/a|na|—|-|\(none\))$").unwrap())
}

// ---- UTF-16 感知工具（对齐 JS .length / slice）----
// utf16_len 已提升至 crate::utils::language（多域共用的 JS parity 基准）。

/// 取前 `n` 个 UTF-16 码元（不劈开代理对；JS slice 会劈，但规划目标不含 emoji，实际等价）。
fn utf16_take(s: &str, n: usize) -> String {
    let mut units = 0usize;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let len = c.len_utf16();
        if units + len > n {
            break;
        }
        out.push(c);
        units += len;
    }
    out
}

// ---- 辅助函数（逐个对齐 TS）----

/// 提取 heading 到下一个 H2（或正文末）之间的内容，折叠空白。heading 不含。
fn extract_section_content(body: &str, heading: &str) -> String {
    let Some(start) = body.find(heading) else {
        return String::new();
    };
    let after = &body[start + heading.len()..];
    let section_raw = match next_h2_re().find(after) {
        Some(m) => &after[..m.start()],
        None => after,
    };
    ws_collapse_re().replace_all(section_raw, " ").trim().to_string()
}

fn strip_wrapping_fence(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(caps) = fence_re().captures(trimmed) {
        caps.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_else(|| trimmed.to_string())
    } else {
        trimmed.to_string()
    }
}

fn drop_leading_prose(raw: &str) -> String {
    // markers 含 GOAL/THREAD/REQUIRED 各标题 + 章首标记；找最早出现位置切片
    let mut min_idx: Option<usize> = None;
    let markers = ["# 第 ", "# Chapter "]
        .into_iter()
        .chain(GOAL_HEADINGS.iter().copied())
        .chain(THREAD_HEADINGS.iter().copied())
        .chain(REQUIRED_SECTIONS.iter().flat_map(|s| [s.zh, s.en]));
    for m in markers {
        if let Some(i) = raw.find(m) {
            min_idx = Some(min_idx.map_or(i, |cur| cur.min(i)));
        }
    }
    match min_idx {
        Some(i) => raw[i..].trim().to_string(),
        None => raw.trim().to_string(),
    }
}

fn extract_any_heading(body: &str, headings: &[&str]) -> String {
    for h in headings {
        let content = extract_section_content(body, h);
        if !content.is_empty() {
            return content;
        }
    }
    String::new()
}

fn extract_goal(body: &str) -> String {
    let explicit = extract_any_heading(body, GOAL_HEADINGS);
    if !explicit.is_empty() {
        let first = goal_split_re().split(&explicit).next().unwrap_or("");
        return first.trim().to_string();
    }
    String::new()
}

fn extract_thread_refs(body: &str) -> Vec<String> {
    let block = extract_any_heading(body, THREAD_HEADINGS);
    let trimmed = block.trim();
    if trimmed.is_empty() || empty_marker_re().is_match(trimmed) {
        return Vec::new();
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for m in thread_ref_re().find_iter(&block) {
        let s = m.as_str().to_string();
        if seen.insert(s.clone()) {
            out.push(s);
        }
    }
    out
}

fn extract_memo_body(markdown: &str) -> String {
    let mut min_start: Option<usize> = None;
    for s in REQUIRED_SECTIONS {
        for h in [s.zh, s.en] {
            if let Some(i) = markdown.find(h) {
                min_start = Some(match min_start {
                    Some(m) => m.min(i),
                    None => i,
                });
            }
        }
    }
    match min_start {
        Some(i) => markdown[i..].trim().to_string(),
        None => markdown.trim().to_string(),
    }
}

fn make_display_goal(goal: &str) -> String {
    if utf16_len(goal) <= 50 {
        return goal.to_string();
    }
    let taken = utf16_take(goal, 47);
    let trimmed = taken.trim_end();
    format!("{trimmed}...")
}

fn prepend_full_goal_if_needed(markdown: &str, body: &str, full_goal: &str, display_goal: &str) -> String {
    if full_goal == display_goal {
        return body.to_string();
    }
    let heading = if markdown.contains("## Chapter goal") {
        "## Chapter goal"
    } else {
        "## 本章目标"
    };
    format!("{heading}\n{full_goal}\n\n{body}")
}

/// 解析 planner memo（LLM 产出）。
///
/// 严格校验 LLM 拥有的必备小节（缺/空均抛错）；caller 拥有的字段（chapter /
/// is_golden_opening）由入参传入而非取自模型。长目标保留全文进 body，仅 schema
/// 字段截短为 ≤50 码元的显示标签。
pub fn parse_memo(raw: &str, expected_chapter: u32, is_golden_opening: bool) -> Result<ChapterMemo, PlannerParseError> {
    let markdown = drop_leading_prose(&strip_wrapping_fence(raw));
    let goal = extract_goal(&markdown);
    let body = extract_memo_body(&markdown);
    let thread_refs = extract_thread_refs(&markdown);

    if goal.is_empty() {
        return Err(PlannerParseError("goal must be a non-empty string".to_string()));
    }
    let display_goal = make_display_goal(&goal);

    // 缺失小节检测
    let missing: Vec<&str> = REQUIRED_SECTIONS
        .iter()
        .filter(|s| !body.contains(s.zh) && !body.contains(s.en))
        .map(|s| s.zh)
        .collect();
    if !missing.is_empty() {
        return Err(PlannerParseError(format!("missing sections: {}", missing.join(", "))));
    }

    // 空小节检测（内容 < minContentChars）
    let empty: Vec<String> = REQUIRED_SECTIONS
        .iter()
        .filter_map(|s| {
            let heading = if body.contains(s.zh) { s.zh } else { s.en };
            let content = extract_section_content(&body, heading);
            if utf16_len(&content) < s.min_content_chars {
                Some(format!("{} (need ≥ {} chars)", s.zh, s.min_content_chars))
            } else {
                None
            }
        })
        .collect();
    if !empty.is_empty() {
        return Err(PlannerParseError(format!("empty sections: {}", empty.join(", "))));
    }

    let final_body = prepend_full_goal_if_needed(&markdown, &body, &goal, &display_goal);
    Ok(ChapterMemo {
        chapter: expected_chapter,
        goal: display_goal,
        is_golden_opening,
        body: final_body,
        thread_refs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_memo(goal: &str) -> String {
        // 构造一个所有必备小节都满足 minContentChars（≥20 码元）的合法 memo
        format!(
            "## 本章目标\n{goal}\n\n## 当前任务\n推进主角觉醒系统面板，并完成第一次战斗场景。\n\n## 读者此刻在等什么\n等待主角如何应对突如其来的危机，以及力量的边界。\n\n## 该兑现的 / 暂不掀的\n兑现：系统面板功能；暂不掀：幕后黑手身份。\n\n## 日常/过渡承担什么任务\n用早餐场景建立主角与同伴的关系，埋下后续冲突的种子。\n\n## 关键抉择过三连问\n是否暴露能力？是否信任同伴？是否追击敌人？\n\n## 章尾必须发生的改变\n主角公开表明自己的身份，世界对他的态度彻底转变。\n\n## 本章 hook 账\n埋伏：神秘符文；呼唤：未完成的誓言；悬念：暗处窥视者。\n\n## 不要做\n无\n"
        )
    }

    #[test]
    fn parses_valid_memo() {
        let raw = full_memo("主角觉醒系统并击退敌人");
        let m = parse_memo(&raw, 3, false).unwrap();
        assert_eq!(m.chapter, 3);
        assert_eq!(m.goal, "主角觉醒系统并击退敌人");
        assert!(!m.is_golden_opening);
        assert!(m.body.contains("## 当前任务"));
    }

    #[test]
    fn missing_section_errors() {
        // 去掉「## 章尾必须发生的改变」整段
        let raw = full_memo("目标").replace(
            "## 章尾必须发生的改变\n主角公开表明自己的身份，世界对他的态度彻底转变。\n\n",
            "",
        );
        let err = parse_memo(&raw, 1, false).unwrap_err();
        assert!(err.0.contains("missing sections"), "实际: {}", err.0);
        assert!(err.0.contains("## 章尾必须发生的改变"));
    }

    #[test]
    fn empty_section_errors() {
        // 「## 当前任务」内容过短（< 20 码元）
        let raw = full_memo("目标").replace("推进主角觉醒系统面板，并完成第一次战斗场景。", "短");
        let err = parse_memo(&raw, 1, false).unwrap_err();
        assert!(err.0.contains("empty sections"), "实际: {}", err.0);
        assert!(err.0.contains("## 当前任务"));
    }

    #[test]
    fn empty_goal_errors() {
        // 无目标小节
        let raw = full_memo("目标").replace("## 本章目标\n目标\n\n", "");
        let err = parse_memo(&raw, 1, false).unwrap_err();
        assert_eq!(err.0, "goal must be a non-empty string");
    }

    #[test]
    fn long_goal_truncated_to_50_with_ellipsis() {
        let long_goal = "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十"; // 40 chars, lengthen
        let longer = format!("{long_goal}{long_goal}"); // 80 chars
        let raw = full_memo(&longer);
        let m = parse_memo(&raw, 1, false).unwrap();
        assert!(utf16_len(&m.goal) <= 50, "goal 应 ≤50 码元，实际 {} ({})", utf16_len(&m.goal), m.goal);
        assert!(m.goal.ends_with("..."));
        // body 保留完整目标
        assert!(m.body.contains(&longer));
    }

    #[test]
    fn thread_refs_deduped() {
        let mut raw = full_memo("目标");
        raw.push_str("\n\n## 关联线索\nT1 T2 T1 FOO3\n");
        let m = parse_memo(&raw, 1, false).unwrap();
        assert_eq!(m.thread_refs, vec!["T1".to_string(), "T2".into(), "FOO3".into()]);
    }

    #[test]
    fn thread_refs_none_marker_yields_empty() {
        let mut raw = full_memo("目标");
        raw.push_str("\n\n## 关联线索\n无\n");
        let m = parse_memo(&raw, 1, false).unwrap();
        assert!(m.thread_refs.is_empty());
    }

    #[test]
    fn strips_wrapping_fence_and_leading_prose() {
        let inner = full_memo("目标");
        let raw = format!("好的，下面是第 3 章规划：\n```md\n{inner}\n```");
        let m = parse_memo(&raw, 3, true).unwrap();
        assert_eq!(m.chapter, 3);
        assert!(m.is_golden_opening);
        assert_eq!(m.goal, "目标");
    }
}
