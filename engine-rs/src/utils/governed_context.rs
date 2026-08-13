//! 治理记忆证据块（governed-context）。
//!
//! 移植自 `packages/core/src/utils/governed-context.ts`（101 行，纯逻辑）。
//! 把 [`ContextPackage`] 的 selectedContext 按 source 前缀/全等分桶，渲染为
//! markdown 证据块（伏笔 / hook 债 / 章节摘要 / 卷级摘要 / 标题历史 / 情绪轨迹 / 正典），
//! 供 ContinuityAuditor 等审查 agent 注入提示词。

use serde::Serialize;
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

use crate::models::input_governance::{ContextPackage, ContextSource};
use crate::utils::language::WritingLanguage;

/// 治理记忆证据块集合。对齐 TS `buildGovernedMemoryEvidenceBlocks` 的返回
/// （undefined ↔ None，序列化时省略，与 JSON.stringify 丢 undefined 对齐）。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GovernedMemoryEvidenceBlocks {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_debt_block: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks_block: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summaries_block: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_summaries_block: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_history_block: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mood_trail_block: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canon_block: Option<String>,
}

/// 构建治理记忆证据块。对齐 TS `buildGovernedMemoryEvidenceBlocks`。
///
/// 分桶规则（source 前缀/全等，逐字对齐 TS）：
/// - 伏笔：`story/pending_hooks.md#` 前缀
/// - hook 债：`runtime/hook_debt#` 前缀
/// - 章节摘要：`story/chapter_summaries.md#` 前缀
/// - 卷级摘要：`story/volume_summaries.md#` 前缀
/// - 标题历史：全等 `story/chapter_summaries.md#recent_titles`
/// - 情绪轨迹：全等 `story/chapter_summaries.md#recent_mood_type_trail`
/// - 正典：全等 `story/parent_canon.md` / `story/fanfic_canon.md`
pub fn build_governed_memory_evidence_blocks(
    context_package: &ContextPackage,
    language: Option<WritingLanguage>,
) -> GovernedMemoryEvidenceBlocks {
    let language = language.unwrap_or(WritingLanguage::Zh);
    let selected = &context_package.selected_context;

    let hook_entries = filter_by_prefix(selected, "story/pending_hooks.md#");
    let hook_debt_entries = filter_by_prefix(selected, "runtime/hook_debt#");
    let summary_entries = filter_by_prefix(selected, "story/chapter_summaries.md#");
    let volume_summary_entries = filter_by_prefix(selected, "story/volume_summaries.md#");
    let title_history_entries = filter_exact(selected, "story/chapter_summaries.md#recent_titles");
    let mood_trail_entries =
        filter_exact(selected, "story/chapter_summaries.md#recent_mood_type_trail");
    let canon_entries = filter_any_exact(
        selected,
        &["story/parent_canon.md", "story/fanfic_canon.md"],
    );

    let is_en = language == WritingLanguage::En;
    GovernedMemoryEvidenceBlocks {
        // 注：TS 此处两分支同为 "Hook Debt Briefs"（原文如此，保留）。
        hook_debt_block: (!hook_debt_entries.is_empty())
            .then(|| render_hook_debt_block("Hook Debt Briefs", hook_debt_entries)),
        hooks_block: (!hook_entries.is_empty()).then(|| {
            render_evidence_block(
                if is_en { "Selected Hook Evidence" } else { "已选伏笔证据" },
                hook_entries,
            )
        }),
        summaries_block: (!summary_entries.is_empty()).then(|| {
            render_evidence_block(
                if is_en { "Selected Chapter Summary Evidence" } else { "已选章节摘要证据" },
                summary_entries,
            )
        }),
        volume_summaries_block: (!volume_summary_entries.is_empty()).then(|| {
            render_evidence_block(
                if is_en { "Selected Volume Summary Evidence" } else { "已选卷级摘要证据" },
                volume_summary_entries,
            )
        }),
        title_history_block: (!title_history_entries.is_empty()).then(|| {
            render_evidence_block(
                if is_en { "Recent Title History" } else { "近期标题历史" },
                title_history_entries,
            )
        }),
        mood_trail_block: (!mood_trail_entries.is_empty()).then(|| {
            render_evidence_block(
                if is_en { "Recent Mood / Chapter Type Trail" } else { "近期情绪/章节类型轨迹" },
                mood_trail_entries,
            )
        }),
        canon_block: (!canon_entries.is_empty()).then(|| {
            render_evidence_block(
                if is_en { "Canon Evidence" } else { "正典约束证据" },
                canon_entries,
            )
        }),
    }
}

fn filter_by_prefix<'a>(entries: &'a [ContextSource], prefix: &str) -> Vec<&'a ContextSource> {
    entries
        .iter()
        .filter(|e| e.source.starts_with(prefix))
        .collect()
}

fn filter_exact<'a>(entries: &'a [ContextSource], source: &str) -> Vec<&'a ContextSource> {
    entries.iter().filter(|e| e.source == source).collect()
}

fn filter_any_exact<'a>(entries: &'a [ContextSource], sources: &[&str]) -> Vec<&'a ContextSource> {
    entries.iter().filter(|e| sources.contains(&e.source.as_str())).collect()
}

/// hook 债块（无 source 前缀）。逐字移植 TS `renderHookDebtBlock`：
/// `- ${entry.excerpt ?? entry.reason}`。
fn render_hook_debt_block(heading: &str, entries: Vec<&ContextSource>) -> String {
    let lines: Vec<String> = entries
        .into_iter()
        .map(|e| format!("- {}", e.excerpt.as_deref().unwrap_or(&e.reason)))
        .collect();
    format!("\n## {heading}\n{}\n", lines.join("\n"))
}

