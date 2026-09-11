import { z } from "zod";

/**
 * 作者友好报告契约（G8b/334 号，WNW W2 报告契约的采纳版）。
 *
 * 长任务（写章/批量/审改/导入/雷达等）收尾通知一律走三段式：
 *   完成情况 → 问题与异常耗时 → 下一步建议；
 * 异常三分类（已自动处理 / 建议确认 / 必须处理）让作者一眼分清
 * "要动手的"与"看看即可的"；禁止把裸事件名 / JSON / traceback 直接抛给作者。
 *
 * 单点契约：`buildTaskReport(event, data)` 把引擎 SSE 收尾事件 + payload
 * 直接转换为通知 `{ level, title, detail }`（studio 通知桥与聊天面共用，
 * 未知事件返回 null 由调用侧决定忽略）。
 */

export const REPORT_ISSUE_CATEGORY_SCHEMA = z.enum(["auto-handled", "needs-review", "must-handle"]);
export type ReportIssueCategory = z.infer<typeof REPORT_ISSUE_CATEGORY_SCHEMA>;

export const REPORT_ISSUE_CATEGORY_LABELS: Readonly<
  Record<ReportIssueCategory, { zh: string; en: string }>
> = {
  "auto-handled": { zh: "已自动处理", en: "Auto-handled" },
  "needs-review": { zh: "建议确认", en: "Needs review" },
  "must-handle": { zh: "必须处理", en: "Must handle" },
};

export interface ReportIssue {
  readonly category: ReportIssueCategory;
  readonly message: string;
}

/** 超过该耗时（毫秒）视为"异常耗时"，在报告的问题段标注。 */
export const ABNORMAL_DURATION_MS = 60_000;

/** 单行折叠上限（字符）：防 traceback / 长栈撑爆通知。 */
const MAX_ISSUE_CHARS = 200;

/** 折叠为单行人话（去换行/制表/多空格、截断）——禁裸 traceback 的落点。 */
export function collapseToLine(value: string, maxChars = MAX_ISSUE_CHARS): string {
  const collapsed = value.replace(/\s+/g, " ").trim();
  return collapsed.length > maxChars ? `${collapsed.slice(0, maxChars - 1)}…` : collapsed;
}

export function formatDurationMs(durationMs: number): string {
  if (durationMs < 1000) return `${durationMs}ms`;
  if (durationMs < 60_000) return `${(durationMs / 1000).toFixed(1)}s`;
  const minutes = Math.floor(durationMs / 60_000);
  const seconds = Math.round((durationMs % 60_000) / 1000);
  return `${minutes}m${seconds.toString().padStart(2, "0")}s`;
}

/**
 * 引擎错误事件/错误码 → 异常三分类。
 * 瞬时与自我恢复类（busy/中止/取消）= 已自动处理；上游依赖（LLM/网络/配额）
 * = 建议确认；其余（数据/状态/未知）= 必须处理。
 */
export function classifyEngineIssue(event: string, message: string): ReportIssue {
  const haystack = `${event} ${message}`.toLowerCase();
  if (/abort|cancel|busy|already processing/.test(haystack)) {
    return {
      category: "auto-handled",
      message: collapseToLine(message) || "任务已停止，可随时重试。",
    };
  }
  if (/llm|429|timeout|network|fetch|econn|upstream|服务|配额/.test(haystack)) {
    return {
      category: "needs-review",
      message: collapseToLine(message) || "上游服务暂时不可用，恢复后重试即可。",
    };
  }
  return { category: "must-handle", message: collapseToLine(message) || "任务失败，需人工介入。" };
}

export interface AuthorReportInput {
  /** 任务中文名（如 写章 / 批量调度 / 连续审改）。 */
  readonly taskLabel: string;
  /** 完成情况条目（每条一句人话）。 */
  readonly completed: readonly string[];
  /** 异常列表（可空；空则问题段写"无"）。 */
  readonly issues?: readonly ReportIssue[];
  /** 整轮耗时（毫秒；超阈值才在问题段标注）。 */
  readonly durationMs?: number;
  /** 下一步建议（每条一句人话）。 */
  readonly nextSteps: readonly string[];
  readonly language?: "zh" | "en";
}

export interface AuthorReport {
  /** 通知标题。 */
  readonly title: string;
  /** 通知中心单行摘要（完成 N · 问题 M · 耗时）。 */
  readonly summaryLine: string;
  /** 三段式全文（多行；聊天面/详情面用）。 */
  readonly text: string;
}

