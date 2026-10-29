import { describe, expect, it } from "vitest";
import {
  pendingTrustSourcePath,
  serializeSkillFolder,
  selectedSkillIdsForSend,
  toggleSelectedSkillIds,
} from "./skill-ui-state";

describe("skill-ui-state", () => {
  it("toggles selected skill ids with normalization", () => {
    expect(toggleSelectedSkillIds([], "Open World Play")).toEqual(["open-world-play"]);
    expect(toggleSelectedSkillIds(["open-world-play"], "open-world-play")).toEqual([]);
    expect(toggleSelectedSkillIds(["open-world-play"], "  Script@Skill  ")).toEqual([
      "open-world-play",
      "script-skill",
    ]);
  });

  it("returns undefined when no skills should be sent", () => {
    expect(selectedSkillIdsForSend([])).toBeUndefined();
    expect(selectedSkillIdsForSend(["", "   "])).toBeUndefined();
  });

  it("deduplicates selected skill ids for send", () => {
    expect(selectedSkillIdsForSend(["open-world-play", "Open World Play", "script"])).toEqual([
      "open-world-play",
      "script",
    ]);
  });

  it("rejects oversized skill folders before reading file contents", async () => {
    let reads = 0;
    const files = Array.from({ length: 129 }, (_, index) => ({
      name: `file-${index}.md`,
      size: 1,
      type: "text/markdown",
      webkitRelativePath: `skill/file-${index}.md`,
      arrayBuffer: async () => {
        reads += 1;
        return new ArrayBuffer(1);
      },
    })) as unknown as File[];

    await expect(serializeSkillFolder(files)).rejects.toThrow(/128 files/i);
    expect(reads).toBe(0);
  });

  it("preserves browser folder-relative paths when serializing an imported skill", async () => {
    const files = [
      {
        name: "SKILL.md",
        size: 4,
        type: "text/markdown",
        webkitRelativePath: "writer-distillation/SKILL.md",
        arrayBuffer: async () => new TextEncoder().encode("body").buffer,
      },
      {
        name: "rubric.md",
        size: 6,
        type: "text/markdown",
        webkitRelativePath: "writer-distillation/references/rubric.md",
        arrayBuffer: async () => new TextEncoder().encode("rubric").buffer,
      },
    ] as unknown as File[];

    const result = await serializeSkillFolder(files);

    expect(result.map((file) => file.path)).toEqual([
      "writer-distillation/SKILL.md",
      "writer-distillation/references/rubric.md",
    ]);
    expect(result[0]?.dataUrl).toMatch(/^data:text\/markdown;base64,/);
  });
});

// ── 769 号：待信任来源判定（768 载入闸诊断的消费面）─────────────
describe("pendingTrustSourcePath（769 号）", () => {
  it("载入闸诊断 + 非空 path → 返回可确认的来源路径", () => {
    expect(pendingTrustSourcePath({
      message: "external skill source pending trust confirmation; confirm it in the skills trust settings.",
      path: "/ext/skills",
    })).toBe("/ext/skills");
  });

  it("普通格式诊断 → null（不走信任确认）", () => {
    expect(pendingTrustSourcePath({ message: "SKILL.md frontmatter requires name.", path: "/x" })).toBeNull();
  });

  it("待信任串但 path 缺失/空 → null（无可确认目标）", () => {
    expect(pendingTrustSourcePath({ message: "pending trust confirmation" })).toBeNull();
    expect(pendingTrustSourcePath({ message: "pending trust confirmation", path: "" })).toBeNull();
  });

  it("message 缺失 → null", () => {
    expect(pendingTrustSourcePath({ path: "/x" })).toBeNull();
  });
});
