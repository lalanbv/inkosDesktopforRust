//! 叙事控制文本净化与渲染。
//!
//! 移植自 `packages/core/src/utils/narrative-control.ts`（177 行，纯字符串处理）。
//! ChapterIntent/ContextPackage 用最小签名（Option<&str> / ContextSourceRef）替代，
//! 待 input-governance 补全 ChapterIntent/ContextPackage 后可换强类型。

use crate::models::input_governance::ChapterMemo;
use crate::utils::language::WritingLanguage;
use regex::Regex;
use std::sync::OnceLock;

fn hook_id_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\bH\d+\b").unwrap())
}
fn hook_slug_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b[a-z]+(?:-[a-z]+){1,3}\b").unwrap())
}
fn chapter_ref_word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\bch(?:apter)?\s*\d+\b").unwrap())
}
fn chapter_ref_zh_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"第\s*\d+\s*章").unwrap())
}
fn evidence_source_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^-\s+(?:story|runtime)/[^:\n]+:\s*").unwrap())
}

/// 中英替换对（移植 ZH/EN_REPLACEMENTS）。
const ZH_REPLACEMENTS: &[(&str, &str)] = &[
    ("前几章", "此前"),
    ("本章要做的是", "眼下要处理的是"),
    ("本章要做的", "眼下要处理的"),
    ("仿佛", "像"),
    ("似乎", "像是"),
];
const EN_REPLACEMENTS: &[(&str, &str)] = &[
    ("(?i)previous chapters", "earlier scenes"),
    ("(?i)this chapter needs to", "the current move is to"),
];

/// 选定上下文条目（renderNarrativeSelectedContext 用）。
pub struct ContextSourceRef<'a> {
    pub reason: &'a str,
    pub excerpt: Option<&'a str>,
}

/// 净化叙事控制文本：替换 hook id/slug、章节引用、套话。
pub fn sanitize_narrative_control_text(text: &str, language: WritingLanguage) -> String {
    let mut result = text.to_string();
    let hook_replacement = match language {
        WritingLanguage::En => "this thread",
        WritingLanguage::Zh => "这条线索",
    };
    result = hook_id_re().replace_all(&result, hook_replacement).into_owned();
    result = hook_slug_re().replace_all(&result, hook_replacement).into_owned();

    let chapter_ref_replacement = match language {
        WritingLanguage::En => "an earlier scene",
        WritingLanguage::Zh => "此前",
    };
    result = chapter_ref_word_re().replace_all(&result, chapter_ref_replacement).into_owned();
    result = chapter_ref_zh_re().replace_all(&result, chapter_ref_replacement).into_owned();

    for (pat, rep) in ZH_REPLACEMENTS.iter().chain(EN_REPLACEMENTS.iter()) {
        // EN 模式带 (?i) 前缀
        if let Some(stripped) = pat.strip_prefix("(?i)") {
            if let Ok(re) = Regex::new(&format!("(?i){}", stripped)) {
                result = re.replace_all(&result, *rep).into_owned();
            }
        } else if let Ok(re) = Regex::new(pat) {
            result = re.replace_all(&result, *rep).into_owned();
        }
    }
    result
}