const T = {
  done: { zh: "完成情况", en: "Completed" },
  issues: { zh: "问题与异常耗时", en: "Issues & timing" },
  next: { zh: "下一步建议", en: "Next steps" },
  none: { zh: "无", en: "none" },
  total: { zh: "合计", en: "total" },
  slow: { zh: "（比通常更久）", en: "(longer than usual)" },
} as const;

function sectionLabel(key: keyof typeof T, language: "zh" | "en"): string {
  return language === "en" ? T[key].en : T[key].zh;
}

/** 三段式报告构建：完成情况 → 问题与异常耗时（三分类分组）→ 下一步建议。 */
export function buildAuthorReport(input: AuthorReportInput): AuthorReport {
  const language = input.language ?? "zh";
  const title = input.taskLabel;

  const completedLines = input.completed.length > 0
    ? input.completed.map((entry) => `- ${collapseToLine(entry)}`)
    : [`- ${T.none[language]}`];
  const issueLines = (input.issues ?? []).map((issue) =>
    `- [${REPORT_ISSUE_CATEGORY_LABELS[issue.category][language]}] ${issue.message}`,
  );
  const slowNote = input.durationMs !== undefined && input.durationMs > ABNORMAL_DURATION_MS
    ? [`- ${collapseToLine(`${formatDurationMs(input.durationMs)}${T.slow[language]}`)}`]
    : [];
  const issueSectionLines = issueLines.length > 0 || slowNote.length > 0
    ? [...issueLines, ...slowNote]
    : [`- ${T.none[language]}`];
  const nextLines = input.nextSteps.length > 0
    ? input.nextSteps.map((entry) => `- ${collapseToLine(entry)}`)
    : [`- ${T.none[language]}`];

  const issueCount = (input.issues ?? []).filter((issue) => issue.category !== "auto-handled").length;
  const durationText = input.durationMs !== undefined ? ` · ${formatDurationMs(input.durationMs)}` : "";
  const summaryLine = language === "en"
    ? `Done ${input.completed.length} · issues ${issueCount}${durationText}`
    : `完成 ${input.completed.length} 项 · 问题 ${issueCount}${durationText}`;

  const text = [
    `【${sectionLabel("done", language)}】`,
    ...completedLines,
    "",
    `【${sectionLabel("issues", language)}】`,
    ...issueSectionLines,
    "",
    `【${sectionLabel("next", language)}】`,
    ...nextLines,
  ].join("\n");

  return { title, summaryLine, text };
}

// ── 引擎收尾事件 → 报告（通知中心单点契约）──

// 对齐 studio 通知中心 NotificationLevel（info/warn/error）。
const NotificationLevelSchema = z.enum(["info", "warn", "error"]);
export type NotificationLevel = z.infer<typeof NotificationLevelSchema>;

export interface TaskNotification {
  readonly level: NotificationLevel;
  readonly title: string;
  readonly detail: string;
}

/** SSE 事件名 → 任务中文名（未知事件返回 null，不通知）。 */
export function resolveTaskLabel(event: string, language: "zh" | "en" = "zh"): string | null {
  const zh = language === "zh";
  const table: ReadonlyArray<readonly [string, string, string]> = [
    ["write:complete", "写章完成", "Chapter complete"],
    ["write:error", "写章失败", "Chapter failed"],
    ["draft:complete", "草稿生成完成", "Draft complete"],
    ["draft:error", "草稿生成失败", "Draft failed"],
    ["audit:complete", "连续审改完成", "Audit complete"],
    ["audit:error", "连续审改失败", "Audit failed"],
    ["revise:complete", "修订完成", "Revision complete"],
    ["revise:error", "修订失败", "Revision failed"],
    ["rewrite:complete", "重写完成", "Rewrite complete"],
    ["rewrite:error", "重写失败", "Rewrite failed"],
    ["style:complete", "文风导入完成", "Style import complete"],
    ["style:error", "文风导入失败", "Style import failed"],
    ["import:complete", "导入完成", "Import complete"],
    ["import:error", "导入失败", "Import failed"],
    ["fanfic:complete", "同人导入完成", "Fanfic import complete"],
    ["fanfic:error", "同人导入失败", "Fanfic import failed"],
    ["radar:complete", "市场雷达完成", "Radar complete"],
    ["radar:error", "市场雷达失败", "Radar failed"],
    ["daemon:chapter", "批量调度产出", "Batch chapter"],
    ["daemon:error", "批量调度出错", "Batch error"],
    ["book:created", "建书完成", "Book created"],
    ["book:error", "建书失败", "Book creation failed"],
    ["agent:error", "聊天任务出错", "Chat task error"],
  ];
  const hit = table.find(([key]) => key === event);
  if (!hit) return null;
  return zh ? hit[1] : hit[2];
}

