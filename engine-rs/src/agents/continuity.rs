//! 连续性审查（continuity）——ContinuityAuditor 全量。
//!
//! 移植自 `packages/core/src/agents/continuity.ts`（842 行）：
//! - 类型层：[`AuditResult`] / [`AuditIssue`]（writer-parser / sensitive-words /
//!   hook-health 的共同依赖）
//! - 纯逻辑层：37 维度标签 / [`build_dimension_note`] / [`build_dimension_list`]
//!   / [`parse_audit_result`] 四策略 / [`build_reduced_control_block`]
//! - 编排层：[`audit_chapter`]（真相文件加载 + prompt 构造 + 解析），
//!   LLM 调用经 [`AuditorChat`] trait 注入（测试 mock / 生产 BaseAgent 实现）

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use super::fanfic_dimensions::{get_fanfic_dimension_config, FanficDimensionConfig, FANFIC_DIMENSIONS};
use super::rules_reader::{
    read_book_language, read_book_rules, read_genre_profile, ReadGenreProfileError,
};
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::book::FanficMode;
use crate::models::book_rules::{AuditDimension, BookRules};
use crate::models::genre_profile::GenreProfile;
use crate::models::input_governance::{ChapterMemo, ContextPackage, RuleStack};
use crate::prompts::prompt_pack::{
    append_prompt_pack_guidance, LoadPromptPackPromptInput, PromptPackPromptNotFoundError,
};
use crate::state::store::StateStore;
use crate::utils::context_filter::{
    filter_character_matrix, filter_emotional_arcs, filter_hooks, filter_subplots, filter_summaries,
};
use crate::utils::governed_context::build_governed_memory_evidence_blocks;
use crate::utils::language::WritingLanguage;
use crate::utils::outline_paths::{
    read_character_context, read_current_state_with_fallback, read_volume_map,
};

/// 审查结果。对齐 TS `AuditResult`。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditResult {
    pub passed: bool,
    pub issues: Vec<AuditIssue>,
    pub summary: String,
    /// 审查响应本身不可解析时为 true；调用方不得据此结果自动修订内容。
    pub parse_failed: Option<bool>,
    /// 0-100 整体质量分（审查器支持打分时存在）。
    pub overall_score: Option<u32>,
    pub token_usage: Option<AuditTokenUsage>,
}

/// token 用量。对齐 TS `AuditResult.tokenUsage`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditTokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// 单条审查问题。对齐 TS `AuditIssue`。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditIssue {
    pub severity: AuditSeverity,
    pub category: String,
    pub description: String,
    pub suggestion: String,
    pub repair_scope: Option<RepairScope>,
}

/// 问题严重度。对齐 TS `"critical" | "warning" | "info"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "export-bindings",
    ts(export, type = "\"critical\" | \"warning\" | \"info\"")
)]
pub enum AuditSeverity {
    #[serde(rename = "critical")]
    Critical,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "info")]
    Info,
}

/// 修复范围。对齐 TS `"local" | "structural" | "unknown"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum RepairScope {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "structural")]
    Structural,
    #[serde(rename = "unknown")]
    Unknown,
}

/// 规范化修复范围值（未知值 → None）。对齐 TS `normalizeRepairScope`。
pub fn normalize_repair_scope(value: &serde_json::Value) -> Option<RepairScope> {
    match value.as_str()? {
        "local" => Some(RepairScope::Local),
        "structural" => Some(RepairScope::Structural),
        "unknown" => Some(RepairScope::Unknown),
        _ => None,
    }
}

/// 文本是否含中文字符。对齐 TS `containsChinese`。
pub fn contains_chinese(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// 解析题材标签：中文语言或 profileName 不含中文时直接用 profileName；
/// 否则 "other" → "general"，余者把 `_-` 转空格。对齐 TS `resolveGenreLabel`。
pub fn resolve_genre_label(genre_id: &str, profile_name: &str, language: WritingLanguage) -> String {
    if language == WritingLanguage::Zh || !contains_chinese(profile_name) {
        return profile_name.to_string();
    }
    if genre_id == "other" {
        return "general".to_string();
    }
    genre_id.replace(['_', '-'], " ")
}

/// 维度名（37 个，中英）。对齐 TS `dimensionName`。
pub fn dimension_name(id: u32, language: WritingLanguage) -> Option<&'static str> {
    let labels = dimension_labels();
    let entry = labels.get(&id)?;
    Some(if language == WritingLanguage::En { entry.en } else { entry.zh })
}

/// 本地化 join（中文「、」，英文「, 」）。对齐 TS `joinLocalized`。
pub fn join_localized(items: &[String], language: WritingLanguage) -> String {
    let sep = if language == WritingLanguage::En { ", " } else { "、" };
    items.join(sep)
}

/// 同人维度严重度提示。对齐 TS `formatFanficSeverityNote`。
pub fn format_fanfic_severity_note(severity: AuditSeverity, language: WritingLanguage) -> &'static str {
    if language == WritingLanguage::En {
        return match severity {
            AuditSeverity::Critical => "Strict check.",
            AuditSeverity::Info => "Log only; do not fail the chapter.",
            AuditSeverity::Warning => "Warning level.",
        };
    }
    match severity {
        AuditSeverity::Critical => "（严格检查）",
        AuditSeverity::Info => "（仅记录，不判定失败）",
        AuditSeverity::Warning => "（警告级别）",
    }
}

// --- 37 维度标签（OnceLock HashMap）-------------------------------------------

struct DimensionLabel {
    zh: &'static str,
    en: &'static str,
}

fn dimension_labels() -> &'static HashMap<u32, DimensionLabel> {
    static M: OnceLock<HashMap<u32, DimensionLabel>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        let entries: [(u32, &str, &str); 39] = [
            (1, "OOC检查", "OOC Check"),
            (2, "时间线检查", "Timeline Check"),
            (3, "设定冲突", "Lore Conflict Check"),
            (4, "战力崩坏", "Power Scaling Check"),
            (5, "数值检查", "Numerical Consistency Check"),
            (6, "伏笔检查", "Hook Check"),
            (7, "节奏检查", "Pacing Check"),
            (8, "文风检查", "Style Check"),
            (9, "信息越界", "Information Boundary Check"),
            (10, "词汇疲劳", "Lexical Fatigue Check"),
            (11, "利益链断裂", "Incentive Chain Check"),
            (12, "年代考据", "Era Accuracy Check"),
            (13, "配角降智", "Side Character Competence Check"),
            (14, "配角工具人化", "Side Character Instrumentalization Check"),
            (15, "爽点虚化", "Payoff Dilution Check"),
            (16, "台词失真", "Dialogue Authenticity Check"),
            (17, "流水账", "Chronicle Drift Check"),
            (18, "知识库污染", "Knowledge Base Pollution Check"),
            (19, "视角一致性", "POV Consistency Check"),
            (20, "段落等长", "Paragraph Uniformity Check"),
            (21, "套话密度", "Cliche Density Check"),
            (22, "公式化转折", "Formulaic Twist Check"),
            (23, "列表式结构", "List-like Structure Check"),
            (24, "支线停滞", "Subplot Stagnation Check"),
            (25, "弧线平坦", "Arc Flatline Check"),
            (26, "节奏单调", "Pacing Monotony Check"),
            (27, "敏感词检查", "Sensitive Content Check"),
            (28, "正传事件冲突", "Mainline Canon Event Conflict"),
            (29, "未来信息泄露", "Future Knowledge Leak Check"),
            (30, "世界规则跨书一致性", "Cross-Book World Rule Check"),
            (31, "番外伏笔隔离", "Spinoff Hook Isolation Check"),
            (32, "读者期待管理", "Reader Expectation Check"),
            (33, "章节备忘偏离", "Chapter Memo Drift Check"),
            (34, "角色还原度", "Character Fidelity Check"),
            (35, "世界规则遵守", "World Rule Compliance Check"),
            (36, "关系动态", "Relationship Dynamics Check"),
            (37, "正典事件一致性", "Canon Event Consistency Check"),
            // G7a/341 号：信息差账本两维度（story/info_gaps.md 存在时激活）。
            (38, "泄密机检", "Secret Leak Check"),
            (39, "废笔机检", "Reader Redundancy Check"),
        ];
        for (id, zh, en) in entries {
            m.insert(id, DimensionLabel { zh, en });
        }
        m
    })
}

// --- ContinuityAuditor 纯逻辑主体（buildDimensionNote / buildDimensionList）----------

/// 激活维度条目。对齐 TS `buildDimensionList` 返回元素。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimensionEntry {
    pub id: u32,
    pub name: String,
    pub note: String,
}