/// 把 ChapterMemo + 可选弧线背景渲染为净化后的叙事控制块。
pub fn render_memo_as_narrative_block(
    memo: &ChapterMemo,
    arc_context: Option<&str>,
    language: WritingLanguage,
) -> String {
    let s = |text: &str| sanitize_narrative_control_text(text, language);
    let is_en = matches!(language, WritingLanguage::En);
    let mut sections: Vec<String> = Vec::new();

    sections.push(format!("## {}\n- {}", if is_en { "Goal" } else { "目标" }, s(&memo.goal)));

    if let Some(arc) = arc_context {
        if !arc.is_empty() {
            sections.push(format!("## {}\n- {}", if is_en { "Arc Context" } else { "弧线背景" }, s(arc)));
        }
    }

    if !memo.thread_refs.is_empty() {
        let threads: Vec<String> = memo.thread_refs.iter().map(|id| format!("- {id}")).collect();
        sections.push(format!("## {}\n{}", if is_en { "Thread Refs" } else { "关联线索" }, threads.join("\n")));
    }

    // R1 读者体验合同（357 号）：memo 携带结构化合同时，把它升级为顶层任务块，
    // writer 照它写、reviser 照它修；不携带时静默跳过（旧 memo / 稀疏 memo）。
    // 逐字段对齐 TS renderMemoAsNarrativeBlock（标签/全角冒号/分隔符一致）。
    if let Some(re) = &memo.reader_experience {
        let fields: [(&str, &str); 7] = [
            (if is_en { "previousHandoff" } else { "开头承接" }, &re.previous_handoff),
            (if is_en { "readerQuestion" } else { "读者问题" }, &re.reader_question),
            (if is_en { "promisePayoff" } else { "承诺兑现" }, &re.promise_payoff),
            (if is_en { "protagonistWant" } else { "主角欲求" }, &re.protagonist_want),
            (if is_en { "protagonistObstacle" } else { "主角障碍" }, &re.protagonist_obstacle),
            (if is_en { "sceneTurn" } else { "场景转折" }, &re.scene_turn),
            (if is_en { "endingNetChange" } else { "章末净变化" }, &re.ending_net_change),
        ];
        let mut lines: Vec<String> = fields
            .iter()
            .map(|(label, value)| format!("- {label}：{}", s(value)))
            .collect();
        if !re.title_candidates.is_empty() {
            lines.push(format!(
                "- {}：{}",
                if is_en { "titleCandidates" } else { "章名候选" },
                re.title_candidates.join(" ｜ ")
            ));
        }
        sections.push(format!(
            "## {}\n{}",
            if is_en { "Reader Experience Contract" } else { "读者体验合同" },
            lines.join("\n")
        ));
    }

    if memo.is_golden_opening {
        sections.push(format!(
            "## {}\n- {}",
            if is_en { "Golden Opening" } else { "黄金开场" },
            if is_en { "This is a golden opening chapter — prioritize hook-dense, high-tempo pacing." } else { "本章是黄金开场章——优先钩子密集、高节奏。" }
        ));
    }

    if !memo.body.trim().is_empty() {
        sections.push(s(&memo.body));
    }

    sections.join("\n\n")
}

/// 由 chapterIntent markdown 构建 6 段简报（Goal/Outline Node/Must Keep/Must Avoid/Style/Directives）。
pub fn build_narrative_intent_brief(chapter_intent: &str, language: WritingLanguage) -> String {
    let is_en = matches!(language, WritingLanguage::En);
    let sections: [(&str, &str); 6] = [
        ("## Goal", if is_en { "Goal" } else { "目标" }),
        ("## Outline Node", if is_en { "Outline Node" } else { "当前节点" }),
        ("## Must Keep", if is_en { "Keep" } else { "保留" }),
        ("## Must Avoid", if is_en { "Avoid" } else { "避免" }),
        ("## Style Emphasis", if is_en { "Style" } else { "风格" }),
        ("## Structured Directives", if is_en { "Directives" } else { "指令" }),
    ];

    let rendered: Vec<String> = sections
        .iter()
        .filter_map(|(heading, label)| {
            let section = extract_markdown_section(chapter_intent, heading)?;
            let mut lines: Vec<String> = section
                .split('\n')
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .filter(|l| !matches!(l.as_str(), "- none" | "- 无" | "- 本轮无" | "(not found)"))
                .collect();
            if lines.is_empty() {
                return None;
            }
            lines = lines
                .into_iter()
                .map(|l| if let Some(stripped) = l.strip_prefix("- ") { stripped.to_string() } else { l })
                .map(|l| sanitize_narrative_control_text(&l, language))
                .filter(|l| !l.is_empty())
                .map(|l| format!("- {l}"))
                .collect();
            Some(format!("## {label}\n{}", lines.join("\n")))
        })
        .collect();

    rendered.join("\n\n")
}

