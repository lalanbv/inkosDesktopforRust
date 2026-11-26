import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import { computeContextFingerprint } from "../forecast/context-builder.js";
import { compareByCodePoint } from "../utils/chapter-files.js";

// 1684 号（跨腿排序口径全仓扫）：forecast 上下文指纹的 canonical 输入里，文件对
// 按码点序排（与产品引擎 `fingerprint_canonical_json` 的字节序一致）——增补平面
// 与高位 BMP（全角标点）这对边界上 UTF-16 码元序是反的。指纹跨腿互认（缓存
// 有效性判定）的前提是两侧对同一输入算出同一个 sha256：这里与 Rust 侧测试
// 各钉同一个哈希值（输入顺序故意相反，排序正确才会同值）。
//
// Rust 侧同值钉：engine-rs/src/forecast/store.rs `fingerprint_matches_node_leg`。

const EXPECTED = "f582f187284453bf63e7381c0f576a29c6e6531e03f9521484401c15decd756f";

const canonicalOf = (pairs: ReadonlyArray<readonly [string, string]>): string =>
  JSON.stringify({
    baseChapter: 1,
    files: [...pairs].sort((a, b) => compareByCodePoint(a[0], b[0])),
  });

describe("computeContextFingerprint", () => {
  it("matches the pinned codepoint-order hash for astral vs fullwidth names", () => {
    expect(computeContextFingerprint({
      baseChapter: 1,
      files: [["😀.md", "a"], ["！.md", "b"]],
    })).toBe(EXPECTED);
  });

  it("is independent of the input pair order (canonical sort)", () => {
    const a = computeContextFingerprint({ baseChapter: 1, files: [["😀.md", "a"], ["！.md", "b"]] });
    const b = computeContextFingerprint({ baseChapter: 1, files: [["！.md", "b"], ["😀.md", "a"]] });
    expect(a).toBe(b);
  });

  it("equals a locally recomputed codepoint-order canonical hash", () => {
    const pairs: ReadonlyArray<readonly [string, string]> = [["😀.md", "a"], ["！.md", "b"]];
    expect(computeContextFingerprint({ baseChapter: 1, files: pairs }))
      .toBe(createHash("sha256").update(canonicalOf(pairs), "utf8").digest("hex"));
  });
});