/// 构造维度注记。逐字移植 TS `buildDimensionNote`（含分支顺序与不可达分支形状）。
///
/// **分支顺序是行为**：
/// - `id=15`：`satisfactionTypes` 非空时**提前返回**简版注记——其后 v10 增强版的
///   `base` 非空分支因此不可达（base 恒空串）。TS 原文如此，逐字保留并由单测钉死。
/// - `id=25`：v10「人设三问」提前返回——switch 中 case 25（情绪弧线版）不可达。
#[allow(clippy::too_many_arguments)]
pub fn build_dimension_note(
    id: u32,
    language: WritingLanguage,
    gp: &GenreProfile,
    book_rules: Option<&BookRules>,
    fanfic_mode: Option<FanficMode>,
    fanfic_config: Option<&FanficDimensionConfig>,
) -> String {
    let words: &Vec<String> = match book_rules {
        Some(rules) if !rules.fatigue_words_override.is_empty() => &rules.fatigue_words_override,
        _ => &gp.fatigue_words,
    };

    // fanficConfig.notes 命中且中文 → 直接返回（en 时落到通用分支，TS 行为）。
    if let Some(cfg) = fanfic_config {
        if language == WritingLanguage::Zh {
            if let Some(note) = cfg.notes.get(&id) {
                return note.clone();
            }
        }
    }

    if id == 1 && fanfic_mode == Some(FanficMode::Ooc) {
        return if language == WritingLanguage::En {
            "In OOC mode, personality drift can be intentional; record only, do not fail. Evaluate against the character dossiers in fanfic_canon.md.".to_string()
        } else {
            "OOC模式下角色可偏离性格底色，此维度仅记录不判定失败。参照 fanfic_canon.md 角色档案评估偏离程度。".to_string()
        };
    }

    if id == 1 && fanfic_mode == Some(FanficMode::Canon) {
        return if language == WritingLanguage::En {
            "Canon-faithful fanfic: characters must stay close to their original personality core. Evaluate against fanfic_canon.md character dossiers.".to_string()
        } else {
            "原作向同人：角色必须严格遵守性格底色。参照 fanfic_canon.md 角色档案中的性格底色和行为模式。".to_string()
        };
    }

    if id == 10 && !words.is_empty() {
        return if language == WritingLanguage::En {
            format!(
                "Fatigue words: {}. Also check AI tell markers (仿佛/不禁/宛如/竟然/忽然/猛地); warn when any appears more than once per 3,000 words.",
                words.join(", ")
            )
        } else {
            format!(
                "高疲劳词：{}。同时检查AI标记词（仿佛/不禁/宛如/竟然/忽然/猛地）密度，每3000字超过1次即warning",
                words.join("、")
            )
        };
    }

    if id == 15 && !gp.satisfaction_types.is_empty() {
        return if language == WritingLanguage::En {
            format!("Payoff types: {}", gp.satisfaction_types.join(", "))
        } else {
            format!("爽点类型：{}", gp.satisfaction_types.join("、"))
        };
    }

    if id == 12 {
        if let Some(rules) = book_rules {
            if let Some(era) = &rules.era_constraints {
                let parts: Vec<&str> = [&era.period, &era.region]
                    .iter()
                    .filter_map(|p| p.as_deref().filter(|s| !s.is_empty()))
                    .collect();
                if !parts.is_empty() {
                    return if language == WritingLanguage::En {
                        format!("Era: {}", parts.join(", "))
                    } else {
                        format!("年代：{}", parts.join("，"))
                    };
                }
            }
        }
    }

    // v10：写作方法论感知的增强维度注记。
    if id == 7 {
        return if language == WritingLanguage::En {
            "Check pacing rhythm: Do the recent 3-5 chapters form a complete mini-goal cycle (build-up → escalation → climax → aftermath)? If 5+ consecutive chapters pass without a climax (payoff/reward/reversal), flag as pacing stagnation. If the previous chapter was a climax/big reversal, does this chapter show change (relationships shifted, status changed, costs paid)? If it jumps straight to new build-up without showing impact, flag as 'post-climax impact missing'. Daily/transition scenes must carry at least one task: plant a hook, advance a relationship, set up contrast, or prepare the next cycle.".to_string()
        } else {
            "检查节奏波形：最近 3-5 章是否形成了完整的「蓄压→升级→爆发→后效」周期？如果连续 5 章没有爆发（兑现/回报/翻转），标记为节奏停滞。如果上一章是爆发/高潮/大反转，本章是否写出了改变？如果直接跳到新蓄压而没有展示前一波爆发的影响，标记为「高潮后影响缺失」。非冲突章节中的日常/过渡/对话段落，是否至少承担了一项任务：埋伏笔、推关系、建立反差、准备下一轮蓄压。纯水日常标记为流水账风险。".to_string()
        };
    }

    if id == 15 {
        // 到达此分支时 satisfactionTypes 必为空（上方提前返回），base 恒空串。
        // 保留 TS 的 base 计算形状以钉死「提前返回」这一行为边界。
        let base = if !gp.satisfaction_types.is_empty() {
            if language == WritingLanguage::En {
                format!("Payoff types: {}. ", gp.satisfaction_types.join(", "))
            } else {
                format!("爽点类型：{}。", gp.satisfaction_types.join("、"))
            }
        } else {
            String::new()
        };
        return if language == WritingLanguage::En {
            format!("{base}Check desire engine: Has the chapter created an emotional gap (reader wants release) OR delivered a payoff that exceeds expectations? A payoff that only satisfies 70% of built-up anticipation counts as diluted. If this chapter is in the aftermath phase of a mini-goal cycle, verify that consequences are shown — not just emotional reactions, but concrete changes to status, relationships, or resources.")
        } else {
            format!("{base}检查欲望驱动：本章是否制造了情绪缺口（读者渴望释放）或完成了超出预期的兑现？只满足读者70%期待的兑现等于爽点虚化。如果本章处于小目标周期的后效阶段，检查是否展示了具体改变——不只是情绪反应，而是地位、关系或资源的实际变化。")
        };
    }

    if id == 25 {
        return if language == WritingLanguage::En {
            "Cross-check character behavior against the 3-question test: (1) Why does the character do this? (2) Does it match their established profile? (3) Would a reader who only read prior chapters find it jarring? Also check if character's emotional state progresses or stagnates.".to_string()
        } else {
            "人设三问检查：(1)角色为什么这么做？(2)符合之前建立的人设吗？(3)只看过前面章节的读者会觉得突兀吗？同时检查角色情绪弧线是否在推进还是停滞。".to_string()
        };
    }

    match id {
        6 => {
            // Phase 7 — hook-debt 升级规则（含 hotfix 2/3）。
            if language == WritingLanguage::En {
                "Hook-debt escalation (Phase 7 + hotfixes 2/3). Read the pending_hooks.md ledger and escalate based on the stale / blocked / core_hook / depends_on / promoted columns, NOT only on \"undelivered hook present\":\n\n• Critical severity only applies to hooks with promoted=true in the ledger. A stale/blocked non-promoted hook stays at info — the promotion flag is the gate that keeps reviewer noise down, because architect-seed emits many non-load-bearing seeds.\n• A promoted core_hook=true hook that has been stale for over 10 chapters → escalate from warning to critical. The book has only 3-7 core hooks; letting one drift that long is the lead symptom of narrative rot.\n• A promoted hook whose status cell contains \"blocked on X (blocked Y chapters)\" with Y >= 6 → warning. The literal \"blocked Y chapters\" token comes straight from the ledger — read it, don't guess. Call out the upstream hook id so the planner can route the resolution.\n• At volume end (final chapter of any volume per volume_map) a promoted core_hook that is still open or stale without explicit \"carried over to volume N+1\" planning → critical.\n• Any non-promoted stale hook → info-level log; do not fail the chapter on it, but note it so the planner can schedule cleanup.\n\nQuote the exact hook_id in description and include the stale / blocked marker text verbatim. Structure check only — do not judge hook prose quality.".to_string()
            } else {
                "Phase 7 hook-debt 升级规则（含 hotfix 2/3）。阅读 pending_hooks.md 伏笔池时不要只看\"有没有悬而未决的伏笔\"，要读状态列中的 stale / blocked 标记、core_hook 列、depends_on 列、以及升级列：\n\n• critical 级别仅适用于升级=是（promoted=true）的伏笔。非升级的 stale/blocked 伏笔一律保持 info——升级标志是降噪的开关，因为架构师阶段会产出大量非承重的伏笔种子。\n• 升级=是且 core_hook=是 的伏笔过期超过 10 章未回收 → warning 升级为 critical。全书只有 3-7 条核心伏笔，任何一条漂移这么久都是烂尾前兆。\n• 升级=是的受阻伏笔，状态列中\"受阻于 X (已阻 Y 章)\"且 Y ≥ 6 → warning。\"已阻 Y 章\"这个字面 token 直接读自账本，不要猜。描述中要写出具体的上游 hook_id，让 planner 能安排落地路径。\n• 卷尾（volume_map 中任一卷的末章）仍有升级=是的主线伏笔处于 open 或 stale 且没有显式\"延至下一卷\"规划 → critical。\n• 升级=否的 stale 伏笔 → info 级记录，不判本章失败，但保留以便 planner 安排清理。\n\ndescription 中要明确引用 hook_id，并把状态列中 stale / blocked 的原文标记字面抄进去。本维度只审结构，不评价伏笔文笔。".to_string()
            }
        }
        19 => {
            if language == WritingLanguage::En {
                "Check whether POV shifts are signaled clearly and stay consistent with the configured viewpoint.".to_string()
            } else {
                "检查视角切换是否有过渡、是否与设定视角一致".to_string()
            }
        }
        24 => {
            if language == WritingLanguage::En {
                "Cross-check subplot_board and chapter_summaries: flag any subplot that stays dormant long enough to feel abandoned, or a recent run where every subplot is only restated instead of genuinely moving.".to_string()
            } else {
                "对照 subplot_board 和 chapter_summaries：标记那些沉寂到接近被遗忘的支线，或近期连续只被重复提及、没有真实推进的支线。".to_string()
            }
        }
        25 => {
            // 不可达分支（上方 id==25 v10 提前返回）；逐字保留对齐 TS switch 形状。
            if language == WritingLanguage::En {
                "Cross-check emotional_arcs and chapter_summaries: flag any major character whose emotional line holds one pressure shape across a run instead of taking new pressure, release, reversal, or reinterpretation. Distinguish unchanged circumstances from unchanged inner movement.".to_string()
            } else {
                "对照 emotional_arcs 和 chapter_summaries：标记主要角色在一段时间内始终停留在同一种情绪压力形态、没有新压力、释放、转折或重估的情况。注意区分'处境未变'和'内心未变'。".to_string()
            }
        }
        26 => {
            if language == WritingLanguage::En {
                "Cross-check chapter_summaries for chapter-type distribution: warn when the recent sequence stays in the same mode long enough to flatten rhythm, or when payoff / release beats disappear for too long. Explicitly list the recent type sequence.".to_string()
            } else {
                "对照 chapter_summaries 的章节类型分布：当近期章节长时间停留在同一种模式、把节奏压平，或回收/释放/高潮章节缺席过久时给出 warning。请明确列出最近章节的类型序列。".to_string()
            }
        }
        28 => {
            if language == WritingLanguage::En {
                "Check whether spinoff events contradict the mainline canon constraints.".to_string()
            } else {
                "检查番外事件是否与正典约束表矛盾".to_string()
            }
        }
        29 => {
            if language == WritingLanguage::En {
                "Check whether characters reference information that should only be revealed after the divergence point (see the information-boundary table).".to_string()
            } else {
                "检查角色是否引用了分歧点之后才揭示的信息（参照信息边界表）".to_string()
            }
        }
        30 => {
            if language == WritingLanguage::En {
                "Check whether the spinoff violates mainline world rules (power system, geography, factions).".to_string()
            } else {
                "检查番外是否违反正传世界规则（力量体系、地理、阵营）".to_string()
            }
        }
        31 => {
            if language == WritingLanguage::En {
                "Check whether the spinoff resolves mainline hooks without authorization (warning level).".to_string()
            } else {
                "检查番外是否越权回收正传伏笔（warning级别）".to_string()
            }
        }
        32 => {
            if language == WritingLanguage::En {
                "Check whether the ending renews curiosity, whether promised payoffs are landing on the cadence their hooks imply, whether pressure gets any release, and whether reader expectation gaps are accumulating faster than they are being satisfied. If a climax just occurred, check whether the aftermath chapters show concrete change before starting a new cycle.".to_string()
            } else {
                "检查：章尾是否重新点燃好奇心，已经承诺的回收是否按伏笔自身节奏落地，压力是否得到释放，读者期待缺口是在持续累积还是在被满足。如果刚经历高潮，检查后效章节是否在开启新周期前展示了具体改变。".to_string()
            }
        }
        33 => {
            if language == WritingLanguage::En {
                "Cross-check the chapter_memo provided with the chapter. Does the final prose deliver the memo's goal and leave a visible trace for every one of the 7 sections it contains (tasks, pay-offs / held-back cards, daily/transition function map, three-question check, end-of-chapter concrete changes, hard-don'ts)? Missing or contradicted sections -> critical. Note: a sparse memo (breather chapter, goal + skeleton body only) is legitimate — only flag drift against sections that the memo actually populates. Never flag the memo itself for being sparse.".to_string()
            } else {
                "对照随章提供的 chapter_memo。成稿是否兑现了 memo 中的 goal，并在 7 段正文（当前任务 / 该兑现·暂不掀 / 日常过渡功能 / 关键抉择三连问 / 章尾必须发生的改变 / 不要做 等）中留下可见落地痕迹？任何段落缺失或被写反 → critical。提醒：稀疏 memo 合法（喘息章 memo 可以只有 goal + 骨架 body），只检查 memo 实际写出的段落，不能因为 memo 稀疏就判 incomplete。".to_string()
            }
        }
        34..=37 => {
            let Some(cfg) = fanfic_config else {
                return String::new();
            };
            let severity = cfg.severity_overrides.get(&id).copied()
                .unwrap_or(AuditSeverity::Warning);
            let base_note: Option<&str> = if language == WritingLanguage::En {
                match id {
                    34 => Some("Check whether dialogue tics, speaking style, and behavior remain consistent with the character dossiers in fanfic_canon.md. Deviations need clear situational motivation."),
                    35 => Some("Check whether the chapter violates world rules documented in fanfic_canon.md (geography, power system, faction relations)."),
                    36 => Some("Check whether relationship beats remain plausible and aligned with, or meaningfully develop from, the key relationships documented in fanfic_canon.md."),
                    37 => Some("Check whether the chapter contradicts the key event timeline in fanfic_canon.md."),
                    _ => None,
                }
            } else {
                FANFIC_DIMENSIONS
                    .iter()
                    .find(|dimension| dimension.id == id)
                    .map(|dimension| dimension.base_note)
            };

            match base_note {
                Some(base) => format!(
                    "{base} {}",
                    format_fanfic_severity_note(severity, language)
                ),
                None => String::new(),
            }
        }
        _ => String::new(),
    }
}

/// f64 维度 id → u32（TS `Set<number>` 键语义：非整数/超范围查不到标签 → 跳过）。
fn as_dimension_id(id: f64) -> Option<u32> {
    if id.fract() != 0.0 || !(0.0..=u32::MAX as f64).contains(&id) {
        return None;
    }
    Some(id as u32)
}

