//! reviser 编排 —— 审稿问题 → 修稿（auto 分流 / legacy 模式）+ 状态回写文本。
//!
//! 移植自 `packages/core/src/agents/reviser.ts`（714 行）。writeNextChapter 的
//! audit→revise 环核心：
//! - **auto 模式**（默认）：按问题类型分流输出——`resolveAutoOutputMode` 依
//!   repairScope / 分类正则判 patch-only / rewrite-only / allow-full；
//!   patch-only 走 spot-fix 补丁（≥50% 应用率才接受），rewrite-only 拒绝补丁
//! - **legacy 模式**（manual CLI）：polish / rewrite / rework / anti-detect /
//!   spot-fix 五档修改幅度
//! - **governed 入参**（chapterIntent + contextPackage + ruleStack 三全）时切换
//!   治理装配：hook/矩阵工作集裁剪 + 记忆证据块 + 缩减控制块 + 表格合并回写
//!
//! ## 移植纪律
//! - 系统提示词（auto zh/en + legacy + MODE_DESCRIPTIONS）逐字移植
//! - 输出解析的 tag 正则含 lookahead（`(?==== [A-Z_]+ ===|$)`）→ 消费式终止符等价还原
//! - `revisedContent.length`（wordCount 的 JS 语义 = UTF-16 码元数）

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use regex::Regex;
use std::sync::OnceLock;

use crate::agents::continuity::{AuditIssue, AuditSeverity, ChatOutcome, RepairScope};
use crate::agents::rules_reader::{read_book_language, read_book_rules, read_genre_profile};
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::book_rules::BookRules;
use crate::models::genre_profile::GenreProfile;
use crate::models::input_governance::{
    ChapterIntent, ChapterMemo, ContextPackage, RuleStack,
};
use crate::models::length_governance::LengthSpec;
use crate::prompts::prompt_pack::{
    append_prompt_pack_guidance, LoadPromptPackPromptInput,
};
use crate::state::store::StateStore;
use crate::utils::context_filter::filter_summaries;
use crate::utils::governed_context::build_governed_memory_evidence_blocks;
use crate::utils::governed_working_set::{
    build_governed_character_matrix_working_set, build_governed_hook_working_set,
};
use crate::utils::language::{utf16_len, WritingLanguage};
use crate::utils::length_metrics::count_chapter_length;
use crate::utils::narrative_control::{
    build_narrative_intent_brief, render_memo_as_narrative_block,
    render_narrative_selected_context, sanitize_narrative_evidence_block,
};
use crate::utils::outline_paths::{
    read_character_context, read_current_state_with_fallback, read_story_frame, read_volume_map,
};
use crate::utils::spot_fix_patches::{apply_spot_fix_patches, parse_spot_fix_patches};

/// agent 名。对齐 TS `ReviserAgent.name`。
pub const REVISER_NAME: &str = "reviser";

/// readFileSafe 的统一 fallback（对齐 TS 硬编码 "(文件不存在)"）。
const MISSING_FILE: &str = "(文件不存在)";

/// 修稿模式。对齐 TS `ReviseMode`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviseMode {
    #[default]
    Auto,
    Polish,
    Rewrite,
    Rework,
    AntiDetect,
    SpotFix,
}

/// auto 模式输出分流。对齐 TS `AutoOutputMode`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoOutputMode {
    PatchOnly,
    RewriteOnly,
    AllowFull,
}

/// 修稿出参。对齐 TS `ReviseOutput`。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviseOutput {
    pub revised_content: String,
    pub word_count: u64,
    pub fixed_issues: Vec<String>,
    pub token_usage: Option<crate::agents::continuity::AuditTokenUsage>,
}

/// LLM 聊天端口（复用注入模式）。
#[async_trait]
pub trait ReviserChat: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String>;
}

/// reviser 环境依赖。
pub struct ReviserCtx<'a> {
    pub project_root: &'a Path,
    pub builtin_genres_dir: &'a Path,
    pub prompt_store: &'a dyn StateStore,
}

/// reviseChapter 可选治理入参。对齐 TS options 对象。
pub struct ReviseOptions<'a> {
    pub chapter_intent: Option<&'a str>,
    pub chapter_memo: Option<&'a ChapterMemo>,
    pub chapter_intent_data: Option<&'a ChapterIntent>,
    pub context_package: Option<&'a ContextPackage>,
    pub rule_stack: Option<&'a RuleStack>,
    pub length_spec: Option<&'a LengthSpec>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReviseChapterError {
    #[error("prompt pack guidance: {0}")]
    PromptPack(String),
    #[error("LLM chat failed: {0}")]
    Chat(String),
}

/// 审稿问题 → 分层清单（critical / warning→High / 其余→Medium）。
pub fn build_tiered_issue_list(issues: &[AuditIssue], is_english: bool) -> String {
    let mut critical: Vec<String> = Vec::new();
    let mut high: Vec<String> = Vec::new();
    let mut medium: Vec<String> = Vec::new();

    for issue in issues {
        let line = format!("- {}: {}", issue.category, issue.description);
        match issue.severity {
            AuditSeverity::Critical => critical.push(line),
            AuditSeverity::Warning => high.push(line),
            AuditSeverity::Info => medium.push(line),
        }
    }

    let mut parts: Vec<String> = Vec::new();
    if !critical.is_empty() {
        parts.push(if is_english {
            format!("## Critical — Must Fix\n{}", critical.join("\n"))
        } else {
            format!("## Critical（必须解决）\n{}", critical.join("\n"))
        });
    }
    if !high.is_empty() {
        parts.push(if is_english {
            format!("## High — Should Improve\n{}", high.join("\n"))
        } else {
            format!("## High（应当改善）\n{}", high.join("\n"))
        });
    }
    if !medium.is_empty() {
        parts.push(if is_english {
            format!("## Medium — Reference\n{}", medium.join("\n"))
        } else {
            format!("## Medium（参考建议）\n{}", medium.join("\n"))
        });
    }
    parts.join("\n\n")
}

