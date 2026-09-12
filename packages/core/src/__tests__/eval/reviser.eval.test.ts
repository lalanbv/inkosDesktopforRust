//! R8 reviser 行为评测（369 号）。
//!
//! 漂移锚点：问题清单注入/修订输出 TAG 模板/模式护栏（rewrite 局部优先）；
//! 回放：mock LLM 修订产物 → ReviseOutput 结构化断言。
import { mkdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, vi } from "vitest";
import { describe, expect, it } from "vitest";
import { ReviserAgent } from "../../agents/reviser.js";
import type { AuditIssue } from "../../agents/continuity.js";
import { makeAgentCtx, makeBookFixture, SAMPLE_CHAPTER, ZERO_USAGE } from "./eval-fixtures.js";

const CRITICAL_ISSUE: AuditIssue = {
  severity: "critical",
  category: "timeline-coherence",
  description: "怀表时间线与第 3 章设定冲突",
  suggestion: "补充怀表停摆的触发事件",
};

const REVISION_RESPONSE = [
  "=== FIXED_ISSUES ===",
  "- 怀表时间线已补触发事件",
  "",
  "=== REVISED_CONTENT ===",
  "修订后的章节正文。",
  "",
  "=== UPDATED_STATE ===",
  "状态卡",
].join("\n");

function makeAgent(root: string): ReviserAgent {
  return new ReviserAgent(makeAgentCtx(root));
}

describe("eval: reviser (R8)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("prompt injects audit issues and keeps revision TAG discipline", async () => {
    const { root, bookDir } = await makeBookFixture("reviser-drift", "en");
    await mkdir(join(bookDir, "story"), { recursive: true });
    const agent = makeAgent(root);
    const chatSpy = vi
      .spyOn(ReviserAgent.prototype as never, "chat" as never)
      .mockResolvedValue({ content: REVISION_RESPONSE, usage: ZERO_USAGE });

    try {
      await agent.reviseChapter(bookDir, SAMPLE_CHAPTER, 12, [CRITICAL_ISSUE], "rewrite", "xuanhuan");
      const messages = chatSpy.mock.calls[0]?.[0] as ReadonlyArray<{ content: string }> | undefined;
      const systemPrompt = messages?.[0]?.content ?? "";
      const userPrompt = messages?.at(-1)?.content ?? "";

      expect(systemPrompt).toContain("MUST be in English");
      expect(userPrompt).toContain("timeline-coherence");
      expect(userPrompt).toContain("怀表时间线");
      expect(systemPrompt).toContain("=== REVISED_CONTENT ===");
      expect(systemPrompt).toContain("=== FIXED_ISSUES ===");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("replays the revision fixture into a structured ReviseOutput", async () => {
    const { root, bookDir } = await makeBookFixture("reviser-replay", "en");
    await mkdir(join(bookDir, "story"), { recursive: true });
    const agent = makeAgent(root);
    vi.spyOn(ReviserAgent.prototype as never, "chat" as never).mockResolvedValue({
      content: REVISION_RESPONSE,
      usage: ZERO_USAGE,
    });

    try {
      const output = await agent.reviseChapter(bookDir, SAMPLE_CHAPTER, 12, [CRITICAL_ISSUE], "rewrite", "xuanhuan");
      expect(output.fixedIssues.join("\n")).toContain("怀表时间线");
      expect(output.revisedContent).toContain("修订后的章节正文");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("keeps rewrite mode local-first instead of wholesale replacement", async () => {
    const { root, bookDir } = await makeBookFixture("reviser-guardrail", "en");
    await mkdir(join(bookDir, "story"), { recursive: true });
    const agent = makeAgent(root);
    const chatSpy = vi
      .spyOn(ReviserAgent.prototype as never, "chat" as never)
      .mockResolvedValue({ content: REVISION_RESPONSE, usage: ZERO_USAGE });

    try {
      await agent.reviseChapter(bookDir, SAMPLE_CHAPTER, 12, [CRITICAL_ISSUE], "rewrite", "xuanhuan");
      const systemPrompt = (chatSpy.mock.calls[0]?.[0] as ReadonlyArray<{ content: string }>)?.[0]?.content ?? "";
      // 护栏锚点：rewrite 模式优先保留原文句段，禁止整章推倒重写。
      expect(systemPrompt).toContain("优先保留原文的绝大部分句段");
      expect(systemPrompt).toContain("禁止整章推倒重写");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});

// tmpdir 引用保持（makeBookFixture 内部用 mkdtemp/tmpdir；此处显式保 import 平衡）。
void tmpdir;
void join;