/// 名称 → id 反查表（zh 在前 en 在后，按 id 升序——对齐 TS Map 插入序）。
fn dimension_name_to_id_pairs() -> &'static Vec<(&'static str, u32)> {
    static M: OnceLock<Vec<(&'static str, u32)>> = OnceLock::new();
    M.get_or_init(|| {
        let labels = dimension_labels();
        let mut pairs = Vec::with_capacity(74);
        for id in 1..=37u32 {
            let entry = labels.get(&id).expect("1..=37 全覆盖");
            pairs.push((entry.zh, id));
            pairs.push((entry.en, id));
        }
        pairs
    })
}

/// 构造激活维度列表。逐字移植 TS `buildDimensionList`。
pub fn build_dimension_list(
    gp: &GenreProfile,
    book_rules: Option<&BookRules>,
    language: WritingLanguage,
    has_parent_canon: bool,
    fanfic_mode: Option<FanficMode>,
    has_info_gaps: bool,
) -> Vec<DimensionEntry> {
    let mut active_ids: Vec<f64> = Vec::new();
    let add = |id: f64, ids: &mut Vec<f64>| {
        if !ids.contains(&id) {
            ids.push(id);
        }
    };
    for &id in &gp.audit_dimensions {
        add(id, &mut active_ids);
    }

    // 书级追加维度（数字 id 与名称字符串皆可）。
    if let Some(rules) = book_rules {
        if !rules.additional_audit_dimensions.is_empty() {
            let name_to_id = dimension_name_to_id_pairs();
            for d in &rules.additional_audit_dimensions {
                match d {
                    AuditDimension::Number(n) => add(*n, &mut active_ids),
                    AuditDimension::Text(s) => {
                        // 先精确匹配，再子串模糊匹配。
                        if let Some((_, id)) = name_to_id.iter().find(|(name, _)| *name == s.as_str()) {
                            add(*id as f64, &mut active_ids);
                        } else if let Some((_, id)) = name_to_id
                            .iter()
                            .find(|(name, _)| name.contains(s.as_str()) || s.contains(name))
                        {
                            add(*id as f64, &mut active_ids);
                        }
                    }
                }
            }
        }
    }

    // 恒激活维度。
    add(32.0, &mut active_ids); // 读者期待管理 — universal
    add(33.0, &mut active_ids); // 章节备忘偏离 — universal（替代 legacy 卷纲偏离）

    // G7a/341 号：信息差账本存在 → 泄密机检(38)/废笔机检(39) 激活。
    if has_info_gaps {
        add(38.0, &mut active_ids);
        add(39.0, &mut active_ids);
    }

    // 条件覆盖。
    if gp.era_research
        || book_rules.is_some_and(|r| r.era_constraints.as_ref().is_some_and(|e| e.enabled))
    {
        add(12.0, &mut active_ids);
    }

    // 番外维度 — parent_canon.md 存在时激活（但同人模式下不激活）。
    if has_parent_canon && fanfic_mode.is_none() {
        add(28.0, &mut active_ids); // 正传事件冲突
        add(29.0, &mut active_ids); // 未来信息泄露
        add(30.0, &mut active_ids); // 世界规则跨书一致性
        add(31.0, &mut active_ids); // 番外伏笔隔离
    }

    // 同人维度 — 以同人专用检查替换番外维度。
    let mut fanfic_config: Option<FanficDimensionConfig> = None;
    if let Some(mode) = fanfic_mode {
        let empty: Vec<String> = Vec::new();
        let allowed = book_rules
            .map(|r| r.allowed_deviations.clone())
            .unwrap_or(empty);
        let cfg = get_fanfic_dimension_config(mode, &allowed);
        for &id in &cfg.active_ids {
            add(id as f64, &mut active_ids);
        }
        active_ids.retain(|id| !cfg.deactivated_ids.contains(&(*id as u32)));
        fanfic_config = Some(cfg);
    }

    active_ids.sort_unstable_by(f64::total_cmp);

    let mut dims = Vec::with_capacity(active_ids.len());
    for id in active_ids {
        let Some(uid) = as_dimension_id(id) else {
            continue;
        };
        let Some(name) = dimension_name(uid, language) else {
            continue;
        };
        let note = build_dimension_note(
            uid,
            language,
            gp,
            book_rules,
            fanfic_mode,
            fanfic_config.as_ref(),
        );
        dims.push(DimensionEntry {
            id: uid,
            name: name.to_string(),
            note,
        });
    }
    dims
}

// --- 审查结果解析（parseAuditResult 四策略）-----------------------------------------

/// 从文本提取首个平衡 JSON 对象（非贪婪）。对齐 TS `extractBalancedJson`。
///
/// 按 ASCII 字节计数 `{`/`}`（UTF-8 多字节序列不含这两个字节，安全）。
pub fn extract_balanced_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    for (i, &b) in bytes[start..].iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return Some(&text[start..start + i + 1]);
        }
    }
    None
}

/// JS `??` 合并语义：缺失或 null 取右值。
fn coalesce<'a>(a: Option<&'a serde_json::Value>, b: Option<&'a serde_json::Value>) -> Option<&'a serde_json::Value> {
    match a {
        Some(serde_json::Value::Null) | None => b,
        other => other,
    }
}

/// 尝试把 JSON 文本解析为 AuditResult。失败/形状不符 → None。对齐 TS `tryParseAuditJson`。
fn try_parse_audit_json(json: &str, language: WritingLanguage) -> Option<AuditResult> {
    let parsed: serde_json::Value = serde_json::from_str(json).ok()?;

    // typeof passed !== "boolean" && passed !== undefined → null。
    match parsed.get("passed") {
        Some(serde_json::Value::Bool(_)) | None => {}
        Some(serde_json::Value::Null) => {}
        Some(_) => return None,
    }

    let raw_score = coalesce(parsed.get("overall_score"), parsed.get("overallScore"));
    let overall_score = raw_score
        .and_then(|v| v.as_f64())
        .filter(|n| n.is_finite())
        .map(|n| n.clamp(0.0, 100.0).round() as u32);

    let issues = match parsed.get("issues") {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|i| parse_audit_issue(i, language))
            .collect(),
        _ => Vec::new(),
    };

    Some(AuditResult {
        passed: parsed
            .get("passed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        issues,
        summary: parsed
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        parse_failed: None,
        overall_score,
        token_usage: None,
    })
}

/// 单条 issue 的宽松字段映射。对齐 TS tryParseAuditJson / 策略 4 的逐字段 `??` 回退。
///
/// 非字符串的极端值（如 severity: 123）在 TS 运行时被保留但下游按「非 critical/info」
/// 处理——Rust 枚举收拢为 Warning（行为等价）；category/description 同理取回退值。
fn parse_audit_issue(i: &serde_json::Value, language: WritingLanguage) -> AuditIssue {
    let uncategorized = if language == WritingLanguage::En {
        "Uncategorized"
    } else {
        "未分类"
    };
    let severity = i
        .get("severity")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "critical" => AuditSeverity::Critical,
            "info" => AuditSeverity::Info,
            _ => AuditSeverity::Warning,
        })
        .unwrap_or(AuditSeverity::Warning);
    AuditIssue {
        severity,
        category: i
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or(uncategorized)
            .to_string(),
        description: i
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        suggestion: i
            .get("suggestion")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        repair_scope: coalesce(i.get("repair_scope"), i.get("repairScope"))
            .and_then(normalize_repair_scope),
    }
}