/// legacy 模式描述。逐字移植 TS `MODE_DESCRIPTIONS`。
pub fn mode_description(mode: ReviseMode) -> &'static str {
    match mode {
        ReviseMode::Auto => "",
        ReviseMode::Polish => "润色：只改表达、节奏、段落呼吸，不改事实与剧情结论。禁止：增删段落、改变人名/地名/物品名、增加新情节或新对话、改变因果关系。只允许：替换用词、调整句序、修改标点节奏",
        ReviseMode::Rewrite => "改写：允许重组问题段落、调整画面和叙述力度，但优先保留原文的绝大部分句段。除非问题跨越整章，否则禁止整章推倒重写；只能围绕问题段落及其直接上下文改写，同时保留核心事实与人物动机",
        ReviseMode::Rework => "重写：可重构场景推进和冲突组织，但不改主设定和大事件结果",
        ReviseMode::AntiDetect => {
            "反检测改写：在保持剧情不变的前提下，降低AI生成可检测性。\n\n改写手法（附正例）：\n1. 打破句式规律：连续短句 → 长短交替，句式不可预测\n2. 口语化替代：✗\"然而事情并没有那么简单\" → ✓\"哪有那么便宜的事\"\n3. 减少\"了\"字密度：✗\"他走了过去，拿了杯子\" → ✓\"他走过去，端起杯子\"\n4. 转折词降频：✗\"虽然…但是…\" → ✓ 用角色内心吐槽或直接动作切换\n5. 情绪外化：✗\"他感到愤怒\" → ✓\"他捏碎了茶杯，滚烫的茶水流过指缝\"\n6. 删掉叙述者结论：✗\"这一刻他终于明白了力量\" → ✓ 只写行动，让读者自己感受\n7. 群像反应具体化：✗\"全场震惊\" → ✓\"老陈的烟掉在裤子上，烫得他跳起来\"\n8. 段落长度差异化：不再等长段落，有的段只有一句话，有的段七八行\n9. 消灭\"不禁\"\"仿佛\"\"宛如\"等AI标记词：换成具体感官描写"
        }
        ReviseMode::SpotFix => "定点修复：只修改审稿意见指出的具体句子或段落，其余所有内容必须原封不动保留。修改范围限定在问题句子及其前后各一句。禁止改动无关段落",
    }
}

/// auto 模式输出分流：repairScope 优先（structural → rewrite-only；全 local →
/// patch-only），否则按分类正则计数（任一 structural → rewrite-only；全 local →
/// patch-only；混合/未知 → allow-full）。
pub fn resolve_auto_output_mode(issues: &[AuditIssue]) -> AutoOutputMode {
    if issues.is_empty() {
        return AutoOutputMode::AllowFull;
    }
    let scoped_blocking: Vec<&AuditIssue> = issues
        .iter()
        .filter(|issue| issue.severity != AuditSeverity::Info && issue.repair_scope.is_some())
        .collect();
    if !scoped_blocking.is_empty() {
        if scoped_blocking
            .iter()
            .any(|issue| issue.repair_scope == Some(RepairScope::Structural))
        {
            return AutoOutputMode::RewriteOnly;
        }
        let blocking_count = issues
            .iter()
            .filter(|issue| issue.severity != AuditSeverity::Info)
            .count();
        if scoped_blocking.len() == blocking_count
            && scoped_blocking
                .iter()
                .all(|issue| issue.repair_scope == Some(RepairScope::Local))
        {
            return AutoOutputMode::PatchOnly;
        }
    }

    let is_structural = |issue: &AuditIssue| {
        let text = format!("{} {}", issue.category, issue.description);
        structural_patterns().iter().any(|p| p.is_match(&text))
    };
    let is_local = |issue: &AuditIssue| {
        let text = format!("{} {}", issue.category, issue.description);
        local_only_patterns().iter().any(|p| p.is_match(&text))
    };

    // 阻塞问题（critical + warning）计数；info 级是 Polisher 的提示，不驱动分流。
    let blocking: Vec<&AuditIssue> = issues
        .iter()
        .filter(|issue| issue.severity != AuditSeverity::Info)
        .collect();
    if blocking.is_empty() {
        return AutoOutputMode::PatchOnly;
    }

    let structural_count = blocking.iter().filter(|issue| is_structural(issue)).count();
    if structural_count > 0 {
        return AutoOutputMode::RewriteOnly;
    }

    let local_only_count = blocking.iter().filter(|issue| is_local(issue)).count();
    if local_only_count == blocking.len() {
        return AutoOutputMode::PatchOnly;
    }

    AutoOutputMode::AllowFull
}

fn local_only_patterns() -> &'static [Regex] {
    static R: OnceLock<Vec<Regex>> = OnceLock::new();
    R.get_or_init(|| {
        [
            r"(?i)Paragraph uniformity|段落等长",
            r"(?i)Hedge density|套话密度",
            r"(?i)Formulaic transitions|公式化转折",
            r"(?i)List-like structure|列表式结构",
            r"(?i)Cross-chapter repetition|跨章重复",
            r"(?i)AI-tell word density",
            r"(?i)Fatigue word|高疲劳词",
            r"(?i)Information Boundary Check|信息越界",
            r"(?i)Knowledge Base Pollution|知识库污染",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("local pattern 应合法"))
        .collect()
    })
}

fn structural_patterns() -> &'static [Regex] {
    static R: OnceLock<Vec<Regex>> = OnceLock::new();
    R.get_or_init(|| {
        [
            r"(?i)OOC|人设|Character Fidelity|Character Matrix|Character.*Consistency",
            r"(?i)Mainline.*Drift|主线偏离|Outline Drift|大纲偏离|Chapter Memo Drift|章节备忘偏离",
            r"(?i)Conflict|冲突乏力|Payoff Dilution|爽点虚化",
            r"(?i)Timeline|时间线",
            r"(?i)Hook Check|伏笔检查|Hook.*Debt|伏笔.*债|未兑现",
            r"(?i)Power Scaling|战力崩坏|金手指",
            r"(?i)Pacing|节奏",
            r"(?i)POV Consistency|视角",
            r"(?i)Subplot Stagnation|支线停滞|Arc Flatline|弧线平坦",
            r"(?i)Relationship Dynamics|关系动态|情感表达",
            r"(?i)Incentive Chain|利益链",
            r"(?i)Canon Event|正典|Mainline Canon",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("structural pattern 应合法"))
        .collect()
    })
}

