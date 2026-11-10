import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { analyzeStyleFingerprint } from "../api/style-fingerprint.js";

// 964 号：风格指纹双端 golden——Rust engine-rs/src/agents/style_analyzer.rs 以
// include_str! 断言同一张表（逐向量全等）。
const golden = JSON.parse(
  readFileSync(join(fileURLToPath(new URL(".", import.meta.url)), "golden", "style-fingerprint-vectors.json"), "utf8"),
) as {
  vectors: Array<{ id: string; language: "zh" | "en"; sourceName: string; text: string; expected: Record<string, unknown> }>;
};

describe("style fingerprint golden (964)", () => {
  it("每个向量与参考实现逐字段全等（analyzedAt 注入不入表）", () => {
    expect(golden.vectors.length).toBeGreaterThanOrEqual(8);
    for (const vector of golden.vectors) {
      const { analyzedAt, ...actual } = analyzeStyleFingerprint(
        vector.text,
        vector.sourceName,
        vector.language,
        "2026-11-10T00:00:00.000Z",
      );
      expect(analyzedAt, vector.id).toBe("2026-11-10T00:00:00.000Z");
      expect(actual, vector.id).toEqual(vector.expected);
    }
  });

  it("关键语义锚点：相邻排比两处、开头并列按首现序、整数形键优先、引号并集剥离", () => {
    const byId = new Map(golden.vectors.map((vector) => [vector.id, vector.expected]));
    expect(byId.get("zh-parallelism-adjacent")?.rhetoricalFeatures).toEqual(["排比(2处)"]);
    expect(byId.get("zh-openings-tie-insertion-order")?.topPatterns).toEqual(["我们...(3次)", "他们...(3次)"]);
    expect(byId.get("zh-openings-integer-like-key-first")?.topPatterns).toEqual(["20...(3次)", "我们...(3次)"]);
    // 弯/直引号均不计入字表：同文去引号后 TTR 不变。
    const quoted = analyzeStyleFingerprint("“你好”他说。\"再见\"她说。", undefined, "zh", "x").vocabularyDiversity;
    const bare = analyzeStyleFingerprint("你好他说。再见她说。", undefined, "zh", "x").vocabularyDiversity;
    expect(quoted).toBe(bare);
  });
});
