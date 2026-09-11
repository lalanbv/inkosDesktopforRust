//! G9/344 号：续跑建议 golden 断言（Phase B 批次三首项）。
//!
//! 唯一事实源 = `golden/resume-advice-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_resume_advice_diff.rs` 读同一文件差分。
//! 三组断言：决策表（五动作全覆盖 + 优先级交叉）、detail 关键语义、hint 拼接；
//! 另冻结契约形状（动作全集 + 手改永不自动覆盖）。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  RESUME_ACTIONS,
  formatResumeHint,
  resolveResumeAdvice,
  type ResumeAdviceInput,
} from "../utils/resume-advice.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/resume-advice-vectors.json"), "utf-8"),
) as {
  advice: Array<{
    name: string;
    input: ResumeAdviceInput;
    expected: { action: string; resumeFrom?: number | null; title: string; detailContains?: string };
  }>;
  hint: Array<{ name: string; input: { savedChapters: number; language: "zh" | "en" }; expectedContains: string[] }>;
  contract: unknown;
};

describe("resume advice (G9)", () => {
  it("resolves every advice per shared decision-table vectors", () => {
    for (const vector of vectors.advice) {
      const got = resolveResumeAdvice(vector.input);
      expect(got.action, vector.name).toBe(vector.expected.action);
      expect(got.resumeFrom, vector.name).toBe(vector.expected.resumeFrom ?? undefined);
      expect(got.title, vector.name).toBe(vector.expected.title);
      if (vector.expected.detailContains) {
        expect(got.detail, vector.name).toContain(vector.expected.detailContains);
      }
    }
    // 决策表五个动作全部被向量覆盖。
    const covered = new Set(vectors.advice.map((v) => v.expected.action));
    for (const action of RESUME_ACTIONS) {
      expect(covered.has(action), `action ${action} uncovered`).toBe(true);
    }
  });

  it("never auto-overwrites manual edits and always suggests resync before rewrite", () => {
    // WNW 铁律性质化校验：手改场景的 detail 必须含"确认/不覆盖"语义。
    for (const saved of [1, 3, 9]) {
      const advice = resolveResumeAdvice({ savedChapters: saved, contentManuallyEdited: true });
      expect(advice.action).toBe("confirm-manual-edit");
      expect(advice.resumeFrom).toBe(saved + 1);
    }
    // 状态落后时永远先建议回灌而不是重写正文。
    const behind = resolveResumeAdvice({ savedChapters: 4, stateBehind: true });
    expect(behind.action).toBe("resync-only");
    expect(behind.resumeFrom).toBe(5);
  });

  it("formats minimal resume hints per shared vectors", () => {
    for (const vector of vectors.hint) {
      const got = formatResumeHint(vector.input.savedChapters, vector.input.language);
      for (const fragment of vector.expectedContains) {
        expect(got, vector.name).toContain(fragment);
      }
    }
  });

  it("freezes the machine-readable contract shape", () => {
    expect(vectors.contract).toEqual({
      actions: [...RESUME_ACTIONS],
      manualEditPriority: "above-regenerate-and-resync",
      neverAutoOverwriteManualEdit: true,
    });
  });
});