/// 输出 tag 段提取：`=== TAG ===` 到下一个 `=== X ===` 或串尾。
/// TS lookahead `(?==== [A-Z_]+ ===|$)` → 消费式终止符等价还原。
fn extract_tag(content: &str, tag: &str) -> String {
    let pattern = format!(r"(?s)=== {} ===\s*(.*?)(?:=== [A-Z_]+ ===|\z)", tag);
    let Ok(regex) = Regex::new(&pattern) else {
        return String::new();
    };
    regex
        .captures(content)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().trim().to_string()))
        .unwrap_or_default()
}

/// 解析 LLM 修稿输出。golden 守门。
pub fn parse_reviser_output(
    content: &str,
    _numerical_system: bool,
    mode: ReviseMode,
    original_chapter: &str,
    auto_output_mode: AutoOutputMode,
) -> ReviseOutput {
    let fixed_raw = extract_tag(content, "FIXED_ISSUES");
    let fixed_issues: Vec<String> = fixed_raw
        .split('\n')
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    let make_result = |revised_content: String, applied: bool| ReviseOutput {
        // TS revisedContent.length：UTF-16 码元数。
        word_count: utf16_len(&revised_content) as u64,
        fixed_issues: if applied { fixed_issues.clone() } else { Vec::new() },
        revised_content,
        token_usage: None,
    };

    // auto：按问题类型分流——结构问题必须 REVISED_CONTENT；纯局部问题只收 PATCHES；
    // 混合集两者皆可。
    if mode == ReviseMode::Auto {
        if auto_output_mode == AutoOutputMode::PatchOnly {
            let patches_raw = extract_tag(content, "PATCHES");
            if !patches_raw.is_empty() {
                let patches = parse_spot_fix_patches(&patches_raw);
                if !patches.is_empty() {
                    let patch_result = apply_spot_fix_patches(original_chapter, &patches);
                    if patch_result.applied
                        && (patch_result.applied_patch_count as usize) * 2 >= patches.len()
                    {
                        return make_result(patch_result.revised_content, true);
                    }
                }
            }
            return make_result(original_chapter.to_string(), false);
        }

        if auto_output_mode == AutoOutputMode::RewriteOnly {
            let revised = extract_tag(content, "REVISED_CONTENT");
            if !revised.is_empty() {
                return make_result(revised, true);
            }
            // 未产出重写——不回退补丁；结构问题无法安全补丁。原样返回。
            return make_result(original_chapter.to_string(), false);
        }

        let revised = extract_tag(content, "REVISED_CONTENT");
        if !revised.is_empty() {
            return make_result(revised, true);
        }
        let patches_raw = extract_tag(content, "PATCHES");
        if !patches_raw.is_empty() {
            let patches = parse_spot_fix_patches(&patches_raw);
            if !patches.is_empty() {
                let patch_result = apply_spot_fix_patches(original_chapter, &patches);
                if patch_result.applied
                    && (patch_result.applied_patch_count as usize) * 2 >= patches.len()
                {
                    return make_result(patch_result.revised_content, true);
                }
            }
        }
        return make_result(original_chapter.to_string(), false);
    }

    // legacy spot-fix：仅补丁。
    if mode == ReviseMode::SpotFix {
        let patches = parse_spot_fix_patches(&extract_tag(content, "PATCHES"));
        let patch_result = apply_spot_fix_patches(original_chapter, &patches);
        return make_result(patch_result.revised_content, patch_result.applied);
    }

    // legacy rewrite/polish/rework/anti-detect：完整正文。
    let revised = extract_tag(content, "REVISED_CONTENT");
    let applied = !revised.is_empty();
    make_result(
        if applied { revised } else { original_chapter.to_string() },
        applied,
    )
}

