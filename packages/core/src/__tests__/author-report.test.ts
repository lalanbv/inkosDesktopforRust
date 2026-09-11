//! G8b/334 号：作者友好报告契约单测。
//!
//! 三段式（完成情况/问题与异常耗时/下一步建议）+ 异常三分类
//! （已自动处理/建议确认/必须处理）+ 禁裸 JSON/traceback 的单行折叠，
//! 以及引擎 SSE 收尾事件 → 通知的单点契约 `buildTaskReport`。
import { describe, expect, it } from "vitest";
import {
  ABNORMAL_DURATION_MS,
  buildAuthorReport,
  buildTaskReport,
  classifyEngineIssue,
  collapseToLine,
  formatDurationMs,
  resolveTaskLabel,
} from "../utils/author-report.js";

describe("author report contract (G8b)", () => {
  it("renders the three-section report in fixed order", () => {
    const report = buildAuthorReport({
      taskLabel: "写章完成",
      completed: ["《夜港》第 3 章 · 2100 字"],
      nextSteps: ["在书籍详情审读新章", "继续写下一章或开启批量"],
    });
    const sections = report.text.split("\n").filter((line) => line.startsWith("【"));
    expect(sections).toEqual(["【完成情况】", "【问题与异常耗时】", "【下一步建议】"]);
    expect(report.text).toContain("- 《夜港》第 3 章 · 2100 字");
    // 无问题时问题段写"无"。
    expect(report.text).toMatch(/【问题与异常耗时】\n- 无/);
    expect(report.summaryLine).toContain("完成 1 项");
  });

  it("groups issues by the three-category contract", () => {
    const report = buildAuthorReport({
      taskLabel: "写章失败",
      completed: [],
      issues: [
        { category: "auto-handled", message: "任务已停止" },
        { category: "needs-review", message: "上游 429" },
        { category: "must-handle", message: "状态冲突" },
      ],
      nextSteps: ["修复后重试"],
    });
    expect(report.text).toContain("- [已自动处理] 任务已停止");
    expect(report.text).toContain("- [建议确认] 上游 429");
    expect(report.text).toContain("- [必须处理] 状态冲突");
    // 摘要只统计需要作者动手的问题（auto-handled 不计）。
    expect(report.summaryLine).toContain("问题 2");
  });

  it("flags abnormal durations beyond the threshold", () => {
    const slow = buildAuthorReport({
      taskLabel: "批量调度",
      completed: ["第 4 章"],
      durationMs: ABNORMAL_DURATION_MS + 32_000,
      nextSteps: [],
    });
    expect(slow.text).toContain("1m32s");
    expect(slow.text).toContain("比通常更久");
    const fast = buildAuthorReport({
      taskLabel: "批量调度",
      completed: ["第 4 章"],
      durationMs: 5_000,
      nextSteps: [],
    });
    expect(fast.text).not.toContain("比通常更久");
  });

  it("classifies engine failures into the three categories", () => {
    expect(classifyEngineIssue("agent:error", "aborted by user").category).toBe("auto-handled");
    expect(classifyEngineIssue("write:error", "AGENT_BUSY: already processing").category).toBe("auto-handled");
    expect(classifyEngineIssue("write:error", "LLM upstream 429 too many requests").category).toBe("needs-review");
    expect(classifyEngineIssue("write:error", "ECONNREFUSED 127.0.0.1:8080").category).toBe("needs-review");
    expect(classifyEngineIssue("write:error", "状态校验失败：章节索引与正文不一致").category).toBe("must-handle");
  });

  it("collapses tracebacks and long payloads to one line", () => {
    const traceback = "Error: boom\n  at fn (file.ts:1:1)\n  at run (run.ts:2:2)";
    expect(collapseToLine(traceback)).toBe("Error: boom at fn (file.ts:1:1) at run (run.ts:2:2)");
    expect(collapseToLine("x".repeat(500))).toMatch(/…$/);
    expect(collapseToLine("x".repeat(500)).length).toBeLessThanOrEqual(200);
  });

  it("formats durations without raw millisecond dumps", () => {
    expect(formatDurationMs(320)).toBe("320ms");
    expect(formatDurationMs(2100)).toBe("2.1s");
    expect(formatDurationMs(92_000)).toBe("1m32s");
  });

  it("maps known finish events to friendly notifications and ignores unknown ones", () => {
    expect(resolveTaskLabel("write:complete")).toBe("写章完成");
    expect(resolveTaskLabel("daemon:error")).toBe("批量调度出错");
    expect(resolveTaskLabel("log")).toBeNull();

    const done = buildTaskReport("write:complete", {
      bookId: "night-harbor",
      chapterNumber: 3,
      status: "completed",
      title: "夜港",
      wordCount: 2100,
    });
    expect(done).toMatchObject({ level: "info", title: "写章完成" });
    expect(done?.detail).toContain("《夜港》");
    expect(done?.detail).toContain("第 3 章");
    // 禁裸事件名/JSON：detail 是人话单行。
    expect(done?.detail).not.toContain("{");

    const failure = buildTaskReport("write:error", { bookId: "night-harbor", error: "LLM 429 quota exceeded" });
    expect(failure).toMatchObject({ level: "warn", title: "写章失败" });
    expect(failure?.detail).toContain("建议确认");

    const must = buildTaskReport("daemon:error", { error: "章节索引与正文不一致" });
    expect(must?.level).toBe("error");
    expect(must?.detail).toContain("必须处理");

    expect(buildTaskReport("log", { x: 1 })).toBeNull();
    expect(buildTaskReport("ping", null)).toBeNull();
  });
});