/// 解析审查器响应为 AuditResult。对齐 TS `parseAuditResult`（4 策略 + 失败兜底）。
pub fn parse_audit_result(content: &str, language: WritingLanguage) -> AuditResult {
    // 策略 1：平衡 JSON 对象（非贪婪）。
    if let Some(balanced) = extract_balanced_json(content) {
        if let Some(result) = try_parse_audit_json(balanced, language) {
            return result;
        }
    }

    // 策略 2：整个内容即 JSON（部分模型输出纯 JSON）。
    let trimmed = content.trim();
    if trimmed.starts_with('{') {
        if let Some(result) = try_parse_audit_json(trimmed, language) {
            return result;
        }
    }

    // 策略 3：```json 代码块。
    {
        static CODE_BLOCK: OnceLock<Regex> = OnceLock::new();
        let code_block = CODE_BLOCK.get_or_init(|| {
            Regex::new(r"```(?:json)?\s*\n?([\s\S]*?)\n?```").unwrap()
        });
        if let Some(m) = code_block.captures(content) {
            let block = m.get(1).expect("组 1 必在").as_str().trim();
            if let Some(result) = try_parse_audit_json(block, language) {
                return result;
            }
        }
    }

    // 策略 4：逐字段正则提取（最后回退）。
    {
        static PASSED: OnceLock<Regex> = OnceLock::new();
        static ISSUES: OnceLock<Regex> = OnceLock::new();
        static SUMMARY: OnceLock<Regex> = OnceLock::new();
        static ISSUE_OBJ: OnceLock<Regex> = OnceLock::new();
        let passed_re = PASSED.get_or_init(|| Regex::new(r#""passed"\s*:\s*(true|false)"#).unwrap());
        let issues_re = ISSUES.get_or_init(|| Regex::new(r#""issues"\s*:\s*\[([\s\S]*?)\]"#).unwrap());
        let summary_re = SUMMARY.get_or_init(|| Regex::new(r#""summary"\s*:\s*"([^"]*)""#).unwrap());
        let issue_obj = ISSUE_OBJ.get_or_init(|| {
            Regex::new(r#"\{[^{}]*"severity"\s*:\s*"[^"]*"[^{}]*\}"#).unwrap()
        });

        if let Some(m) = passed_re.captures(content) {
            let passed = m.get(1).expect("组 1 必在").as_str() == "true";
            let mut issues: Vec<AuditIssue> = Vec::new();
            if let Some(im) = issues_re.captures(content) {
                for om in issue_obj.find_iter(im.get(1).expect("组 1 必在").as_str()) {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(om.as_str()) {
                        issues.push(parse_audit_issue(&value, language));
                    }
                }
            }
            return AuditResult {
                passed,
                issues,
                summary: summary_re
                    .captures(content)
                    .map(|sm| sm.get(1).expect("组 1 必在").as_str().to_string())
                    .unwrap_or_default(),
                parse_failed: None,
                overall_score: None,
                token_usage: None,
            };
        }
    }

    // 全策略失败：parseFailed 兜底（调用方不得据此自动修订内容）。
    let en = language == WritingLanguage::En;
    AuditResult {
        passed: false,
        parse_failed: Some(true),
        issues: vec![AuditIssue {
            severity: AuditSeverity::Critical,
            category: if en { "System Error" } else { "系统错误" }.to_string(),
            description: if en {
                "Audit output format was invalid and could not be parsed as JSON."
            } else {
                "审稿输出格式异常，无法解析为 JSON"
            }
            .to_string(),
            suggestion: if en {
                "The model may not support reliable structured output. Try a stronger model or inspect the API response format."
            } else {
                "可能是模型不支持结构化输出。尝试换一个更大的模型，或检查 API 返回格式。"
            }
            .to_string(),
            repair_scope: None,
        }],
        summary: if en {
            "Audit output parsing failed"
        } else {
            "审稿输出解析失败"
        }
        .to_string(),
        overall_score: None,
        token_usage: None,
    }
}

/// 精简控制输入块（Planner/Composer 编译产物的审查视角渲染）。
/// 逐字移植 TS `buildReducedControlBlock`。
pub fn build_reduced_control_block(
    chapter_intent: &str,
    context_package: &ContextPackage,
    rule_stack: &RuleStack,
    language: WritingLanguage,
) -> String {
    let selected_context = context_package
        .selected_context
        .iter()
        .map(|entry| {
            match &entry.excerpt {
                Some(excerpt) if !excerpt.is_empty() => {
                    format!("- {}: {} | {excerpt}", entry.source, entry.reason)
                }
                _ => format!("- {}: {}", entry.source, entry.reason),
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let overrides = if !rule_stack.active_overrides.is_empty() {
        rule_stack
            .active_overrides
            .iter()
            .map(|o| format!("- {} -> {}: {} ({})", o.from, o.to, o.reason, o.target))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        "- none".to_string()
    };
    let selected = if selected_context.is_empty() {
        "- none".to_string()
    } else {
        selected_context
    };

    if language == WritingLanguage::En {
        format!(
            "\n## Chapter Control Inputs (compiled by Planner/Composer)\n{chapter_intent}\n\n### Selected Context\n{selected}\n\n### Rule Stack\n- Hard guardrails: {hard}\n- Soft constraints: {soft}\n- Diagnostic rules: {diag}\n\n### Active Overrides\n{overrides}\n",
            hard = join_or_none(&rule_stack.sections.hard, ", "),
            soft = join_or_none(&rule_stack.sections.soft, ", "),
            diag = join_or_none(&rule_stack.sections.diagnostic, ", "),
        )
    } else {
        format!(
            "\n## 本章控制输入（由 Planner/Composer 编译）\n{chapter_intent}\n\n### 已选上下文\n{selected}\n\n### 规则栈\n- 硬护栏：{hard}\n- 软约束：{soft}\n- 诊断规则：{diag}\n\n### 当前覆盖\n{overrides}\n",
            hard = join_or_none(&rule_stack.sections.hard, "、"),
            soft = join_or_none(&rule_stack.sections.soft, "、"),
            diag = join_or_none(&rule_stack.sections.diagnostic, "、"),
        )
    }
}

/// join + 空列表回退（TS `arr.join(sep) || "(none)"/"(无)"`）。
fn join_or_none(items: &[String], sep: &str) -> String {
    let joined = items.join(sep);
    if joined.is_empty() {
        if sep == "、" {
            "(无)".to_string()
        } else {
            "(none)".to_string()
        }
    } else {
        joined
    }
}

// --- ContinuityAuditor 编排（auditChapter）-------------------------------------------

/// 一次聊天完成结果。对齐 TS BaseAgent.chat 返回的 `LLMResponse` 消费面
/// （content + usage）。
#[derive(Debug, Clone, Default)]
pub struct ChatOutcome {
    pub content: String,
    pub usage: Option<AuditTokenUsage>,
}

/// LLM 聊天端口：TS `BaseAgent.chat` / `chatWithSearch` 的最小面。
///
/// 编排层只依赖此 trait（可注入 mock 单测全链路 prompt 构造 + 解析）；
/// 生产实现由未来的 BaseAgent/LLMRouter 移植提供（llm 域 streaming_client 之上）。
#[async_trait::async_trait]
pub trait AuditorChat: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String>;

    /// 联网搜索聊天（eraResearch 开启时用）。默认实现回落到 [`Self::chat`，
    /// 对齐 TS chatWithSearch 搜索失败时回落 chat 的容错路径]。
    async fn chat_with_search(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String> {
        self.chat(messages, temperature).await
    }
}

/// 真相文件覆盖（测试 / 回放注入盘上内容的替代）。对齐 TS `truthFileOverrides`。
#[derive(Debug, Clone, Default)]
pub struct TruthFileOverrides {
    pub current_state: Option<String>,
    pub ledger: Option<String>,
    pub hooks: Option<String>,
}

/// auditChapter 选项。对齐 TS `auditChapter` 的 options 参数。
#[derive(Debug, Clone, Default)]
pub struct AuditChapterOptions {
    pub temperature: Option<f64>,
    pub chapter_intent: Option<String>,
    pub chapter_memo: Option<ChapterMemo>,
    pub context_package: Option<ContextPackage>,
    pub rule_stack: Option<RuleStack>,
    pub truth_file_overrides: Option<TruthFileOverrides>,
}

/// auditChapter 环境依赖（路径与存储注入）。
pub struct AuditChapterCtx<'a> {
    pub project_root: &'a Path,
    /// 内置 genres 目录（TS 经 import.meta.url 定死；Rust 由调用方注入）。
    pub builtin_genres_dir: &'a Path,
    /// prompt pack 三级加载的存储（project 覆盖 → user → builtin）。
    pub prompt_store: &'a dyn StateStore,
}

/// auditChapter 错误。
#[derive(Debug, thiserror::Error)]
pub enum AuditChapterError {
    #[error(transparent)]
    Genre(#[from] ReadGenreProfileError),
    #[error("prompt pack guidance: {0}")]
    PromptPack(#[from] PromptPackPromptNotFoundError),
    #[error("LLM chat failed: {0}")]
    Chat(String),
}

/// agent 名。对齐 TS `ContinuityAuditor.name`。
pub const AUDITOR_NAME: &str = "continuity-auditor";

/// readFileSafe 的统一 fallback（对齐 TS 硬编码 "(文件不存在)"）。
const MISSING_FILE: &str = "(文件不存在)";

/// 章节连续性审查编排。逐字移植 TS `ContinuityAuditor.auditChapter`：
/// 11 路真相文件并行加载（含 Phase 5 current_state 派生回退）→ 规则面加载 →
/// 维度列表 → system/user prompt 构造 → LLM chat（eraResearch 时带搜索）→
/// 四策略解析。
pub async fn audit_chapter(
    ctx: &AuditChapterCtx<'_>,
    chat: &dyn AuditorChat,
    book_dir: &Path,
    chapter_content: &str,
    chapter_number: u32,
    genre: Option<&str>,
    options: &AuditChapterOptions,
) -> Result<AuditResult, AuditChapterError> {
    // Phase 5 整合：current_state.md 仍是架构师种子占位时从 roles + 种子伏笔派生。
    let story = book_dir.join("story");
    let ledger_path = story.join("particle_ledger.md");
    let hooks_path = story.join("pending_hooks.md");
    let style_guide_path = story.join("style_guide.md");
    let subplot_path = story.join("subplot_board.md");
    let arcs_path = story.join("emotional_arcs.md");
    let summaries_path = story.join("chapter_summaries.md");
    let parent_canon_path = story.join("parent_canon.md");
    let fanfic_canon_path = story.join("fanfic_canon.md");
    let info_gaps_path = story.join("info_gaps.md");
    let (
        disk_current_state,
        disk_ledger,
        disk_hooks,
        style_guide_raw,
        subplot_board,
        emotional_arcs,
        character_matrix,
        chapter_summaries,
        parent_canon,
        fanfic_canon,
        volume_outline,
        info_gaps_raw,
    ) = tokio::join!(
        read_current_state_with_fallback(book_dir, MISSING_FILE),
        read_file_safe(&ledger_path),
        read_file_safe(&hooks_path),
        read_file_safe(&style_guide_path),
        read_file_safe(&subplot_path),
        read_file_safe(&arcs_path),
        read_character_context(book_dir, MISSING_FILE),
        read_file_safe(&summaries_path),
        read_file_safe(&parent_canon_path),
        read_file_safe(&fanfic_canon_path),
        read_volume_map(book_dir, MISSING_FILE),
        read_file_safe(&info_gaps_path),
    );

    let overrides = options.truth_file_overrides.as_ref();
    let current_state = overrides
        .and_then(|o| o.current_state.clone())
        .unwrap_or(disk_current_state);
    let ledger = overrides
        .and_then(|o| o.ledger.clone())
        .unwrap_or(disk_ledger);
    let hooks = overrides.and_then(|o| o.hooks.clone()).unwrap_or(disk_hooks);

    let has_parent_canon = parent_canon != MISSING_FILE;
    let has_fanfic_canon = fanfic_canon != MISSING_FILE;

    // 加载上一章全文（细粒度衔接检查）。
    let previous_chapter = load_previous_chapter(book_dir, chapter_number).await;

    // 题材画像 + 书语言 + 书规则。
    let genre_id = genre.unwrap_or("other");
    let (parsed_gp, book_language) = tokio::join!(
        read_genre_profile(ctx.project_root, genre_id, ctx.builtin_genres_dir),
        read_book_language(book_dir),
    );
    let gp = parsed_gp?.profile;
    let parsed_rules = read_book_rules(book_dir).await;
    let book_rules = parsed_rules.as_ref().map(|p| &p.rules);

    // 回退：style_guide.md 缺失时用 book_rules 正文。Phase 5 hotfix 2：
    // ParsedBookRules.body 仅 legacy book_rules.md 来源非空——story_frame.md
    // frontmatter 路径产出空 body，空串不是可用文风指南；缺失/空 = 无回退。
    let legacy_rules_body = parsed_rules
        .as_ref()
        .map(|p| p.body.trim().to_string())
        .unwrap_or_default();
    let style_guide = if style_guide_raw != MISSING_FILE {
        style_guide_raw
    } else if !legacy_rules_body.is_empty() {
        legacy_rules_body
    } else {
        "(无文风指南)".to_string()
    };

    let resolved_language = match book_language.as_deref() {
        Some(lang) => parse_writing_language(lang),
        None => parse_writing_language(&gp.language),
    };
    let is_english = resolved_language == WritingLanguage::En;
    let fanfic_mode = if has_fanfic_canon {
        book_rules.and_then(|rules| rules.fanfic_mode)
    } else {
        None
    };
    let has_info_gaps_file = info_gaps_raw != MISSING_FILE && !info_gaps_raw.trim().is_empty();
    let has_info_gaps = has_info_gaps_file;
    let dimensions = build_dimension_list(
        &gp,
        book_rules,
        resolved_language,
        has_parent_canon,
        fanfic_mode,
        has_info_gaps,
    );
    let dim_list = dimensions
        .iter()
        .map(|d| {
            if d.note.is_empty() {
                format!("{}. {}", d.id, d.name)
            } else if is_english {
                format!("{}. {} ({})", d.id, d.name, d.note)
            } else {
                format!("{}. {}（{}）", d.id, d.name, d.note)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let genre_label = resolve_genre_label(genre_id, &gp.name, resolved_language);

    let protagonist_block = book_rules
        .and_then(|rules| rules.protagonist.as_ref())
        .map(|p| {
            if is_english {
                format!(
                    "\n\nProtagonist lock: {}; personality locks: {}; behavioral constraints: {}.",
                    p.name,
                    join_localized(&p.personality_lock, resolved_language),
                    join_localized(&p.behavioral_constraints, resolved_language),
                )
            } else {
                format!(
                    "\n主角人设锁定：{}，{}，行为约束：{}",
                    p.name,
                    p.personality_lock.join("、"),
                    p.behavioral_constraints.join("、"),
                )
            }
        })
        .unwrap_or_default();

    let search_note = if gp.era_research {
        if is_english {
            "\n\nYou have web-search capability (search_web / fetch_url). For real-world eras, people, events, geography, or policies, you must verify with search_web instead of relying on memory. Cross-check at least 2 sources."
        } else {
            "\n\n你有联网搜索能力（search_web / fetch_url）。对于涉及真实年代、人物、事件、地理、政策的内容，你必须用search_web核实，不可凭记忆判断。至少对比2个来源交叉验证。"
        }
    } else {
        ""
    };

    let system_prompt_base = build_system_prompt_base(
        &genre_label,
        &protagonist_block,
        search_note,
        &dim_list,
        resolved_language,
    );
    let system_prompt = append_prompt_pack_guidance(
        ctx.prompt_store,
        &system_prompt_base,
        &LoadPromptPackPromptInput {
            prompt_id: "longform.auditor".to_string(),
            project_root: Some(ctx.project_root.to_string_lossy().into_owned()),
            user_root: None,
        },
    )
    .await?;

    let ledger_block = if gp.numerical_system {
        if is_english {
            format!("\n## Resource Ledger\n{ledger}")
        } else {
            format!("\n## 资源账本\n{ledger}")
        }
    } else {
        String::new()
    };

    // 智能上下文过滤（与 writer 同逻辑）。
    let filtered_subplots = filter_subplots(&subplot_board);
    let filtered_arcs = filter_emotional_arcs(&emotional_arcs, chapter_number, None);
    let filtered_matrix = filter_character_matrix(
        &character_matrix,
        &volume_outline,
        book_rules
            .and_then(|rules| rules.protagonist.as_ref())
            .map(|p| p.name.as_str()),
    );
    let filtered_summaries = filter_summaries(&chapter_summaries, chapter_number, None);
    let filtered_hooks = filter_hooks(&hooks);

    let governed_memory_blocks = options
        .context_package
        .as_ref()
        .map(|pkg| build_governed_memory_evidence_blocks(pkg, Some(resolved_language)));

    let hooks_block = governed_memory_blocks
        .as_ref()
        .and_then(|b| b.hooks_block.clone())
        .unwrap_or_else(|| {
            if filtered_hooks != MISSING_FILE {
                if is_english {
                    format!("\n## Pending Hooks\n{filtered_hooks}\n")
                } else {
                    format!("\n## 伏笔池\n{filtered_hooks}\n")
                }
            } else {
                String::new()
            }
        });
    let subplot_block = block_or_empty(&filtered_subplots, MISSING_FILE, |s| {
        if is_english {
            format!("\n## Subplot Board\n{s}\n")
        } else {
            format!("\n## 支线进度板\n{s}\n")
        }
    });
    let emotional_block = block_or_empty(&filtered_arcs, MISSING_FILE, |s| {
        if is_english {
            format!("\n## Emotional Arcs\n{s}\n")
        } else {
            format!("\n## 情感弧线\n{s}\n")
        }
    });
    let matrix_block = block_or_empty(&filtered_matrix, MISSING_FILE, |s| {
        if is_english {
            format!("\n## Character Interaction Matrix\n{s}\n")
        } else {
            format!("\n## 角色交互矩阵\n{s}\n")
        }
    });
    let summaries_block = governed_memory_blocks
        .as_ref()
        .and_then(|b| b.summaries_block.clone())
        .unwrap_or_else(|| {
            if filtered_summaries != MISSING_FILE {
                if is_english {
                    format!("\n## Chapter Summaries (for pacing checks)\n{filtered_summaries}\n")
                } else {
                    format!("\n## 章节摘要（用于节奏检查）\n{filtered_summaries}\n")
                }
            } else {
                String::new()
            }
        });
    let volume_summaries_block = governed_memory_blocks
        .as_ref()
        .and_then(|b| b.volume_summaries_block.clone())
        .unwrap_or_default();

    let info_gap_block = if has_info_gaps_file {
        if is_english {
            format!(
                "\n## Info Gap Ledger (secrets: knows / readerKnows / keywords / registered chapter — audit dims 38/39)\n{info_gaps_raw}\n"
            )
        } else {
            format!(
                "\n## 信息差账本（秘密/知情人/读者已知/关键词/登记章——对应维度 38/39）\n{info_gaps_raw}\n"
            )
        }
    } else {
        String::new()
    };

    let canon_block = if has_parent_canon {
        if is_english {
            format!("\n## Mainline Canon Reference (for spinoff audit)\n{parent_canon}\n")
        } else {
            format!("\n## 正传正典参照（番外审查专用）\n{parent_canon}\n")
        }
    } else {
        String::new()
    };

    let fanfic_canon_block = if has_fanfic_canon {
        if is_english {
            format!("\n## Fanfic Canon Reference (for fanfic audit)\n{fanfic_canon}\n")
        } else {
            format!("\n## 同人正典参照（同人审查专用）\n{fanfic_canon}\n")
        }
    } else {
        String::new()
    };

    let memo_block = options
        .chapter_memo
        .as_ref()
        .map(|memo| {
            if is_english {
                format!(
                    "\n## Chapter Memo (for memo drift checks)\nGoal: {}\n\n{}\n",
                    memo.goal, memo.body
                )
            } else {
                format!(
                    "\n## 章节备忘（用于 memo 偏离检测）\ngoal：{}\n\n{}\n",
                    memo.goal, memo.body
                )
            }
        })
        .unwrap_or_default();

    let reduced_control_block = match (
        options.chapter_intent.as_deref(),
        options.context_package.as_ref(),
        options.rule_stack.as_ref(),
    ) {
        (Some(intent), Some(pkg), Some(stack)) => build_reduced_control_block(
            intent,
            pkg,
            stack,
            resolved_language,
        ),
        _ => String::new(),
    };
    let style_guide_block = if reduced_control_block.is_empty() {
        if is_english {
            format!("\n## Style Guide\n{style_guide}")
        } else {
            format!("\n## 文风指南\n{style_guide}")
        }
    } else {
        String::new()
    };

    let prev_chapter_block = if !previous_chapter.is_empty() {
        if is_english {
            format!("\n## Previous Chapter Full Text (for transition checks)\n{previous_chapter}\n")
        } else {
            format!("\n## 上一章全文（用于衔接检查）\n{previous_chapter}\n")
        }
    } else {
        String::new()
    };

    let user_prompt = if is_english {
        format!(
            "Review chapter {chapter_number}.\n\n## Current State Card\n{current_state}\n{ledger_block}\n{hooks_block}{volume_summaries_block}{subplot_block}{emotional_block}{matrix_block}{summaries_block}{info_gap_block}{canon_block}{fanfic_canon_block}{reduced_control_block}{memo_block}{prev_chapter_block}{style_guide_block}\n\n## Chapter Content Under Review\n{chapter_content}"
        )
    } else {
        format!(
            "请审查第{chapter_number}章。\n\n## 当前状态卡\n{current_state}\n{ledger_block}\n{hooks_block}{volume_summaries_block}{subplot_block}{emotional_block}{matrix_block}{summaries_block}{info_gap_block}{canon_block}{fanfic_canon_block}{reduced_control_block}{memo_block}{prev_chapter_block}{style_guide_block}\n\n## 待审章节内容\n{chapter_content}"
        )
    };

    let messages = vec![
        LLMMessage {
            role: LLMRole::System,
            content: system_prompt,
            tool_calls: None, tool_call_id: None,
        },
        LLMMessage {
            role: LLMRole::User,
            content: user_prompt,
            tool_calls: None, tool_call_id: None,
        },
    ];
    let temperature = options.temperature.unwrap_or(0.3);

    // eraResearch 开启时用联网搜索核实事实。
    let response = if gp.era_research {
        chat.chat_with_search(messages, temperature).await
    } else {
        chat.chat(messages, temperature).await
    }
    .map_err(AuditChapterError::Chat)?;

    let mut result = parse_audit_result(&response.content, resolved_language);
    result.token_usage = response.usage;
    Ok(result)
}

/// "zh"/"en" 字符串 → WritingLanguage（其余按 zh，对齐 TS `=== "en"` 判定）。
fn parse_writing_language(s: &str) -> WritingLanguage {
    if s == "en" {
        WritingLanguage::En
    } else {
        WritingLanguage::Zh
    }
}

/// 读文件，任何失败 → "(文件不存在)"。对齐 TS `readFileSafe`。
async fn read_file_safe(path: &Path) -> String {
    tokio::fs::read_to_string(path)
        .await
        .unwrap_or_else(|_| MISSING_FILE.to_string())
}

/// 加载上一章全文（chapters/ 目录中 `{prev:04}` 前缀 + .md 的首个文件）。
/// 对齐 TS `loadPreviousChapter`。
async fn load_previous_chapter(book_dir: &Path, current_chapter: u32) -> String {
    if current_chapter <= 1 {
        return String::new();
    }
    let chapters_dir = book_dir.join("chapters");
    let Ok(mut entries) = tokio::fs::read_dir(&chapters_dir).await else {
        return String::new();
    };
    let padded_prev = format!("{:04}", current_chapter - 1);
    loop {
        match entries.next_entry().await {
            Ok(Some(entry)) => {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if name.starts_with(&padded_prev) && name.ends_with(".md") {
                    return tokio::fs::read_to_string(entry.path())
                        .await
                        .unwrap_or_default();
                }
            }
            _ => return String::new(),
        }
    }
}

/// 过滤结果非缺失时渲染块，缺失时空串。对齐 TS 的 `!== "(文件不存在)"` 三元。
fn block_or_empty(filtered: &str, missing: &str, render: impl FnOnce(&str) -> String) -> String {
    if filtered != missing {
        render(filtered)
    } else {
        String::new()
    }
}

/// system prompt 基底（prompt pack 指导追加前）。逐字移植 TS `systemPromptBase`。
fn build_system_prompt_base(
    genre_label: &str,
    protagonist_block: &str,
    search_note: &str,
    dim_list: &str,
    language: WritingLanguage,
) -> String {
    if language == WritingLanguage::En {
        format!(
            "You are a strict {genre_label} web-fiction structural editor. Audit the chapter for completion and structure, not for prose craft. ALL OUTPUT MUST BE IN ENGLISH.{protagonist_block}{search_note}\n\n## Reviewer Scope (hard constraints)\n\nYou audit completion and structure only. Your job is to decide whether the chapter delivers the plan, keeps characters and timelines intact, and moves the book forward. Wording, sentence rhythm, paragraph shape, punctuation, imagery, and other prose-surface choices are NOT yours — those belong to the Polisher pass that runs after you. If you notice prose-surface issues, you may flag them with severity \"info\" so the Polisher can see them, but they do not count toward passed / overall_score and they must never be critical.\n\nYou audit twelve structural reader-pain patterns: dragging / flat openings, blurry worldbuilding disconnected from reality, contradictory character setup, tangled POV, mainline drift or stagnation, weak conflict with missing payoff, pacing loss of control and abrupt transitions, character inconsistency across the arc, thin/one-note characters without contrast, stiff emotion expression and abrupt relationship jumps, imbalanced cheats/power gifts, and settings that never land in concrete action. Alongside these, keep the engineering dimensions listed below (OOC, timeline coherence, information boundary, hook debt, cross-chapter repetition, lexical fatigue, length band, title fatigue, paragraph shape).\n\nSparse chapter_memo is legitimate. Breather / aftermath / transition chapters may ship a memo that only contains goal + a skeleton body — do NOT flag such memos as incomplete, and do NOT penalise the chapter for lacking content against sections the memo itself does not populate. Judge drift only against what the memo actually says.\n\nIf the chapter memo, rule stack, or supplied context specifies content proportions between lines (politics/romance, career/relationship, case/character, etc.), audit whether those lines appear as actual scenes, dialogue, action, or relationship movement. A line that is only summarized in one sentence counts as missing. Mark it critical only when the memo explicitly required it for this chapter.\n\nFor every issue, set repair_scope as a typed routing hint: \"local\" for wording, paragraph shape, small repetition, or narrow sentence-level fixes; \"structural\" for plot drift, timeline break, missing scene/payoff, character logic collapse, POV/knowledge boundary failure, or anything requiring a rewritten scene/chapter; \"unknown\" only when you genuinely cannot decide.\n\nAudit dimensions:\n{dim_list}\n\nOutput format MUST be JSON:\n{{\n  \"passed\": true/false,\n  \"overall_score\": 0-100,\n  \"issues\": [\n\t    {{\n\t      \"severity\": \"critical|warning|info\",\n\t      \"repair_scope\": \"local|structural|unknown\",\n\t      \"category\": \"dimension name\",\n\t      \"description\": \"specific issue description\",\n\t      \"suggestion\": \"fix suggestion\"\n\t    }}\n  ],\n  \"summary\": \"one-sentence audit conclusion\"\n}}\n\npassed is false ONLY when critical-severity issues exist.\n\noverall_score calibration:\n- 95-100: Publishable as-is, no noticeable issues\n- 85-94: Minor blemishes but smooth reading, the reader won't break immersion\n- 75-84: Noticeable problems but the story backbone holds, needs revision but not urgent\n- 65-74: Multiple issues hurt the reading experience, pacing or continuity has gaps\n- < 65: Structural breakdown, needs major rewrite\nScore holistically — do not let a single minor issue tank the score."
        )
    } else {
        format!(
            "你是一位严格的{genre_label}网络小说结构审稿编辑。你只审完成度 + 结构，不审文笔。{protagonist_block}{search_note}\n\n## 审稿边界（硬约束）\n\n你不审文笔、不审排版、不审句式——这些归 Polisher。你发现的文笔问题只能以 severity=\"info\" 标注供 Polisher 参考，不计入 reviewer 的 passed/overall_score，也绝不可标为 critical。\n\n你审 12 条结构类雷点：开篇拖沓/平淡、世界观模糊脱现实、人设矛盾、视角杂乱、主线偏离/停滞、冲突乏力爽点缺失、节奏失控过渡生硬、人设前后矛盾、人物单薄无反差、情感表达生硬/关系突兀、金手指失衡、设定无落地。同时保留工程维度（OOC、timeline 一致、信息越界、hook-debt、跨章重复、词汇疲劳、章节字数、标题疲劳、段落形状）。\n\n稀疏 memo 是合法状态。喘息章 / 后效章 / 过渡章的 memo 可以只有 goal + 骨架 body——此类 memo 不判 incomplete，也不能因为 memo 没写的段落就扣成稿的分。只按 memo 实际写出来的内容判偏离。\n\n如果章节备忘、规则栈或输入上下文明确指定多条剧情线的比例（权谋/感情、事业/恋爱、案件/人物等），要审它们是否真正落成了场景、对话、行动或关系变化。只用一句总结带过的线，视为缺失。只有当 memo 明确要求本章必须推进该线时，才标 critical。\n\n每条 issue 必须给 repair_scope 作为 typed 路由提示：\"local\" 表示措辞、段落形状、小重复、句段级小修；\"structural\" 表示主线偏离、时间线断裂、场面/回报缺失、人物逻辑崩、视角/信息边界失败，或任何需要重写场景/整章的问题；只有确实无法判断时才写 \"unknown\"。\n\n审查维度：\n{dim_list}\n\n输出格式必须为 JSON：\n{{\n  \"passed\": true/false,\n  \"overall_score\": 0-100,\n  \"issues\": [\n\t    {{\n\t      \"severity\": \"critical|warning|info\",\n\t      \"repair_scope\": \"local|structural|unknown\",\n\t      \"category\": \"审查维度名称\",\n\t      \"description\": \"具体问题描述\",\n\t      \"suggestion\": \"修改建议\"\n\t    }}\n  ],\n  \"summary\": \"一句话总结审查结论\"\n}}\n\n只有当存在 critical 级别问题时，passed 才为 false。\n\noverall_score 评分校准：\n- 95-100：可直接发布，无明显问题\n- 85-94：有小瑕疵但整体流畅可读，读者不会出戏\n- 75-84：有明显问题但故事主干完整，需要修但不紧急\n- 65-74：多处影响阅读体验的问题，节奏或连续性有断裂\n- < 65：结构性问题，需要大幅改写\n综合评分，不要因为单一小问题大幅拉低分数。"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_repair_scope_accepts_known_values() {
        assert_eq!(
            normalize_repair_scope(&serde_json::json!("local")),
            Some(RepairScope::Local)
        );
        assert_eq!(
            normalize_repair_scope(&serde_json::json!("structural")),
            Some(RepairScope::Structural)
        );
        assert_eq!(
            normalize_repair_scope(&serde_json::json!("unknown")),
            Some(RepairScope::Unknown)
        );
        assert_eq!(normalize_repair_scope(&serde_json::json!("bogus")), None);
        assert_eq!(normalize_repair_scope(&serde_json::json!(42)), None);
    }

    #[test]
    fn contains_chinese_detects_cjk() {
        assert!(contains_chinese("abc中文"));
        assert!(contains_chinese("中"));
        assert!(!contains_chinese("pure ascii"));
        assert!(!contains_chinese(""));
    }

    #[test]
    fn resolve_genre_label_zh_or_non_chinese_profile_uses_profile_name() {
        assert_eq!(
            resolve_genre_label("xianxia", "仙侠", WritingLanguage::Zh),
            "仙侠"
        );
        // 英文语言但 profileName 无中文 → 仍用 profileName。
        assert_eq!(
            resolve_genre_label("other", "My Profile", WritingLanguage::En),
            "My Profile"
        );
    }

    #[test]
    fn resolve_genre_label_en_with_chinese_profile_falls_back() {
        // 英文语言 + profileName 含中文 + genre=other → "general"。
        assert_eq!(
            resolve_genre_label("other", "仙侠", WritingLanguage::En),
            "general"
        );
        // 否则 genre 的 _- 转空格。
        assert_eq!(
            resolve_genre_label("urban-fantasy", "仙侠", WritingLanguage::En),
            "urban fantasy"
        );
    }

    #[test]
    fn dimension_name_covers_all_39_in_both_languages() {
        // G7a/341 号：维度表扩至 39（38 泄密机检 / 39 废笔机检）。
        for id in 1..=39u32 {
            assert!(dimension_name(id, WritingLanguage::Zh).is_some(), "维度 {id} 缺中文");
            assert!(dimension_name(id, WritingLanguage::En).is_some(), "维度 {id} 缺英文");
        }
        assert_eq!(dimension_name(0, WritingLanguage::Zh), None);
        assert_eq!(dimension_name(40, WritingLanguage::En), None);
    }

    #[test]
    fn dimension_name_specific_labels() {
        assert_eq!(dimension_name(1, WritingLanguage::Zh), Some("OOC检查"));
        assert_eq!(dimension_name(1, WritingLanguage::En), Some("OOC Check"));
        assert_eq!(dimension_name(37, WritingLanguage::Zh), Some("正典事件一致性"));
    }

    #[test]
    fn join_localized_uses_correct_separator() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(join_localized(&items, WritingLanguage::Zh), "a、b、c");
        assert_eq!(join_localized(&items, WritingLanguage::En), "a, b, c");
        assert_eq!(join_localized(&[], WritingLanguage::Zh), "");
    }

    #[test]
    fn format_fanfic_severity_note_per_language() {
        assert_eq!(
            format_fanfic_severity_note(AuditSeverity::Critical, WritingLanguage::En),
            "Strict check."
        );
        assert_eq!(
            format_fanfic_severity_note(AuditSeverity::Critical, WritingLanguage::Zh),
            "（严格检查）"
        );
        assert_eq!(
            format_fanfic_severity_note(AuditSeverity::Info, WritingLanguage::Zh),
            "（仅记录，不判定失败）"
        );
        assert_eq!(
            format_fanfic_severity_note(AuditSeverity::Warning, WritingLanguage::En),
            "Warning level."
        );
    }

    // --- build_dimension_note / build_dimension_list ------------------------------

    fn test_gp() -> GenreProfile {
        GenreProfile {
            name: "仙侠".to_string(),
            id: "xianxia".to_string(),
            fatigue_words: vec!["震惊".to_string()],
            satisfaction_types: vec!["扮猪吃虎".to_string(), "越级挑战".to_string()],
            audit_dimensions: vec![1.0, 6.0, 10.0],
            ..GenreProfile::default()
        }
    }

    #[test]
    fn dimension_note_ooc_mode_relaxes_dim1() {
        let gp = test_gp();
        let note = build_dimension_note(
            1,
            WritingLanguage::Zh,
            &gp,
            None,
            Some(FanficMode::Ooc),
            None,
        );
        assert!(note.contains("OOC模式下角色可偏离性格底色"));
        let en = build_dimension_note(
            1,
            WritingLanguage::En,
            &gp,
            None,
            Some(FanficMode::Ooc),
            None,
        );
        assert!(en.contains("In OOC mode"));
    }

    #[test]
    fn dimension_note_canon_mode_tightens_dim1() {
        let gp = test_gp();
        let note = build_dimension_note(
            1,
            WritingLanguage::Zh,
            &gp,
            None,
            Some(FanficMode::Canon),
            None,
        );
        assert!(note.contains("原作向同人"));
    }

    #[test]
    fn dimension_note_fatigue_words_override_wins() {
        let gp = test_gp();
        let rules = BookRules {
            fatigue_words_override: vec!["瞬间".to_string(), "顿时".to_string()],
            ..BookRules::default()
        };
        let note = build_dimension_note(10, WritingLanguage::Zh, &gp, Some(&rules), None, None);
        assert!(note.contains("高疲劳词：瞬间、顿时"));

        // 无 override → gp.fatigueWords。
        let note = build_dimension_note(10, WritingLanguage::Zh, &gp, None, None, None);
        assert!(note.contains("高疲劳词：震惊"));

        // en 版逗号连接。
        let en = build_dimension_note(10, WritingLanguage::En, &gp, None, None, None);
        assert!(en.starts_with("Fatigue words: 震惊. Also check"));
    }

    #[test]
    fn dimension_note_15_returns_short_form_when_satisfaction_types_present() {
        let gp = test_gp();
        let note = build_dimension_note(15, WritingLanguage::Zh, &gp, None, None, None);
        // satisfactionTypes 非空 → 提前返回简版（无 v10 欲望驱动长文）。
        assert_eq!(note, "爽点类型：扮猪吃虎、越级挑战");

        // satisfactionTypes 空 → v10 增强版，base 恒空（不可达分支钉死）。
        let mut sparse = test_gp();
        sparse.satisfaction_types = vec![];
        let note = build_dimension_note(15, WritingLanguage::Zh, &sparse, None, None, None);
        assert!(note.starts_with("检查欲望驱动："));
    }

    #[test]
    fn dimension_note_25_v10_three_questions_wins() {
        let gp = test_gp();
        let note = build_dimension_note(25, WritingLanguage::Zh, &gp, None, None, None);
        assert!(note.contains("人设三问检查"));
        // switch case 25（情绪弧线版）不可达——同一输入永不产出该文案。
        assert!(!note.contains("情绪压力形态"));
    }

    #[test]
    fn dimension_note_12_era_constraints() {
        use crate::models::book_rules::EraConstraints;
        let gp = test_gp();
        let rules = BookRules {
            era_constraints: Some(EraConstraints {
                period: Some("宋代".to_string()),
                region: Some("江南".to_string()),
                ..EraConstraints::default()
            }),
            ..BookRules::default()
        };
        let note = build_dimension_note(12, WritingLanguage::Zh, &gp, Some(&rules), None, None);
        assert_eq!(note, "年代：宋代，江南");

        // period/region 全空 → 落到 default（空串）。
        let rules2 = BookRules {
            era_constraints: Some(EraConstraints {
                enabled: true,
                ..EraConstraints::default()
            }),
            ..BookRules::default()
        };
        assert_eq!(
            build_dimension_note(12, WritingLanguage::Zh, &gp, Some(&rules2), None, None),
            ""
        );
    }

    #[test]
    fn dimension_note_fanfic_34_37_appends_severity() {
        let gp = test_gp();
        let cfg = get_fanfic_dimension_config(FanficMode::Canon, &[]);
        let note = build_dimension_note(
            34,
            WritingLanguage::Zh,
            &gp,
            None,
            Some(FanficMode::Canon),
            Some(&cfg),
        );
        // canon/34 → critical 严格检查。
        assert!(note.contains("检查角色的语癖"));
        assert!(note.ends_with("（严格检查）"));

        let en_note = build_dimension_note(
            35,
            WritingLanguage::En,
            &gp,
            None,
            Some(FanficMode::Canon),
            Some(&cfg),
        );
        assert!(en_note.contains("world rules documented"));
        assert!(en_note.ends_with("Strict check."));

        // 无 fanficConfig → 空串。
        assert_eq!(
            build_dimension_note(34, WritingLanguage::Zh, &gp, None, None, None),
            ""
        );
    }

    #[test]
    fn dimension_note_fanfic_config_note_wins_in_zh() {
        let gp = test_gp();
        let cfg = get_fanfic_dimension_config(FanficMode::Ooc, &[]);
        let note = build_dimension_note(
            34,
            WritingLanguage::Zh,
            &gp,
            None,
            Some(FanficMode::Ooc),
            Some(&cfg),
        );
        // notes 命中（zh）→ config 注记（baseNote + info 严重度标签）。
        assert!(note.contains("（仅记录，不判定失败）"));
    }

    #[test]
    fn dimension_note_6_hook_debt_escalation() {
        let gp = test_gp();
        let note = build_dimension_note(6, WritingLanguage::Zh, &gp, None, None, None);
        assert!(note.contains("Phase 7 hook-debt 升级规则"));
        assert!(note.contains("promoted=true"));
    }

    #[test]
    fn dimension_note_unknown_id_is_empty() {
        let gp = test_gp();
        assert_eq!(
            build_dimension_note(2, WritingLanguage::Zh, &gp, None, None, None),
            ""
        );
        assert_eq!(
            build_dimension_note(27, WritingLanguage::Zh, &gp, None, None, None),
            ""
        );
    }

    #[test]
    fn dimension_list_base_universal_and_additional() {
        let gp = test_gp();
        let rules = BookRules {
            additional_audit_dimensions: vec![
                AuditDimension::Number(5.0),
                AuditDimension::Text("战力崩坏".to_string()), // 精确匹配 → 4
                AuditDimension::Text("敏感".to_string()),     // 模糊匹配 → 27
            ],
            ..BookRules::default()
        };
        let dims = build_dimension_list(&gp, Some(&rules), WritingLanguage::Zh, false, None, false);
        let ids: Vec<u32> = dims.iter().map(|d| d.id).collect();
        assert_eq!(ids, vec![1, 4, 5, 6, 10, 27, 32, 33]);

        // 名称注记正确挂载。
        let d4 = dims.iter().find(|d| d.id == 4).unwrap();
        assert_eq!(d4.name, "战力崩坏");
    }

    #[test]
    fn dimension_list_always_active_32_33_and_sorted() {
        let gp = test_gp();
        let dims = build_dimension_list(&gp, None, WritingLanguage::Zh, false, None, false);
        let ids: Vec<u32> = dims.iter().map(|d| d.id).collect();
        // gp.auditDimensions [1,6,10] + 恒加 [32,33]。
        assert_eq!(ids, vec![1, 6, 10, 32, 33]);
    }

    #[test]
    fn dimension_list_era_research_adds_12() {
        let mut gp = test_gp();
        gp.era_research = true;
        let dims = build_dimension_list(&gp, None, WritingLanguage::Zh, false, None, false);
        assert!(dims.iter().any(|d| d.id == 12));

        // eraConstraints.enabled 同样触发。
        use crate::models::book_rules::EraConstraints;
        let gp = test_gp();
        let rules = BookRules {
            era_constraints: Some(EraConstraints {
                enabled: true,
                ..EraConstraints::default()
            }),
            ..BookRules::default()
        };
        let dims = build_dimension_list(&gp, Some(&rules), WritingLanguage::Zh, false, None, false);
        assert!(dims.iter().any(|d| d.id == 12));
    }

    #[test]
    fn dimension_list_parent_canon_adds_spinoff_dims_unless_fanfic() {
        let gp = test_gp();
        let dims = build_dimension_list(&gp, None, WritingLanguage::Zh, true, None, false);
        assert!(dims.iter().any(|d| d.id == 28));
        assert!(dims.iter().any(|d| d.id == 31));

        // 同人模式下不激活番外维度，改激活同人维度 34-37。
        let dims = build_dimension_list(&gp, None, WritingLanguage::Zh, true, Some(FanficMode::Au), false);
        assert!(!dims.iter().any(|d| (28..=31).contains(&d.id)));
        assert!(dims.iter().any(|d| (34..=37).contains(&d.id)));
    }

    #[test]
    fn dimension_list_fuzzy_name_match_and_non_integer_skipped() {
        let gp = test_gp();
        let rules = BookRules {
            additional_audit_dimensions: vec![
                AuditDimension::Text("Timeline Check".to_string()), // en 精确 → 2
                AuditDimension::Text("pacing".to_string()),         // en 模糊（包含于 Pacing Check? 否）→
                AuditDimension::Number(1.5),                        // 非整数 → 无名跳过
            ],
            ..BookRules::default()
        };
        let dims = build_dimension_list(&gp, Some(&rules), WritingLanguage::Zh, false, None, false);
        let ids: Vec<u32> = dims.iter().map(|d| d.id).collect();
        assert!(ids.contains(&2));
        // "pacing" 小写包含于… TS includes 区分大小写：name "Pacing Check" 不含 "pacing"，
        // 但 "pacing".includes("Pacing Check") 为 false → 无匹配。1.5 无名 → 跳过。
        assert!(!ids.contains(&1) || gp.audit_dimensions.contains(&1.0)); // 1 来自 gp
        assert_eq!(ids.last().copied(), Some(33));
    }

    #[test]
    fn dimension_list_note_rendering_language() {
        let gp = test_gp();
        let dims = build_dimension_list(&gp, None, WritingLanguage::En, false, None, false);
        let d1 = dims.iter().find(|d| d.id == 1).unwrap();
        assert_eq!(d1.name, "OOC Check");
        assert!(d1.note.is_empty()); // 维度 1 无 fanfic → 无注记

        let d10 = dims.iter().find(|d| d.id == 10).unwrap();
        assert!(d10.note.starts_with("Fatigue words: 震惊"));
    }

    // --- extract_balanced_json / parse_audit_result --------------------------------

    #[test]
    fn extract_balanced_json_finds_first_object() {
        assert_eq!(
            extract_balanced_json(r#"prefix {"a":1,"b":{"c":2}} tail"#),
            Some(r#"{"a":1,"b":{"c":2}}"#)
        );
        assert_eq!(extract_balanced_json("no braces"), None);
        assert_eq!(extract_balanced_json(r#"{"a": {"b": 1"#), None); // 不平衡
        // 嵌套字符串内的花括号——TS 同样按字符计数不感知字符串语义：
        // "a}b" 中的 } 使 depth 归零，提取在 "a} 处截断。
        assert_eq!(
            extract_balanced_json(r#"- {"x": "a}b"}"#),
            Some(r#"{"x": "a}"#)
        );
    }

    #[test]
    fn parse_audit_result_strategy1_balanced_json() {
        let content = r#"审查结论：{"passed": true, "overall_score": 88, "issues": [{"severity":"warning","repair_scope":"local","category":"节奏检查","description":"第2段拖沓","suggestion":"压缩"}], "summary": "整体合格"}"#;
        let r = parse_audit_result(content, WritingLanguage::Zh);
        assert!(r.passed);
        assert_eq!(r.overall_score, Some(88));
        assert_eq!(r.summary, "整体合格");
        assert_eq!(r.issues.len(), 1);
        assert_eq!(r.issues[0].severity, AuditSeverity::Warning);
        assert_eq!(r.issues[0].repair_scope, Some(RepairScope::Local));
        assert_eq!(r.parse_failed, None);
    }

    #[test]
    fn parse_audit_result_strategy2_pure_json() {
        let content = r#"  {"passed": false, "overall_score": 150, "issues": [], "summary": ""}  "#;
        let r = parse_audit_result(content, WritingLanguage::Zh);
        assert!(!r.passed);
        assert_eq!(r.overall_score, Some(100)); // clamp
        assert_eq!(r.issues.len(), 0);
    }

    #[test]
    fn parse_audit_result_strategy3_code_block() {
        let content = "结论如下：\n```json\n{\"passed\": true, \"overall_score\": 42.6, \"issues\": [{\"severity\":\"info\"}], \"summary\": \"ok\"}\n```\n完";
        let r = parse_audit_result(content, WritingLanguage::En);
        assert!(r.passed);
        assert_eq!(r.overall_score, Some(43)); // round
        assert_eq!(r.issues.len(), 1);
        assert_eq!(r.issues[0].severity, AuditSeverity::Info);
        assert_eq!(r.issues[0].category, "Uncategorized"); // en 回退
    }

    #[test]
    fn parse_audit_result_strategy4_regex_fallback() {
        // 策略 4 仅在「无任何平衡对象」时到达：issue 对象故意不闭合
        // （若闭合，策略 1 会提取它并以 passed 缺失视为合法返回）。
        let content = r#"blah "passed": false, "issues": [{"severity":"critical","category":"OOC检查","description":"角色崩坏"], "summary": "不合格""#;
        let r = parse_audit_result(content, WritingLanguage::Zh);
        assert!(!r.passed);
        assert_eq!(r.summary, "不合格");
        // 不完整对象不匹配 issue 正则 → 空列表。
        assert_eq!(r.issues.len(), 0);
    }

    #[test]
    fn parse_audit_result_strategy1_accepts_missing_passed() {
        // 平衡对象缺 passed 字段：TS typeof undefined !== "boolean" 为假 → 不拒绝，
        // 按 passed=false / issues=[] / summary="" 返回。
        let r = parse_audit_result(r#"前缀 {"issues": []} 后缀"#, WritingLanguage::Zh);
        assert!(!r.passed);
        assert_eq!(r.issues.len(), 0);
        assert_eq!(r.summary, "");
        assert_eq!(r.parse_failed, None);
    }

    #[test]
    fn parse_audit_result_parse_failed_fallback() {
        let r = parse_audit_result("模型完全不配合，输出纯散文。", WritingLanguage::Zh);
        assert!(!r.passed);
        assert_eq!(r.parse_failed, Some(true));
        assert_eq!(r.issues.len(), 1);
        assert_eq!(r.issues[0].severity, AuditSeverity::Critical);
        assert_eq!(r.issues[0].category, "系统错误");
        assert_eq!(r.summary, "审稿输出解析失败");

        let en = parse_audit_result("nope", WritingLanguage::En);
        assert_eq!(en.issues[0].category, "System Error");
    }

    #[test]
    fn parse_audit_result_rejects_non_bool_passed() {
        // passed 是字符串（非 boolean 非 undefined）→ tryParse 返回 null → 兜底。
        let r = parse_audit_result(r#"{"passed": "yes"}"#, WritingLanguage::Zh);
        assert_eq!(r.parse_failed, Some(true));
    }

    #[test]
    fn parse_audit_result_score_field_aliases_and_clamp() {
        // overallScore 驼峰别名。
        let r = parse_audit_result(r#"{"passed": true, "overallScore": 72.4}"#, WritingLanguage::Zh);
        assert_eq!(r.overall_score, Some(72));
        // null score → overallScore 不可用 → None。
        let r = parse_audit_result(r#"{"passed": true, "overall_score": null, "overallScore": "x"}"#, WritingLanguage::Zh);
        assert_eq!(r.overall_score, None);
        // 负分 clamp 到 0。
        let r = parse_audit_result(r#"{"passed": true, "overall_score": -5}"#, WritingLanguage::Zh);
        assert_eq!(r.overall_score, Some(0));
    }

    #[test]
    fn parse_audit_result_repair_scope_aliases() {
        let r = parse_audit_result(
            r#"{"passed": true, "issues": [{"repairScope": "structural"}]}"#,
            WritingLanguage::Zh,
        );
        assert_eq!(r.issues[0].repair_scope, Some(RepairScope::Structural));
        // 非法值 → None。
        let r = parse_audit_result(
            r#"{"passed": true, "issues": [{"repair_scope": "wild"}]}"#,
            WritingLanguage::Zh,
        );
        assert_eq!(r.issues[0].repair_scope, None);
    }

    // --- build_reduced_control_block ------------------------------------------------

    fn test_context_package() -> ContextPackage {
        use crate::models::input_governance::ContextSource;
        ContextPackage {
            chapter: 3,
            selected_context: vec![
                ContextSource {
                    source: "story/pending_hooks.md#H1".to_string(),
                    reason: "主线伏笔".to_string(),
                    excerpt: Some("H1 玉佩悬念".to_string()),
                },
                ContextSource {
                    source: "story/chapter_summaries.md".to_string(),
                    reason: "前情".to_string(),
                    excerpt: None,
                },
            ],
        }
    }

    fn test_rule_stack() -> RuleStack {
        use crate::models::input_governance::{ActiveOverride, RuleLayer, RuleLayerScope};
        RuleStack {
            layers: vec![RuleLayer {
                id: "book".to_string(),
                name: "书级规则".to_string(),
                precedence: 10,
                scope: RuleLayerScope::Book,
            }],
            sections: crate::models::input_governance::RuleStackSections {
                hard: vec!["主角不死".to_string()],
                soft: vec![],
                diagnostic: vec!["禁用第一人称".to_string()],
            },
            override_edges: vec![],
            active_overrides: vec![ActiveOverride {
                from: "卷级节奏".to_string(),
                to: "章级节奏".to_string(),
                target: "pacing".to_string(),
                reason: "卷尾冲刺".to_string(),
            }],
        }
    }

    #[test]
    fn reduced_control_block_zh_rendering() {
        let block = build_reduced_control_block(
            "本章目标：主角突破",
            &test_context_package(),
            &test_rule_stack(),
            WritingLanguage::Zh,
        );
        assert!(block.starts_with("\n## 本章控制输入（由 Planner/Composer 编译）\n本章目标：主角突破\n"));
        assert!(block.contains("- story/pending_hooks.md#H1: 主线伏笔 | H1 玉佩悬念"));
        assert!(block.contains("- story/chapter_summaries.md: 前情"));
        assert!(block.contains("- 硬护栏：主角不死"));
        assert!(block.contains("- 软约束：(无)"));
        assert!(block.contains("- 诊断规则：禁用第一人称"));
        assert!(block.contains("- 卷级节奏 -> 章级节奏: 卷尾冲刺 (pacing)"));
        assert!(block.ends_with("\n"));
    }

    #[test]
    fn reduced_control_block_en_and_empty_overrides() {
        let mut stack = test_rule_stack();
        stack.active_overrides = vec![];
        let block = build_reduced_control_block(
            "Goal",
            &test_context_package(),
            &stack,
            WritingLanguage::En,
        );
        assert!(block.contains("## Chapter Control Inputs (compiled by Planner/Composer)"));
        assert!(block.contains("- Hard guardrails: 主角不死"));
        assert!(block.contains("- Soft constraints: (none)"));
        assert!(block.contains("### Active Overrides\n- none\n"));
    }

    #[test]
    fn reduced_control_block_empty_selected_context() {
        let pkg = ContextPackage {
            chapter: 1,
            selected_context: vec![],
        };
        let stack = test_rule_stack();
        let block = build_reduced_control_block("g", &pkg, &stack, WritingLanguage::Zh);
        assert!(block.contains("### 已选上下文\n- none\n"));
    }

    // --- audit_chapter 编排 ----------------------------------------------------------

    use crate::state::store::InMemoryStateStore;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    /// mock 聊天端口：返回固定响应并记录调用。
    struct MockChat {
        response: String,
        usage: Option<AuditTokenUsage>,
        calls: Mutex<Vec<(Vec<LLMMessage>, f64)>>,
        search_used: AtomicBool,
    }

    impl MockChat {
        fn new(response: &str) -> Self {
            MockChat {
                response: response.to_string(),
                usage: Some(AuditTokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 20,
                    total_tokens: 120,
                }),
                calls: Mutex::new(Vec::new()),
                search_used: AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl AuditorChat for MockChat {
        async fn chat(
            &self,
            messages: Vec<LLMMessage>,
            temperature: f64,
        ) -> Result<ChatOutcome, String> {
            self.calls.lock().unwrap().push((messages, temperature));
            Ok(ChatOutcome {
                content: self.response.clone(),
                usage: self.usage,
            })
        }

        async fn chat_with_search(
            &self,
            messages: Vec<LLMMessage>,
            temperature: f64,
        ) -> Result<ChatOutcome, String> {
            self.search_used.store(true, Ordering::SeqCst);
            self.chat(messages, temperature).await
        }
    }

    /// 建 audit fixture：临时 project（genres）/ builtin genres / book 目录。
    async fn audit_fixture(
        genre_md: &str,
        book_files: &[(&str, &str)],
    ) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let dir = tempfile::tempdir().expect("临时目录");
        let project = dir.path().join("project");
        let builtin = dir.path().join("builtin");
        let book = dir.path().join("book");
        tokio::fs::create_dir_all(project.join("genres"))
            .await
            .expect("建 project genres");
        tokio::fs::create_dir_all(&builtin).await.expect("建 builtin");
        tokio::fs::write(builtin.join("xianxia.md"), genre_md)
            .await
            .expect("写内置 genre");
        for (rel, content) in book_files {
            let path = book.join(rel);
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.expect("建父目录");
            }
            tokio::fs::write(&path, content).await.expect("写书文件");
        }
        (dir, project, builtin, book)
    }

    const AUDIT_GENRE_ZH: &str = "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6, 10]\n---\n正文指导\n";

    #[tokio::test]
    async fn audit_chapter_full_pipeline_zh() {
        let (_tmp, project, builtin, book) = audit_fixture(
            AUDIT_GENRE_ZH,
            &[
                ("book.json", r#"{"language":"zh"}"#),
                (
                    "story/book_rules.md",
                    "---\nversion: \"1.0\"\nprotagonist:\n  name: 林动\n  personalityLock: [冷静]\n  behavioralConstraints: [不滥杀]\n---\n规则正文",
                ),
                ("story/current_state.md", "# 当前状态\n林动：凝魂境三层"),
                ("story/pending_hooks.md", "| hook_id | 状态 |\n| --- | --- |\n| H1 | open |\n"),
                ("story/style_guide.md", "简练有力"),
                ("chapters/0002 第二章.md", "上一章的内容"),
            ],
        )
        .await;

        let mock = MockChat::new(
            r#"{"passed": true, "overall_score": 90, "issues": [{"severity":"warning","repair_scope":"local","category":"节奏检查","description":"中段拖沓","suggestion":"压缩"}], "summary": "合格"}"#,
        );
        let store = InMemoryStateStore::new();
        let ctx = AuditChapterCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &store,
        };
        let options = AuditChapterOptions::default();

        let result = audit_chapter(
            &ctx,
            &mock,
            &book,
            "第三章正文",
            3,
            Some("xianxia"),
            &options,
        )
        .await
        .expect("审查应成功");

        assert!(result.passed);
        assert_eq!(result.overall_score, Some(90));
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.parse_failed, None);
        assert_eq!(
            result.token_usage,
            Some(AuditTokenUsage {
                prompt_tokens: 100,
                completion_tokens: 20,
                total_tokens: 120
            })
        );

        // prompt 断言：单次调用、默认温度。
        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, 0.3);
        let (system, user) = {
            let msgs = &calls[0].0;
            assert_eq!(msgs.len(), 2);
            assert_eq!(msgs[0].role, LLMRole::System);
            assert_eq!(msgs[1].role, LLMRole::User);
            (msgs[0].content.clone(), msgs[1].content.clone())
        };

        // system：genre 标签 + 主角锁定 + 维度列表（恒加 32/33）+ JSON 格式要求。
        assert!(system.starts_with("你是一位严格的仙侠网络小说结构审稿编辑。你只审完成度 + 结构，不审文笔。\n主角人设锁定：林动，冷静，行为约束：不滥杀"));
        assert!(system.contains("32. 读者期待管理"));
        assert!(system.contains("33. 章节备忘偏离"));
        assert!(system.contains("10. 词汇疲劳（高疲劳词：震惊。"));
        assert!(system.contains("输出格式必须为 JSON"));
        // builtin prompt pack 指导段。
        assert!(system.contains("## Prompt Pack Guidance (longform.auditor"));

        // user：状态卡 + 伏笔池（disk 过滤版）+ 文风指南回退 + 上一章全文 + 待审内容。
        assert!(user.starts_with("请审查第3章。\n\n## 当前状态卡\n# 当前状态\n林动：凝魂境三层\n\n"));
        assert!(user.contains("## 伏笔池"));
        assert!(user.contains("| H1 | open |"));
        assert!(!user.contains("## 资源账本")); // numericalSystem 关闭
        assert!(user.contains("## 文风指南\n简练有力"));
        assert!(user.contains("## 上一章全文（用于衔接检查）\n上一章的内容\n"));
        assert!(user.ends_with("## 待审章节内容\n第三章正文"));
        assert!(!user.contains("## 章节备忘")); // 无 memo
    }

    #[tokio::test]
    async fn audit_chapter_en_routes_search_and_style_fallback() {
        let genre_en = "---\nname: Xianxia\nid: xianxia\nlanguage: en\nchapterTypes: []\nfatigueWords: []\neraResearch: true\nauditDimensions: [1]\n---\nbody\n";
        let (_tmp, project, builtin, book) = audit_fixture(
            genre_en,
            &[
                ("book.json", r#"{"language":"en"}"#),
                (
                    "story/book_rules.md",
                    "---\nversion: \"1.0\"\n---\nLegacy rules body as style fallback",
                ),
                // current_state 缺失 + 种子占位派生不可用 → placeholder。
            ],
        )
        .await;

        let mock = MockChat::new(r#"{"passed": false, "issues": [], "summary": "x"}"#);
        let store = InMemoryStateStore::new();
        let ctx = AuditChapterCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &store,
        };

        let result = audit_chapter(
            &ctx,
            &mock,
            &book,
            "Chapter text",
            1,
            None, // genre 缺省 → other；但 builtin 有 xianxia，这里回退 other.md 缺失 → genre 用 en 版 other？
            &AuditChapterOptions::default(),
        )
        .await;

        // genre=None → "other"，内置 other.md 缺失 → Genre NotFound 错误。
        assert!(matches!(
            result,
            Err(AuditChapterError::Genre(ReadGenreProfileError::NotFound { .. }))
        ));
    }

    #[tokio::test]
    async fn audit_chapter_en_with_builtin_other_uses_search() {
        let genre_en = "---\nname: Xianxia\nid: xianxia\nlanguage: en\nchapterTypes: []\nfatigueWords: []\neraResearch: true\nauditDimensions: [1]\n---\nbody\n";
        let other_md = "---\nname: General\nid: other\nlanguage: en\nchapterTypes: []\nfatigueWords: []\neraResearch: true\n---\nbody\n";
        let dir = tempfile::tempdir().expect("临时目录");
        let project = dir.path().join("project");
        let builtin = dir.path().join("builtin");
        let book = dir.path().join("book");
        tokio::fs::create_dir_all(project.join("genres"))
            .await
            .expect("建 project genres");
        tokio::fs::create_dir_all(&builtin).await.expect("建 builtin");
        tokio::fs::create_dir_all(&book).await.expect("建 book");
        let _ = genre_en;
        tokio::fs::write(builtin.join("other.md"), other_md)
            .await
            .expect("写 other.md");
        tokio::fs::write(book.join("book.json"), r#"{"language":"en"}"#)
            .await
            .expect("写 book.json");

        let mock = MockChat::new(r#"{"passed": true, "issues": [], "summary": "ok"}"#);
        let store = InMemoryStateStore::new();
        let ctx = AuditChapterCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &store,
        };
        let result = audit_chapter(
            &ctx,
            &mock,
            &book,
            "text",
            2,
            None,
            &AuditChapterOptions::default(),
        )
        .await
        .expect("en 审查应成功");

        assert!(result.passed);
        // eraResearch → chat_with_search 路由。
        assert!(mock.search_used.load(Ordering::SeqCst));

        let calls = mock.calls.lock().unwrap();
        let system = &calls[0].0[0].content;
        let user = &calls[0].0[1].content;
        // en genre label：profileName "General" 无中文 → 用 profileName。
        assert!(system.starts_with("You are a strict General web-fiction structural editor."));
        assert!(system.contains("32. Reader Expectation Check"));
        // eraResearch 搜索说明。
        assert!(system.contains("You have web-search capability"));
        assert!(user.starts_with("Review chapter 2.\n\n## Current State Card\n"));
        // 无 chapters 目录 → 无上一章块。
        assert!(!user.contains("Previous Chapter Full Text"));
    }

    #[tokio::test]
    async fn audit_chapter_governed_context_overrides_hooks_block() {
        use crate::models::input_governance::ContextSource;
        let (_tmp, project, builtin, book) = audit_fixture(
            AUDIT_GENRE_ZH,
            &[
                ("book.json", r#"{"language":"zh"}"#),
                ("story/current_state.md", "# 当前状态\n真实内容"),
                ("story/pending_hooks.md", "| hook_id | 状态 |\n| --- | --- |\n| H1 | open |\n"),
            ],
        )
        .await;

        let mock = MockChat::new(r#"{"passed": true, "issues": [], "summary": ""}"#);
        let store = InMemoryStateStore::new();
        let ctx = AuditChapterCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &store,
        };
        let options = AuditChapterOptions {
            context_package: Some(ContextPackage {
                chapter: 5,
                selected_context: vec![ContextSource {
                    source: "story/pending_hooks.md#H1".to_string(),
                    reason: "主线伏笔待回收".to_string(),
                    excerpt: Some("H1 玉佩悬念未解".to_string()),
                }],
            }),
            ..AuditChapterOptions::default()
        };

        audit_chapter(&ctx, &mock, &book, "正文", 5, Some("xianxia"), &options)
            .await
            .expect("应成功");

        let calls = mock.calls.lock().unwrap();
        let user = &calls[0].0[1].content;
        // governed hooks 块替代 disk 伏笔池。
        assert!(user.contains("H1 玉佩悬念未解"));
        assert!(!user.contains("## 伏笔池\n| hook_id"));
    }

    #[tokio::test]
    async fn audit_chapter_options_blocks_and_overrides() {
        let (_tmp, project, builtin, book) = audit_fixture(
            AUDIT_GENRE_ZH,
            &[
                ("book.json", r#"{"language":"zh"}"#),
                ("story/current_state.md", "# 盘上状态（应被覆盖）"),
                (
                    "story/pending_hooks.md",
                    "| hook_id | 状态 |\n| --- | --- |\n| H1 | open |\n",
                ),
            ],
        )
        .await;

        let mock = MockChat::new(r#"{"passed": true, "issues": [], "summary": ""}"#);
        let store = InMemoryStateStore::new();
        let ctx = AuditChapterCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &store,
        };
        let options = AuditChapterOptions {
            temperature: Some(0.7),
            chapter_intent: Some("推进主线".to_string()),
            chapter_memo: Some(ChapterMemo {
                chapter: 7,
                goal: "突破凝魂境".to_string(),
                is_golden_opening: false,
                body: "## 当前任务\n拿下城主之位".to_string(),
                thread_refs: vec![],
            }),
            context_package: Some(ContextPackage::default()),
            rule_stack: Some(RuleStack::default()),
            truth_file_overrides: Some(TruthFileOverrides {
                current_state: Some("# 覆盖状态".to_string()),
                ledger: Some("账本覆盖".to_string()),
                hooks: Some("伏笔覆盖".to_string()),
            }),
        };

        audit_chapter(&ctx, &mock, &book, "正文", 7, Some("xianxia"), &options)
            .await
            .expect("应成功");

        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls[0].1, 0.7); // 自定义温度
        let user = &calls[0].0[1].content;
        // 真相文件覆盖生效。
        assert!(user.contains("# 覆盖状态"));
        assert!(!user.contains("盘上状态"));
        assert!(user.contains("伏笔覆盖"));
        // memo 块。
        assert!(user.contains("## 章节备忘（用于 memo 偏离检测）\ngoal：突破凝魂境\n\n## 当前任务\n拿下城主之位\n"));
        // 三件套齐全 → 精简控制块替代文风指南块。
        assert!(user.contains("## 本章控制输入（由 Planner/Composer 编译）\n推进主线"));
        assert!(!user.contains("## 文风指南"));
    }
}
