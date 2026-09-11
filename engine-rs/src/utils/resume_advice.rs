//! 续跑建议（G9/344 号，Phase B 批次三首项）。
//!
//! TS 真源：`packages/core/src/utils/resume-advice.ts`；共享向量：
//! `packages/core/src/__tests__/golden/resume-advice-vectors.json`
//! （差分测试 `tests/golden_resume_advice_diff.rs`）。
//!
//! 五种显式建议决策表（顺序即优先级）+ resumeFrom 计算 + 双语人话文案，
//! 语义详见 TS 模块 doc。

use serde::Deserialize;
use serde::Serialize;

pub const RESUME_ACTIONS: [&str; 5] = [
    "start-fresh",
    "confirm-manual-edit",
    "regenerate-chapter",
    "resync-only",
    "continue-next",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ResumeAction {
    StartFresh,
    ConfirmManualEdit,
    RegenerateChapter,
    ResyncOnly,
    ContinueNext,
}

impl ResumeAction {
    pub fn as_str(self) -> &'static str {
        match self {
            ResumeAction::StartFresh => "start-fresh",
            ResumeAction::ConfirmManualEdit => "confirm-manual-edit",
            ResumeAction::RegenerateChapter => "regenerate-chapter",
            ResumeAction::ResyncOnly => "resync-only",
            ResumeAction::ContinueNext => "continue-next",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeAdviceInput {
    #[serde(default)]
    pub saved_chapters: i64,
    #[serde(default)]
    pub content_manually_edited: bool,
    #[serde(default)]
    pub last_chapter_incomplete: bool,
    #[serde(default)]
    pub state_behind: bool,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeAdvice {
    pub action: ResumeAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_from: Option<i64>,
    pub title: String,
    pub detail: String,
}

/// 决策表（顺序即优先级，golden 向量锁死）。
pub fn resolve_resume_advice(input: &ResumeAdviceInput) -> ResumeAdvice {
    let saved = input.saved_chapters.max(0);
    let is_en = input.language.as_deref() == Some("en");
    let next = saved + 1;

    if saved == 0 {
        return ResumeAdvice {
            action: ResumeAction::StartFresh,
            resume_from: None,
            title: if is_en { "Start fresh".into() } else { "全新开始".into() },
            detail: if is_en {
                "No chapters saved yet — start writing from chapter 1.".into()
            } else {
                "尚无已保存章节——从第 1 章开始写作。".into()
            },
        };
    }
    if input.content_manually_edited {
        let (title, detail) = if is_en {
            (
                "Manual edit needs confirmation".to_string(),
                format!(
                    "Chapter {saved} prose was edited by hand and will never be overwritten automatically. Confirm the edit, then continue from chapter {next}."
                ),
            )
        } else {
            (
                "正文手改需确认".to_string(),
                format!("第 {saved} 章正文已被手工修改，系统不会自动覆盖。请先确认改动，再从第 {next} 章继续。"),
            )
        };
        return ResumeAdvice {
            action: ResumeAction::ConfirmManualEdit,
            resume_from: Some(next),
            title,
            detail,
        };
    }
    if input.last_chapter_incomplete {
        let (title, detail) = if is_en {
            (
                "Regenerate the last chapter".to_string(),
                format!("Chapter {saved} looks incomplete — rewrite it (resumeFrom={saved}); earlier chapters are kept."),
            )
        } else {
            (
                "重写最后一章".to_string(),
                format!("第 {saved} 章疑似不完整——建议重写该章（resumeFrom={saved}），此前章节保留。"),
            )
        };
        return ResumeAdvice {
            action: ResumeAction::RegenerateChapter,
            resume_from: Some(saved),
            title,
            detail,
        };
    }
    if input.state_behind {
        let (title, detail) = if is_en {
            (
                "Resync state only".to_string(),
                format!("Chapter {saved} prose is already saved but state/summaries are behind — run resync_chapter_state instead of rewriting.")
            )
        } else {
            (
                "只需补回灌".to_string(),
                format!("第 {saved} 章正文已保存，但状态/摘要未回灌——只需 resync_chapter_state 补回灌，无需重写正文。"),
            )
        };
        return ResumeAdvice {
            action: ResumeAction::ResyncOnly,
            resume_from: Some(next),
            title,
            detail,
        };
    }
    let (title, detail) = if is_en {
        (
            "Continue from the next chapter".to_string(),
            format!("Prose and state are consistent — continue from chapter {next} (resumeFrom={next})."),
        )
    } else {
        (
            "从下一章继续".to_string(),
            format!("正文与状态一致——从第 {next} 章继续（resumeFrom={next}）。"),
        )
    };
    ResumeAdvice {
        action: ResumeAction::ContinueNext,
        resume_from: Some(next),
        title,
        detail,
    }
}

/// 最小建议文案（resumeFrom 必填校验错误的提示拼接场景）。
pub fn format_resume_hint(saved_chapters: i64, language: Option<&str>) -> String {
    let advice = resolve_resume_advice(&ResumeAdviceInput {
        saved_chapters,
        language: language.map(String::from),
        ..Default::default()
    });
    if language == Some("zh") {
        format!("{}：{}", advice.title, advice.detail)
    } else {
        format!("{}: {}", advice.title, advice.detail)
    }
}

/// 机器可读契约（双端 golden 锁形状）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeAdviceContract {
    pub actions: Vec<&'static str>,
    pub manual_edit_priority: &'static str,
    pub never_auto_overwrite_manual_edit: bool,
}

pub fn resume_advice_contract() -> ResumeAdviceContract {
    ResumeAdviceContract {
        actions: RESUME_ACTIONS.to_vec(),
        manual_edit_priority: "above-regenerate-and-resync",
        never_auto_overwrite_manual_edit: true,
    }
}
