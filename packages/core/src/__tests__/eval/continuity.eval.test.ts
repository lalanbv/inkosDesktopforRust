//! R8 连续性审查 agent 行为评测（369 号）。
//!
//! 漂移锚点：结构编辑角色/十二模式/repair_scope 路由/稀疏 memo 合法性/
//! JSON 输出格式模板——任一丢失即评测红。回放：mock LLM 返回审查 JSON →
//! 断言 AuditResult 结构化产出（passed/repair_scope 路由/severity 归一）。
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, vi } from "vitest";
import { describe, expect, it } from "vitest";
import { ContinuityAuditor } from "../../agents/continuity.js";
import { spyOnLoose } from "../spy-loose.js";
import {
  makeAgentCtx,
  makeBookFixture,
  SAMPLE_AUDIT_RESPONSE,
  SAMPLE_CHAPTER,
  ZERO_USAGE,
} from "./eval-fixtures.js";

describe("eval: continuity auditor (R8)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("prompt keeps the structural-editor contract anchors", async () => {
    const { root, bookDir } = await makeBookFixture("drift", "en");
    const auditor = new ContinuityAuditor(makeAgentCtx(root));
    const chatSpy = spyOnLoose(ContinuityAuditor.prototype, "chat")
      .mockResolvedValue({ content: SAMPLE_AUDIT_RESPONSE, usage: ZERO_USAGE });

    try {
      await auditor.auditChapter(bookDir, SAMPLE_CHAPTER, 12, "xuanhuan");
      const messages = chatSpy.mock.calls[0]?.[0] as ReadonlyArray<{ content: string }> | undefined;
      const systemPrompt = messages?.[0]?.content ?? "";
      const userPrompt = messages?.at(-1)?.content ?? "";

      // 角色与范围锚点。
      expect(systemPrompt).toContain("structural editor");
      expect(systemPrompt).toContain("repair_scope");
      // 输出格式模板锚点。
      expect(systemPrompt).toContain('"overall_score"');
      expect(systemPrompt).toContain('"repair_scope": "local|structural|unknown"');
      // 稀疏 memo 合法性锚点（防误伤降级）。
      expect(systemPrompt).toContain("Sparse chapter_memo is legitimate");
      // 用户侧含正文与上下文锚点。
      expect(userPrompt).toContain("祖符石");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("replays the audit fixture into a structured AuditResult", async () => {
    const { root, bookDir } = await makeBookFixture("replay", "en");
    const auditor = new ContinuityAuditor(makeAgentCtx(root));
    const spy = spyOnLoose(ContinuityAuditor.prototype, "chat")
      .mockResolvedValue({ content: SAMPLE_AUDIT_RESPONSE, usage: ZERO_USAGE });

    try {
      const result = await auditor.auditChapter(bookDir, SAMPLE_CHAPTER, 12, "xuanhuan");
      void spy;

      expect(result.passed).toBe(false);
      expect(result.overallScore).toBe(62);
      expect(result.issues).toHaveLength(2);
      expect(result.issues[0]!.severity).toBe("critical");
      expect(result.issues[0]!.repairScope).toBe("structural");
      expect(result.issues[0]!.category).toBe("timeline-coherence");
      // info 级 prose-surface 不计入通过判定（scope 锚点）。
      expect(result.issues[1]!.severity).toBe("info");
      expect(result.summary).toContain("时间线");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("chapter memo fixture flows into the audit prompt", async () => {
    const { root, bookDir } = await makeBookFixture("memo", "en");
    const auditor = new ContinuityAuditor(makeAgentCtx(root));
    const chatSpy = spyOnLoose(ContinuityAuditor.prototype, "chat")
      .mockResolvedValue({ content: SAMPLE_AUDIT_RESPONSE, usage: ZERO_USAGE });

    try {
      await auditor.auditChapter(bookDir, SAMPLE_CHAPTER, 12, "xuanhuan", {
        chapterMemo: {
          chapter: 12,
          goal: "越界",
          isGoldenOpening: false,
          body: "林动踏出边界，怀表复走。",
          threadRefs: ["mentor-debt"],
        },
      });
      // memoBlock 拼装位置随实现（system/user）——对全部消息拼接断言。
      const allContent = ((chatSpy.mock.calls[0]?.[0] as ReadonlyArray<{ content: string }>) ?? [])
        .map((message) => message.content)
        .join("\n");
      expect(allContent).toContain("越界");
      expect(allContent).toContain("林动踏出边界，怀表复走。");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});