/// 修稿主入口。
#[allow(clippy::too_many_arguments)]
pub async fn revise_chapter(
    chat: &dyn ReviserChat,
    ctx: &ReviserCtx<'_>,
    book_dir: &Path,
    chapter_content: &str,
    chapter_number: u32,
    issues: &[AuditIssue],
    mode: ReviseMode,
    genre: Option<&str>,
    options: &ReviseOptions<'_>,
) -> Result<ReviseOutput, ReviseChapterError> {
    let story = book_dir.join("story");
    let particle_ledger_path = story.join("particle_ledger.md");
    let pending_hooks_path = story.join("pending_hooks.md");
    let style_guide_path2 = story.join("style_guide.md");
    let chapter_summaries_path = story.join("chapter_summaries.md");
    let parent_canon_path = story.join("parent_canon.md");
    let fanfic_canon_path = story.join("fanfic_canon.md");
    let (current_state, ledger, hooks, style_guide_raw, volume_outline, story_bible, character_matrix, chapter_summaries, parent_canon, fanfic_canon) = tokio::join!(
        // Phase 5 整合：current_state.md 仍是架构师占位时从 roles + 种子 hook 推导。
        read_current_state_with_fallback(book_dir, MISSING_FILE),
        read_file_safe(&particle_ledger_path),
        read_file_safe(&pending_hooks_path),
        read_file_safe(&style_guide_path2),
        read_volume_map(book_dir, MISSING_FILE),
        read_story_frame(book_dir, MISSING_FILE),
        read_character_context(book_dir, MISSING_FILE),
        read_file_safe(&chapter_summaries_path),
        read_file_safe(&parent_canon_path),
        read_file_safe(&fanfic_canon_path),
    );

    let genre_id = genre.unwrap_or("other");
    let (parsed_genre, book_language) = tokio::join!(
        read_genre_profile(ctx.project_root, genre_id, ctx.builtin_genres_dir),
        read_book_language(book_dir),
    );
    let parsed_genre = parsed_genre.map_err(|e| ReviseChapterError::Chat(e.to_string()))?;
    let gp = parsed_genre.profile;
    let parsed_rules = read_book_rules(book_dir).await;
    let book_rules: Option<&BookRules> = parsed_rules.as_ref().map(|parsed| &parsed.rules);

    // style_guide.md 缺失时回退 book_rules 正文（Phase 5 hotfix 2：story_frame
    // frontmatter 来源的 body 为空——空串不是可用文风指南，视为无回退）。
    let legacy_rules_body = parsed_rules
        .as_ref()
        .map(|parsed| parsed.body.trim().to_string())
        .unwrap_or_default();
    let style_guide = if style_guide_raw != MISSING_FILE {
        style_guide_raw
    } else if !legacy_rules_body.is_empty() {
        legacy_rules_body
    } else {
        "(无文风指南)".to_string()
    };

    let is_english = book_language.as_deref().unwrap_or(gp.language.as_str()) == "en";
    let resolved_language = if is_english { WritingLanguage::En } else { WritingLanguage::Zh };

    let issue_list = if mode == ReviseMode::Auto {
        build_tiered_issue_list(issues, is_english)
    } else {
        issues
            .iter()
            .map(|issue| {
                format!(
                    "- [{}] {}: {}\n  {}: {}",
                    severity_label(issue.severity),
                    issue.category,
                    issue.description,
                    if is_english { "Suggestion" } else { "建议" },
                    issue.suggestion
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let numerical_rule = if gp.numerical_system {
        if is_english {
            "\n3. Numerical errors must be fixed precisely — cross-check before and after"
        } else {
            "\n3. 数值错误必须精确修正，前后对账"
        }
    } else {
        ""
    };
    let protagonist_block = book_rules
        .and_then(|rules| rules.protagonist.as_ref())
        .map(|protagonist| {
            if is_english {
                format!(
                    "\n\nProtagonist lock: {} — {}. Revisions must not violate the protagonist profile.",
                    protagonist.name,
                    protagonist.personality_lock.join(", ")
                )
            } else {
                format!(
                    "\n\n主角人设锁定：{}，{}。修改不得违反人设。",
                    protagonist.name,
                    protagonist.personality_lock.join("、")
                )
            }
        })
        .unwrap_or_default();
    // 字数护栏仅 legacy 模式使用（manual CLI revise）；auto 模式交给 normalize。
    let length_guardrail = if mode != ReviseMode::Auto && options.length_spec.is_some() {
        if is_english {
            "\n8. Keep the chapter word count within the target range; only allow minor deviation when fixing critical issues truly requires it"
        } else {
            "\n8. 保持章节字数在目标区间内；只有在修复关键问题确实需要时才允许轻微偏离"
        }
    } else {
        ""
    };
    let lang_prefix = if is_english {
        "【LANGUAGE OVERRIDE】ALL output (FIXED_ISSUES, PATCHES, REVISED_CONTENT) MUST be in English.\n\n"
    } else {
        ""
    };
    let governed_mode = options.chapter_intent.is_some()
        && options.context_package.is_some()
        && options.rule_stack.is_some();

    let governed_package = if governed_mode { options.context_package } else { None };
    let hooks_working_set = if let Some(package) = governed_package {
        build_governed_hook_working_set(&crate::utils::governed_working_set::GovernedHookWorkingSetInput {
            hooks_markdown: &hooks,
            context_package: package,
            chapter_intent: options.chapter_intent,
            chapter_number,
            language: resolved_language,
            keep_recent: None,
        })
    } else {
        hooks.clone()
    };
    let chapter_summaries_working_set = if governed_mode {
        filter_summaries(&chapter_summaries, chapter_number, None)
    } else {
        chapter_summaries.clone()
    };
    let character_matrix_working_set = if let Some(package) = governed_package {
        build_governed_character_matrix_working_set(
            &crate::utils::governed_working_set::GovernedMatrixWorkingSetInput {
                matrix_markdown: &character_matrix,
                chapter_intent: options.chapter_intent.unwrap_or(&volume_outline),
                context_package: package,
                protagonist_name: book_rules
                    .and_then(|rules| rules.protagonist.as_ref())
                    .map(|protagonist| protagonist.name.as_str()),
            },
        )
    } else {
        character_matrix.clone()
    };

    let auto_output_mode = if mode == ReviseMode::Auto {
        resolve_auto_output_mode(issues)
    } else {
        AutoOutputMode::AllowFull
    };
    let system_prompt_base = if mode == ReviseMode::Auto {
        build_auto_system_prompt(
            lang_prefix,
            &gp,
            &protagonist_block,
            numerical_rule,
            resolved_language,
            options.length_spec,
            auto_output_mode,
        )
    } else {
        build_legacy_system_prompt(
            lang_prefix,
            &gp,
            &protagonist_block,
            numerical_rule,
            length_guardrail,
            mode,
        )
    };
    let system_prompt = append_prompt_pack_guidance(
        ctx.prompt_store,
        &system_prompt_base,
        &LoadPromptPackPromptInput {
            prompt_id: "longform.reviser".to_string(),
            project_root: Some(ctx.project_root.to_string_lossy().into_owned()),
            user_root: None,
        },
    )
    .await
    .map_err(|e| ReviseChapterError::PromptPack(e.to_string()))?;

    let ledger_block = if gp.numerical_system {
        format!("\n## 资源账本\n{ledger}")
    } else {
        String::new()
    };
    let governed_memory_blocks = options
        .context_package
        .map(|package| build_governed_memory_evidence_blocks(package, Some(resolved_language)));
    let hook_debt_block = governed_memory_blocks
        .as_ref()
        .and_then(|blocks| blocks.hook_debt_block.clone())
        .unwrap_or_default();
    let hooks_block = governed_memory_blocks
        .as_ref()
        .and_then(|blocks| blocks.hooks_block.clone())
        .unwrap_or_else(|| format!("\n## 伏笔池\n{hooks_working_set}\n"));
    let outline_block = if volume_outline != MISSING_FILE {
        format!("\n## 卷纲\n{volume_outline}\n")
    } else {
        String::new()
    };
    let bible_block = if !governed_mode && story_bible != MISSING_FILE {
        format!("\n## 世界观设定\n{story_bible}\n")
    } else {
        String::new()
    };
    let matrix_block = if character_matrix_working_set != MISSING_FILE {
        format!("\n## 角色交互矩阵\n{character_matrix_working_set}\n")
    } else {
        String::new()
    };
    let summaries_block = governed_memory_blocks
        .as_ref()
        .and_then(|blocks| blocks.summaries_block.clone())
        .unwrap_or_else(|| {
            if chapter_summaries_working_set != MISSING_FILE {
                format!("\n## 章节摘要\n{chapter_summaries_working_set}\n")
            } else {
                String::new()
            }
        });
    let volume_summaries_block = governed_memory_blocks
        .as_ref()
        .and_then(|blocks| blocks.volume_summaries_block.clone())
        .unwrap_or_default();

    let has_parent_canon = parent_canon != MISSING_FILE;
    let has_fanfic_canon = fanfic_canon != MISSING_FILE;
    let canon_block = if has_parent_canon {
        format!("\n## 正传正典参照（修稿专用）\n本书为番外作品。修改时参照正典约束，不可改变正典事实。\n{parent_canon}\n")
    } else {
        String::new()
    };
    let fanfic_canon_block = if has_fanfic_canon {
        format!("\n## 同人正典参照（修稿专用）\n本书为同人作品。修改时参照正典角色档案和世界规则，不可违反正典事实。角色对话必须保留原作语癖。\n{fanfic_canon}\n")
    } else {
        String::new()
    };
    let reduced_control_block = match (governed_package, options.rule_stack) {
        (Some(package), Some(rule_stack)) => build_reduced_control_block(
            options.chapter_memo,
            options.chapter_intent_data,
            options.chapter_intent,
            package,
            rule_stack,
        ),
        _ => String::new(),
    };
    // 字数护栏仅 legacy——auto 模式交给 normalize。
    let length_guidance_block = if mode != ReviseMode::Auto {
        if let Some(spec) = options.length_spec {
        format!(
            "\n## 字数护栏\n目标字数：{}\n允许区间：{}-{}\n极限区间：{}-{}\n如果修正后超出允许区间，请优先压缩冗余解释、重复动作和弱信息句，不得新增支线或删掉核心事实。\n",
            spec.target, spec.soft_min, spec.soft_max, spec.hard_min, spec.hard_max
        )
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let style_guide_block = if reduced_control_block.is_empty() {
        format!("\n## 文风指南\n{style_guide}")
    } else {
        String::new()
    };

    let user_prompt = format!(
        "请修正第{chapter_number}章。\n\n## 审稿问题\n{issue_list}\n\n## 当前状态卡\n{current_state}{ledger_block}{}{}{}{}{bible_block}{matrix_block}{}{canon_block}{fanfic_canon_block}{style_guide_block}{length_guidance_block}\n\n## 待修正章节\n{chapter_content}",
        sanitize_or_empty(&hook_debt_block, resolved_language),
        sanitize_or_empty(&hooks_block, resolved_language),
        sanitize_or_empty(&volume_summaries_block, resolved_language),
        if reduced_control_block.is_empty() { &outline_block } else { &reduced_control_block },
        sanitize_or_empty(&summaries_block, resolved_language),
    );

    let response = chat
        .chat(
            vec![
                LLMMessage { role: LLMRole::System, content: system_prompt, tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user_prompt, tool_calls: None, tool_call_id: None },
            ],
            0.3,
        )
        .await
        .map_err(ReviseChapterError::Chat)?;

    let output = parse_reviser_output(
        &response.content,
        gp.numerical_system,
        mode,
        chapter_content,
        auto_output_mode,
    );
    let merged_output = output;
    let word_count = match options.length_spec {
        Some(spec) => {
            u64::from(count_chapter_length(&merged_output.revised_content, spec.counting_mode))
        }
        None => merged_output.word_count,
    };
    Ok(ReviseOutput {
        word_count,
        token_usage: response.usage,
        ..merged_output
    })
}

fn sanitize_or_empty(block: &str, language: WritingLanguage) -> String {
    sanitize_narrative_evidence_block(Some(block), language).unwrap_or_default()
}

fn severity_label(severity: AuditSeverity) -> &'static str {
    match severity {
        AuditSeverity::Critical => "critical",
        AuditSeverity::Warning => "warning",
        AuditSeverity::Info => "info",
    }
}

/// 缩减控制块（governed 模式的「本章控制输入」）。
pub fn build_reduced_control_block(
    memo: Option<&ChapterMemo>,
    intent: Option<&ChapterIntent>,
    chapter_intent: Option<&str>,
    context_package: &ContextPackage,
    rule_stack: &RuleStack,
) -> String {
    let selected_refs: Vec<crate::utils::narrative_control::ContextSourceRef> = context_package
        .selected_context
        .iter()
        .map(|entry| crate::utils::narrative_control::ContextSourceRef {
            reason: entry.reason.as_str(),
            excerpt: entry.excerpt.as_deref(),
        })
        .collect();
    let selected_context = heading_prefix_re()
        .replace_all(
            &render_narrative_selected_context(&selected_refs, WritingLanguage::Zh),
            "- ",
        )
        .into_owned();
    let overrides = if !rule_stack.active_overrides.is_empty() {
        rule_stack
            .active_overrides
            .iter()
            .map(|override_| {
                format!(
                    "- {} -> {}: {} ({})",
                    override_.from, override_.to, override_.reason, override_.target
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        "- none".to_string()
    };
    // memo 叙事块优先；回退 legacy intent markdown。
    let narrative_block = if let Some(memo) = memo {
        render_memo_as_narrative_block(memo, intent.and_then(|i| i.arc_context.as_deref()), WritingLanguage::Zh)
    } else if let Some(chapter_intent) = chapter_intent {
        build_narrative_intent_brief(chapter_intent, WritingLanguage::Zh)
    } else {
        "(无)".to_string()
    };

    let join_or_none = |items: &[String]| -> String {
        if items.is_empty() {
            "(无)".to_string()
        } else {
            items.join("、")
        }
    };

    format!(
        "\n## 本章控制输入（由 Planner/Composer 编译）\n{narrative_block}\n\n### 已选上下文\n{selected}\n\n### 规则栈\n- 硬护栏：{}\n- 软约束：{}\n- 诊断规则：{}\n\n### 当前覆盖\n{overrides}\n",
        join_or_none(&rule_stack.sections.hard),
        join_or_none(&rule_stack.sections.soft),
        join_or_none(&rule_stack.sections.diagnostic),
        selected = if selected_context.is_empty() { "- none".to_string() } else { selected_context },
    )
}

/// auto 模式系统提示词（zh/en）。逐字移植 TS `buildAutoSystemPrompt`。
pub fn build_auto_system_prompt(
    lang_prefix: &str,
    gp: &GenreProfile,
    protagonist_block: &str,
    numerical_rule: &str,
    resolved_language: WritingLanguage,
    length_spec: Option<&LengthSpec>,
    auto_output_mode: AutoOutputMode,
) -> String {
    let en = resolved_language == WritingLanguage::En;
    let rewrite_length_constraint = length_spec
        .map(|spec| {
            if en {
                format!(
                    "\n  HARD CONSTRAINT: The revised chapter must stay within {}-{} characters (target: {}, ±25%). This is non-negotiable — do not exceed this range.",
                    spec.soft_min, spec.soft_max, spec.target
                )
            } else {
                format!(
                    "\n  硬性约束：重写后的章节必须控制在 {}-{} 字以内（目标 {} 字，±25%）。这是不可突破的底线。",
                    spec.soft_min, spec.soft_max, spec.target
                )
            }
        })
        .unwrap_or_default();

    let routing_directive = if en {
        match auto_output_mode {
            AutoOutputMode::RewriteOnly => "\n\nROUTING: The reviewer's blocking issues are structural / semantic (character collapse, mainline drift, missing payoff, timeline break, unpaid hook, memo drift, etc.). You MUST output REVISED_CONTENT — do not emit PATCHES, they cannot fix this class of problem. If you cannot safely rewrite, say so in FIXED_ISSUES and leave REVISED_CONTENT empty.",
            AutoOutputMode::PatchOnly => "\n\nROUTING: The reviewer's blocking issues are local (wording, paragraph shape, fatigue word, information boundary, knowledge pollution). You MUST output PATCHES only — do not rewrite the whole chapter. If patches are not possible, leave PATCHES empty.",
            AutoOutputMode::AllowFull => "",
        }
    } else {
        match auto_output_mode {
            AutoOutputMode::RewriteOnly => "\n\n分流指令：reviewer 报告的阻塞问题属于结构/语义错（人设崩、主线偏、爽点缺、时间线错、伏笔未收、memo 偏离等）。你必须输出 REVISED_CONTENT——禁止输出 PATCHES，这类问题不能靠补丁修复。如果无法安全重写，在 FIXED_ISSUES 里说明并留空 REVISED_CONTENT。",
            AutoOutputMode::PatchOnly => "\n\n分流指令：reviewer 报告的阻塞问题属于局部错（措辞、段落形状、疲劳词、信息越界、知识污染）。你必须只输出 PATCHES——不要整章改写。如果做不出补丁，留空 PATCHES。",
            AutoOutputMode::AllowFull => "",
        }
    };

    if en {
        format!(
            r#"{lang_prefix}You are a professional {name} web-fiction revision editor. Fix the chapter according to the review notes.{protagonist_block}{routing_directive}

PATCHES and REVISED_CONTENT serve different problems — choose by problem type, not preference:

PATCHES — for local text issues (wording, dialogue, AI-tell phrases, small continuity errors).
  Each PATCH quotes the passage to change (a sentence, a paragraph, or multiple paragraphs) and provides a replacement. Untouched text stays exactly as-is.

REVISED_CONTENT — for whole-chapter issues (length compression, structural rewrite, pacing restructure, major plot realignment).
  Outputs the full revised chapter. When Critical issues include length or structural problems, you must use REVISED_CONTENT — patches cannot compress or restructure a chapter.{rewrite_length_constraint}

If Critical issues include both local and whole-chapter problems, use REVISED_CONTENT (it addresses everything in one pass).

Revision principles:
1. Fix root causes — do not apply superficial polish{numerical_rule}
2. Hook status must stay in sync with the hooks board. If hook debt briefs are provided, preserve hook payoff scenes
3. Do not alter the plot direction or core conflicts
4. Preserve the original language style, rhythm, and pacing — do not compress transitional scenes or remove breathing room
5. Emotion through action (never "he felt angry" — show it). Values through behavior, not slogans
6. Different characters speak differently. No "everyone gasped in unison"
7. Escalate: bad things stack, each worse than the last

Cycle-aware revision:
- If this chapter should be "aftermath" but is still escalating tension, rewrite the densest conflict passage into a change-showing passage — who lost what, whose attitude shifted, what the new normal is
- If this chapter should be "climax" but has no clear payoff, find the closest scene to a reward and amplify it — make the promised release exceed reader expectations
- Daily passages that don't serve the main line: rewrite as "bait" — add a detail pointing to the future, a hint, a character reaction that seeds curiosity

Output format:

=== FIXED_ISSUES ===
(List each fix on its own line; if a safe local fix is not possible, explain here)

=== PATCHES ===
(Output local patches if applicable. Omit this section entirely if using REVISED_CONTENT)
--- PATCH 1 ---
TARGET_TEXT:
(Exact quote from the original that identifies the passage to change)
REPLACEMENT_TEXT:
(Replacement text for this passage)
--- END PATCH ---

=== REVISED_CONTENT ===
(Full revised chapter content — only when PATCHES cannot solve the problem. Omit this section if using PATCHES)"#,
            name = gp.name,
        )
    } else {
        format!(
            r#"{lang_prefix}你是一位专业的{name}网络小说修稿编辑。你的任务是根据审稿意见对章节进行修正。{protagonist_block}{routing_directive}

PATCHES 和 REVISED_CONTENT 分别处理不同类型的问题——按问题类型选择，不是按偏好：

PATCHES——处理局部文字问题（措辞、对话、AI痕迹、小的连续性错误）。
  每个 PATCH 引用要修改的原文段落（一句、一段或多段皆可），给出替换文本。未涉及的内容保持原样。

REVISED_CONTENT——处理全章级问题（字数压缩、结构重组、节奏重排、重大剧情偏离）。
  输出修正后的完整正文。当 Critical 问题包含字数或结构性问题时，必须使用 REVISED_CONTENT——PATCHES 无法压缩或重构整章。{rewrite_length_constraint}

如果 Critical 同时包含局部问题和全章问题，使用 REVISED_CONTENT（一次性解决所有问题）。

修稿原则：
1. 修根因，不做表面润色{numerical_rule}
2. 伏笔状态必须与伏笔池同步。如果提供了 Hook Debt 简报，必须保留伏笔兑现段落
3. 不改变剧情走向和核心冲突
4. 保持原文的语言风格、节奏和呼吸——不要压缩过渡段、不要删掉减速段
5. 情绪用动作外化（不写"他感到愤怒"，写动作）。价值观通过行为传达
6. 不同角色说话方式必须不同。禁止"众人齐声惊呼"
7. 坏事叠坏事，每层比上一层过分

小目标周期修稿指引：
- 如果本章应该是"后效"阶段但仍在加压，把最密集的冲突段落改写为展示改变的段落——谁失去了什么、谁的态度变了、新的常态是什么
- 如果本章应该是"爆发"阶段但没有明确兑现，找到最接近回报的场景并放大它——让承诺的释放超过读者预期
- 日常段落如果不服务主线，改写为"饵"：加入一个指向未来的细节、一句暗示、一个角色反应

输出格式：

=== FIXED_ISSUES ===
(逐条说明修正了什么)

=== PATCHES ===
(局部补丁——仅用于局部文字问题。有全章级问题时省略此区块)
--- PATCH 1 ---
TARGET_TEXT:
(从原文中精确引用要修改的段落)
REPLACEMENT_TEXT:
(替换后的文本)
--- END PATCH ---

=== REVISED_CONTENT ===
(修正后的完整正文——用于字数/结构/节奏等全章级问题。仅局部问题时省略此区块)"#,
            name = gp.name,
        )
    }
}

/// legacy 模式系统提示词。逐字移植 TS `buildLegacySystemPrompt`。
pub fn build_legacy_system_prompt(
    lang_prefix: &str,
    gp: &GenreProfile,
    protagonist_block: &str,
    numerical_rule: &str,
    length_guardrail: &str,
    mode: ReviseMode,
) -> String {
    let mode_desc = mode_description(mode);
    let output_format = if mode == ReviseMode::SpotFix {
        "=== FIXED_ISSUES ===\n(逐条说明修正了什么，一行一条；如果无法安全定点修复，也在这里说明)\n\n=== PATCHES ===\n--- PATCH 1 ---\nTARGET_TEXT:\n(必须从原文中精确复制、且能唯一命中的原句或原段)\nREPLACEMENT_TEXT:\n(替换后的局部文本)\n--- END PATCH ---".to_string()
    } else {
        "=== FIXED_ISSUES ===\n(逐条说明修正了什么，一行一条)\n\n=== REVISED_CONTENT ===\n(修正后的完整正文)".to_string()
    };
    let spot_fix_extra = if mode == ReviseMode::SpotFix {
        "\n9. spot-fix 只能输出局部补丁，禁止输出整章改写；TARGET_TEXT 必须能在原文中唯一命中\n10. 如果需要大面积改写，说明无法安全 spot-fix，并让 PATCHES 留空"
    } else {
        ""
    };
    format!(
        "{lang_prefix}你是一位专业的{name}网络小说修稿编辑。你的任务是根据审稿意见对章节进行修正。{protagonist_block}\n\n修稿模式：{mode_desc}\n\n修稿原则：\n1. 按模式控制修改幅度\n2. 修根因，不做表面润色{numerical_rule}\n4. 正文必须服从既有事实和伏笔约束，但不要输出或重写状态文件；宿主会根据修订正文重新结算\n5. 不改变剧情走向和核心冲突\n6. 保持原文的语言风格和节奏\n{length_guardrail}\n{spot_fix_extra}\n\n输出格式：\n\n{output_format}",
        name = gp.name,
    )
}

fn heading_prefix_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^### ").unwrap())
}

async fn read_file_safe(path: &Path) -> String {
    tokio::fs::read_to_string(path)
        .await
        .unwrap_or_else(|_| MISSING_FILE.to_string())
}

#[allow(dead_code)]
fn _unused(_p: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(severity: AuditSeverity, category: &str, description: &str, scope: Option<RepairScope>) -> AuditIssue {
        AuditIssue {
            severity,
            category: category.to_string(),
            description: description.to_string(),
            suggestion: String::new(),
            repair_scope: scope,
        }
    }

    #[test]
    fn tiered_issue_list_groups_by_severity() {
        let issues = vec![
            issue(AuditSeverity::Critical, "人设", "主角崩了", None),
            issue(AuditSeverity::Warning, "节奏", "节奏拖沓", None),
            issue(AuditSeverity::Info, "提示", "可改可不改", None),
        ];
        let zh = build_tiered_issue_list(&issues, false);
        assert!(zh.contains("## Critical（必须解决）\n- 人设: 主角崩了"));
        assert!(zh.contains("## High（应当改善）\n- 节奏: 节奏拖沓"));
        assert!(zh.contains("## Medium（参考建议）\n- 提示: 可改可不改"));
        let en = build_tiered_issue_list(&issues, true);
        assert!(en.contains("## Critical — Must Fix"));
    }

    #[test]
    fn auto_output_mode_routing() {
        use AutoOutputMode::*;
        assert_eq!(resolve_auto_output_mode(&[]), AllowFull);

        // repairScope 优先：structural → rewrite-only。
        let scoped = vec![issue(AuditSeverity::Critical, "X", "d", Some(RepairScope::Structural))];
        assert_eq!(resolve_auto_output_mode(&scoped), RewriteOnly);

        // 全部 local（scoped 占满 blocking）→ patch-only。
        let all_local = vec![
            issue(AuditSeverity::Warning, "A", "d", Some(RepairScope::Local)),
            issue(AuditSeverity::Critical, "B", "d", Some(RepairScope::Local)),
        ];
        assert_eq!(resolve_auto_output_mode(&all_local), PatchOnly);

        // 无 scope：正则分类。结构类（人设）→ rewrite-only。
        let structural = vec![issue(AuditSeverity::Critical, "Character Fidelity", "崩", None)];
        assert_eq!(resolve_auto_output_mode(&structural), RewriteOnly);

        // 全局部类 → patch-only。
        let local = vec![issue(AuditSeverity::Warning, "Fatigue word", "高疲劳词", None)];
        assert_eq!(resolve_auto_output_mode(&local), PatchOnly);

        // 只有 info → patch-only。
        let hints = vec![issue(AuditSeverity::Info, "whatever", "hint", None)];
        assert_eq!(resolve_auto_output_mode(&hints), PatchOnly);

        // 混合未知 → allow-full。
        let mixed = vec![issue(AuditSeverity::Critical, "未知分类", "不明问题", None)];
        assert_eq!(resolve_auto_output_mode(&mixed), AllowFull);
    }

    #[test]
    fn parse_output_extracts_tags_with_lookahead_equivalent() {
        let content = "=== FIXED_ISSUES ===\n修正A\n修正B\n\n=== REVISED_CONTENT ===\n新正文内容\n\n=== UPDATED_STATE ===\n新状态\n=== UPDATED_HOOKS ===\n新伏笔池";
        let out = parse_reviser_output(content, true, ReviseMode::Rewrite, "旧正文", AutoOutputMode::AllowFull);
        assert_eq!(out.fixed_issues, vec!["修正A".to_string(), "修正B".to_string()]);
        assert_eq!(out.revised_content, "新正文内容");
        assert_eq!(out.word_count, 5); // 「新正文内容」5 个 UTF-16 码元
        assert!(out.fixed_issues.len() == 2);
    }

    #[test]
    fn parse_output_legacy_falls_back_to_original() {
        let out = parse_reviser_output("没有任何标记", false, ReviseMode::Polish, "原章", AutoOutputMode::AllowFull);
        assert_eq!(out.revised_content, "原章");
        assert!(out.fixed_issues.is_empty());
    }

    #[test]
    fn parse_output_auto_rewrite_only_rejects_patches() {
        // rewrite-only 且只有 PATCHES → 原样返回（不回退补丁）。
        let content = "=== PATCHES ===\n--- PATCH 1 ---\nTARGET_TEXT:\n原句\nREPLACEMENT_TEXT:\n新句\n--- END PATCH ---";
        let out = parse_reviser_output(content, false, ReviseMode::Auto, "含原句的正文", AutoOutputMode::RewriteOnly);
        assert_eq!(out.revised_content, "含原句的正文");
        assert!(out.fixed_issues.is_empty());
    }

    #[test]
    fn parse_output_auto_patch_only_applies_patches() {
        let content = "=== FIXED_ISSUES ===\n修了原句\n\n=== PATCHES ===\n--- PATCH 1 ---\nTARGET_TEXT:\n原句\nREPLACEMENT_TEXT:\n替换句\n--- END PATCH ---";
        let out = parse_reviser_output(content, false, ReviseMode::Auto, "开头。原句。结尾。", AutoOutputMode::PatchOnly);
        assert_eq!(out.revised_content, "开头。替换句。结尾。");
        assert_eq!(out.fixed_issues, vec!["修了原句".to_string()]);
    }

    #[test]
    fn auto_system_prompt_embeds_genre_and_routing() {
        let gp = GenreProfile {
            name: "都市".into(),
            language: "zh".into(),
            numerical_system: true,
            ..Default::default()
        };
        let zh = build_auto_system_prompt("", &gp, "\n\n主角人设锁定：林动。", "\n3. 数值规则", WritingLanguage::Zh, None, AutoOutputMode::RewriteOnly);
        assert!(zh.contains("你是一位专业的都市网络小说修稿编辑"));
        assert!(zh.contains("主角人设锁定：林动。"));
        assert!(zh.contains("分流指令"));

        let en = build_auto_system_prompt("【LANGUAGE OVERRIDE】", &gp, "", "", WritingLanguage::En, None, AutoOutputMode::PatchOnly);
        assert!(en.starts_with("【LANGUAGE OVERRIDE】You are a professional 都市"));
        assert!(en.contains("ROUTING: The reviewer's blocking issues are local"));
    }

    #[test]
    fn legacy_system_prompt_mode_and_guardrails() {
        let gp = GenreProfile {
            name: "玄幻".into(),
            language: "zh".into(),
            numerical_system: false,
            ..Default::default()
        };
        let out = build_legacy_system_prompt("", &gp, "", "", "\n8. 护栏", ReviseMode::SpotFix);
        assert!(out.contains("修稿模式：定点修复"));
        assert!(out.contains("8. 护栏"));
        assert!(out.contains("9. spot-fix 只能输出局部补丁"));
    }

    #[test]
    fn reduced_control_block_shape() {
        let package = ContextPackage {
            chapter: 3,
            selected_context: vec![crate::models::input_governance::ContextSource {
                source: "story/current_focus.md".into(),
                reason: "焦点".into(),
                excerpt: Some("聚焦夺符".into()),
            }],
        };
        let rule_stack = crate::utils::context_assembly::build_governed_rule_stack(
            &["禁止降智".into()],
            &[],
            3,
        );
        let out = build_reduced_control_block(None, None, Some("# Chapter Intent\n## Goal\n目标"), &package, &rule_stack);
        assert!(out.contains("## 本章控制输入（由 Planner/Composer 编译）"));
        assert!(out.contains("### 已选上下文\n"));
        assert!(out.contains("- 硬护栏："));
        assert!(out.contains("- L4 -> L3: 禁止降智 (chapter:3/mustAvoid)"));
    }
}