/** 错误事件的下一步建议（三分类决定口径）。 */
function errorNextSteps(issues: readonly ReportIssue[], language: "zh" | "en"): readonly string[] {
  if (issues.some((issue) => issue.category === "must-handle")) {
    return language === "en"
      ? ["Check diagnostic logs for the cause", "Retry after fixing"]
      : ["查看诊断日志确认原因", "修复后重试该任务"];
  }
  if (issues.some((issue) => issue.category === "needs-review")) {
    return language === "en"
      ? ["Wait for the upstream service to recover", "Retry the task"]
      : ["等待上游服务恢复", "稍后重试该任务"];
  }
  return language === "en" ? ["Retry the task anytime"] : ["可随时重试该任务"];
}

/**
 * 引擎 SSE 收尾事件 → 作者友好通知（通知中心单点契约）。
 * data 为该事件的 payload（宽松读取，缺字段自动降级）；未知事件返回 null。
 */
export function buildTaskReport(
  event: string,
  data: unknown,
  language: "zh" | "en" = "zh",
): TaskNotification | null {
  const taskLabel = resolveTaskLabel(event, language);
  if (!taskLabel) return null;
  const payload = (typeof data === "object" && data !== null ? data : {}) as Record<string, unknown>;
  const optionalString = (key: string): string | undefined => {
    const value = payload[key];
    return typeof value === "string" && value.trim() ? value.trim() : undefined;
  };
  const optionalNumber = (key: string): number | undefined => {
    const value = payload[key];
    return typeof value === "number" && Number.isFinite(value) ? value : undefined;
  };

  // 完成类事件：完成情况一行 + 标准下一步。
  const isDone = event.endsWith(":complete") || event === "daemon:chapter" || event === "book:created";
  if (isDone) {
    const completed: string[] = [];
    if (event === "write:complete") {
      const title = optionalString("title");
      const chapter = optionalNumber("chapterNumber");
      const wordCount = optionalNumber("wordCount");
      completed.push(
        collapseToLine([
          title ? `《${title}》` : optionalString("bookId") ?? "",
          chapter !== undefined ? `第 ${chapter} 章` : "",
          wordCount !== undefined ? `${wordCount} 字` : "",
        ].filter(Boolean).join(" ")) || (language === "en" ? "chapter written" : "章节已写完"),
      );
    } else if (event === "daemon:chapter") {
      const chapter = optionalNumber("chapter");
      completed.push(
        collapseToLine(`${optionalString("bookId") ?? ""}${chapter !== undefined ? ` 第 ${chapter} 章` : ""} · ${optionalString("status") ?? "completed"}`),
      );
    } else if (event === "book:created") {
      completed.push(collapseToLine(optionalString("title") ?? optionalString("bookId") ?? (language === "en" ? "book ready" : "新书已就绪")));
    } else {
      completed.push(optionalString("result") ?? optionalString("status") ?? (language === "en" ? "task finished" : "任务已收尾"));
    }

    const nextByTask: ReadonlyArray<readonly string[]> = language === "en"
      ? [["Review the new chapter", "Continue writing or run a batch"], ["Open book details", "Start writing"], ["Open diagnostics if anything looks off"]]
      : [["在书籍详情审读新章", "继续写下一章或开启批量"], ["打开书籍详情开始写作"], ["如有异常查看诊断日志"]];
    const nextSteps = event === "book:created" ? nextByTask[1] : nextByTask[0];
    const report = buildAuthorReport({ taskLabel, completed, nextSteps, language: language });
    return { level: "info", title: report.title, detail: collapseToLine(completed.join("；")) };
  }

  // 失败类事件：三分类问题 + 分类口径的下一步。
  const rawError = optionalString("error") ?? optionalString("message") ?? event;
  const issue = classifyEngineIssue(event, rawError);
  const report = buildAuthorReport({
    taskLabel,
    completed: [],
    issues: [issue],
    nextSteps: errorNextSteps([issue], language),
    language: language,
  });
  return {
    level: issue.category === "must-handle" ? "error" : "warn",
    title: report.title,
    detail: collapseToLine(`[${REPORT_ISSUE_CATEGORY_LABELS[issue.category][language]}] ${issue.message}`),
  };
}