/// 常规证据块（带 source 前缀）。逐字移植 TS `renderEvidenceBlock`：
/// `- ${entry.source}: ${entry.excerpt ?? entry.reason}`。
fn render_evidence_block(heading: &str, entries: Vec<&ContextSource>) -> String {
    let lines: Vec<String> = entries
        .into_iter()
        .map(|e| {
            format!(
                "- {}: {}",
                e.source,
                e.excerpt.as_deref().unwrap_or(&e.reason)
            )
        })
        .collect();
    format!("\n## {heading}\n{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::input_governance::ContextSource;

    fn src(source: &str, reason: &str, excerpt: Option<&str>) -> ContextSource {
        ContextSource {
            source: source.to_string(),
            reason: reason.to_string(),
            excerpt: excerpt.map(|s| s.to_string()),
        }
    }

    fn full_package() -> ContextPackage {
        ContextPackage {
            chapter: 7,
            selected_context: vec![
                src("story/pending_hooks.md#h-mentor", "伏笔债", Some("导师欠款未回收")),
                src("runtime/hook_debt#h-mentor", "hook 债", Some("受阻 3 章")),
                src("story/chapter_summaries.md#c5", "第 5 章摘要", None),
                src("story/chapter_summaries.md#recent_titles", "标题历史", None),
                src("story/chapter_summaries.md#recent_mood_type_trail", "情绪轨迹", None),
                src("story/volume_summaries.md#v1", "卷摘要", Some("第一卷收束")),
                src("story/parent_canon.md", "正传正典", Some("力量上限")),
                src("story/fanfic_canon.md", "同人正典", None),
                src("story/other.md", "不进任何桶", None),
            ],
        }
    }

    #[test]
    fn builds_all_blocks_zh() {
        let blocks = build_governed_memory_evidence_blocks(&full_package(), None);
        let hooks = blocks.hooks_block.expect("伏笔块应在");
        assert!(hooks.starts_with("\n## 已选伏笔证据\n"));
        assert!(hooks.contains("- story/pending_hooks.md#h-mentor: 导师欠款未回收"));
        assert!(hooks.ends_with('\n'));

        // hook 债块无 source 前缀（对齐 renderHookDebtBlock），且两语言同题。
        let debt = blocks.hook_debt_block.expect("hook 债块应在");
        assert!(debt.contains("\n## Hook Debt Briefs\n"));
        assert!(debt.contains("- 受阻 3 章"));
        assert!(!debt.contains("runtime/hook_debt#"));

        // 摘要缺 excerpt 时回退 reason。
        let summaries = blocks.summaries_block.expect("摘要块应在");
        assert!(summaries.contains("- story/chapter_summaries.md#c5: 第 5 章摘要"));

        let titles = blocks.title_history_block.expect("标题历史块应在");
        assert!(titles.contains("## 近期标题历史"));
        let mood = blocks.mood_trail_block.expect("情绪轨迹块应在");
        assert!(mood.contains("## 近期情绪/章节类型轨迹"));
        let volumes = blocks.volume_summaries_block.expect("卷摘要块应在");
        assert!(volumes.contains("## 已选卷级摘要证据"));
        let canon = blocks.canon_block.expect("正典块应在");
        // 两类正典合并进同一块。
        assert!(canon.contains("- story/parent_canon.md: 力量上限"));
        assert!(canon.contains("- story/fanfic_canon.md: 同人正典"));
    }

    #[test]
    fn builds_all_blocks_en_headings() {
        let blocks =
            build_governed_memory_evidence_blocks(&full_package(), Some(WritingLanguage::En));
        assert!(blocks.hooks_block.as_deref().unwrap().contains("## Selected Hook Evidence"));
        assert!(blocks.summaries_block.as_deref().unwrap().contains("## Selected Chapter Summary Evidence"));
        assert!(blocks.title_history_block.as_deref().unwrap().contains("## Recent Title History"));
        assert!(blocks.canon_block.as_deref().unwrap().contains("## Canon Evidence"));
        // hook 债块标题两语言相同（TS 原文如此）。
        assert!(blocks.hook_debt_block.as_deref().unwrap().contains("## Hook Debt Briefs"));
    }

    #[test]
    fn empty_package_yields_all_none() {
        let blocks = build_governed_memory_evidence_blocks(&ContextPackage::default(), None);
        assert_eq!(blocks, GovernedMemoryEvidenceBlocks::default());
        // 序列化省略 None 字段（对齐 JSON.stringify 丢 undefined）。
        let json = serde_json::to_value(&blocks).unwrap();
        assert_eq!(json, serde_json::json!({}));
    }

    #[test]
    fn title_history_excluded_from_plain_summary_prefix_only_if_exact_mismatch() {
        // recent_titles 是 chapter_summaries.md# 前缀，同时也会进 summaries 桶
        // （TS 同样双计入：前缀桶不排除全等桶，逐字保留）。
        let pkg = ContextPackage {
            chapter: 1,
            selected_context: vec![src("story/chapter_summaries.md#recent_titles", "标题", None)],
        };
        let blocks = build_governed_memory_evidence_blocks(&pkg, None);
        assert!(blocks.summaries_block.is_some());
        assert!(blocks.title_history_block.is_some());
    }
}
