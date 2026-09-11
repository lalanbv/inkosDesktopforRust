/**
 * 续跑建议（G9/344 号，Phase B 批次三首项；WNW W3 失败隔离原则的采纳）。
 *
 * 批量/写章中断后，"从哪继续"不该靠用户猜：本模块把中断现场归纳为五种种
 * 显式建议，并给出可直接执行的 resumeFrom 章号与人话说明——
 *
 *   start-fresh          无正文 → 从第 1 章全新开始
 *   confirm-manual-edit  正文手改 → 必须作者确认，永不自动覆盖（最高优先）
 *   regenerate-chapter   最后一章不完整 → 重写本章
 *   resync-only          正文已存但状态未回灌 → 只补回灌，不重写
 *   continue-next        正文与状态一致 → 从下一章继续
 *
 * 决策表确定性、双端共享 golden（`golden/resume-advice-vectors.json`；
 * Rust 镜像 `engine-rs/src/utils/resume_advice.rs`，差分
 * `tests/golden_resume_advice_diff.rs`）。
 */

export const RESUME_ACTIONS = [
  "start-fresh",
  "confirm-manual-edit",
  "regenerate-chapter",
  "resync-only",
  "continue-next",
] as const;

export type ResumeAction = (typeof RESUME_ACTIONS)[number];

export interface ResumeAdviceInput {
  /** 已落盘章数（章节索引条数）。 */
  readonly savedChapters: number;
  /** 正文被手工修改（索引与正文不一致/哈希不符，由调用方检测）。 */
  readonly contentManuallyEdited?: boolean;
  /** 最后一章正文不完整（半章/空正文）。 */
  readonly lastChapterIncomplete?: boolean;
  /** 状态/摘要落后于正文（未回灌，resync 可修复）。 */
  readonly stateBehind?: boolean;
  readonly language?: "zh" | "en";
}

export interface ResumeAdvice {
  readonly action: ResumeAction;
  /** 建议的 resumeFrom 章号（start-fresh 无；regenerate 指向本章）。 */
  readonly resumeFrom?: number;
  readonly title: string;
  readonly detail: string;
}

/**
 * 决策表（顺序即优先级）：
 * 1. 无正文                        → start-fresh
 * 2. 正文手改                      → confirm-manual-edit（作者确认前不做任何自动动作）
 * 3. 最后一章不完整                → regenerate-chapter（resumeFrom = 本章）
 * 4. 状态未回灌                    → resync-only（只补回灌）
 * 5. 其余                          → continue-next（resumeFrom = 下一章）
 */
export function resolveResumeAdvice(input: ResumeAdviceInput): ResumeAdvice {
  const saved = Math.max(0, input.savedChapters);
  const isEn = input.language === "en";
  const next = saved + 1;

  if (saved === 0) {
    return {
      action: "start-fresh",
      title: isEn ? "Start fresh" : "全新开始",
      detail: isEn
        ? "No chapters saved yet — start writing from chapter 1."
        : "尚无已保存章节——从第 1 章开始写作。",
    };
  }
  if (input.contentManuallyEdited) {
    return {
      action: "confirm-manual-edit",
      resumeFrom: next,
      title: isEn ? "Manual edit needs confirmation" : "正文手改需确认",
      detail: isEn
        ? `Chapter ${saved} prose was edited by hand and will never be overwritten automatically. Confirm the edit, then continue from chapter ${next}.`
        : `第 ${saved} 章正文已被手工修改，系统不会自动覆盖。请先确认改动，再从第 ${next} 章继续。`,
    };
  }
  if (input.lastChapterIncomplete) {
    return {
      action: "regenerate-chapter",
      resumeFrom: saved,
      title: isEn ? "Regenerate the last chapter" : "重写最后一章",
      detail: isEn
        ? `Chapter ${saved} looks incomplete — rewrite it (resumeFrom=${saved}); earlier chapters are kept.`
        : `第 ${saved} 章疑似不完整——建议重写该章（resumeFrom=${saved}），此前章节保留。`,
    };
  }
  if (input.stateBehind) {
    return {
      action: "resync-only",
      resumeFrom: next,
      title: isEn ? "Resync state only" : "只需补回灌",
      detail: isEn
        ? `Chapter ${saved} prose is already saved but state/summaries are behind — run resync_chapter_state instead of rewriting.`
        : `第 ${saved} 章正文已保存，但状态/摘要未回灌——只需 resync_chapter_state 补回灌，无需重写正文。`,
    };
  }
  return {
    action: "continue-next",
    resumeFrom: next,
    title: isEn ? "Continue from the next chapter" : "从下一章继续",
    detail: isEn
      ? `Prose and state are consistent — continue from chapter ${next} (resumeFrom=${next}).`
      : `正文与状态一致——从第 ${next} 章继续（resumeFrom=${next}）。`,
  };
}

/**
 * 便捷口径：只有"已有章数"时的最小建议（resumeFrom 必填校验错误的
 * 提示文案场景）——等价 resolveResumeAdvice({savedChapters})，文案面向
 * 聊天/任务流错误消息拼接。
 */
export function formatResumeHint(savedChapters: number, language: "zh" | "en" = "en"): string {
  const advice = resolveResumeAdvice({ savedChapters, language });
  return language === "en"
    ? `${advice.title}: ${advice.detail}`
    : `${advice.title}：${advice.detail}`;
}