/// 渲染选定上下文条目为证据块。
pub fn render_narrative_selected_context(entries: &[ContextSourceRef], language: WritingLanguage) -> String {
    let heading = matches!(language, WritingLanguage::En).then_some("Evidence").unwrap_or("证据");
    let reason_label = matches!(language, WritingLanguage::En).then_some("reason").unwrap_or("原因");
    let detail_label = matches!(language, WritingLanguage::En).then_some("detail").unwrap_or("细节");

    entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let mut lines = vec![
                format!("### {heading} {}", i + 1),
                format!("- {reason_label}: {}", sanitize_narrative_control_text(entry.reason, language)),
            ];
            if let Some(excerpt) = entry.excerpt {
                lines.push(format!("- {detail_label}: {}", sanitize_narrative_control_text(excerpt, language)));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 净化证据块：先把 story/runtime 路径源替换为 "evidence:"，再 sanitize。
pub fn sanitize_narrative_evidence_block(block: Option<&str>, language: WritingLanguage) -> Option<String> {
    let block = block?;
    let without_sources = evidence_source_re().replace_all(block, "- evidence: ").into_owned();
    Some(sanitize_narrative_control_text(&without_sources, language))
}

/// 提取 markdown 中某 `## heading` 段落内容（到下一个 ## 标题）。
fn extract_markdown_section(content: &str, heading: &str) -> Option<String> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut buffer: Option<Vec<String>> = None;
    for line in lines {
        if line.trim() == heading {
            buffer = Some(Vec::new());
            continue;
        }
        if let Some(buf) = buffer.as_mut() {
            if line.starts_with("## ") && line.trim() != heading {
                break;
            }
            buf.push(line.to_string());
        }
    }
    let section = buffer?.join("\n");
    let trimmed = section.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_hook_ids_and_refs() {
        let s = sanitize_narrative_control_text("见 H123 与 hook-thread-one，第5章提及", WritingLanguage::Zh);
        assert!(s.contains("这条线索"));
        assert!(!s.contains("H123"));
        assert!(s.contains("此前"));
    }

    #[test]
    fn sanitize_zh_replacements() {
        let s = sanitize_narrative_control_text("仿佛他似乎知道", WritingLanguage::Zh);
        assert_eq!(s, "像他像是知道");
    }

    #[test]
    fn render_memo_block() {
        let memo = ChapterMemo {
            chapter: 3,
            goal: "目标 H1".into(),
            is_golden_opening: false,
            body: "## 当前任务\n推进 H2".into(),
            thread_refs: vec!["T1".into()],
            reader_experience: None,
        };
        let out = render_memo_as_narrative_block(&memo, Some("弧线 H3"), WritingLanguage::Zh);
        assert!(out.contains("## 目标"));
        assert!(out.contains("这条线索")); // H1/H2/H3 替换
        assert!(out.contains("## 关联线索"));
        assert!(out.contains("- T1"));
        assert!(out.contains("## 弧线背景"));
    }

    #[test]
    fn render_memo_golden_opening() {
        let memo = ChapterMemo { chapter: 1, goal: "g".into(), is_golden_opening: true, body: "".into(), thread_refs: vec![], reader_experience: None };
        let out = render_memo_as_narrative_block(&memo, None, WritingLanguage::Zh);
        assert!(out.contains("## 黄金开场"));
    }

    #[test]
    fn build_intent_brief_extracts_sections() {
        let intent = "## Goal\n- 达成 X\n\n## Must Avoid\n- 避免 Y\n\n## Other\n- 其他";
        let brief = build_narrative_intent_brief(intent, WritingLanguage::Zh);
        assert!(brief.contains("## 目标"));
        assert!(brief.contains("达成 X"));
        assert!(brief.contains("## 避免"));
        assert!(brief.contains("避免 Y"));
        // "## Other" 非 6 段之一 → 不出现
        assert!(!brief.contains("其他"));
    }

    #[test]
    fn render_selected_context() {
        let entries = vec![
            ContextSourceRef { reason: "见 H1", excerpt: Some("片段") },
            ContextSourceRef { reason: "另一条", excerpt: None },
        ];
        let out = render_narrative_selected_context(&entries, WritingLanguage::Zh);
        assert!(out.contains("### 证据 1"));
        assert!(out.contains("这条线索")); // H1 替换
        assert!(out.contains("### 证据 2"));
    }

    #[test]
    fn sanitize_evidence_block_strips_sources() {
        let block = Some("- story/path/file.md: 内容");
        let out = sanitize_narrative_evidence_block(block, WritingLanguage::Zh);
        assert!(out.unwrap().contains("- evidence: 内容"));
    }
}
