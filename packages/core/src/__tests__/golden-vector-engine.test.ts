//! R12/380 号：sqlite-vec 引擎决策 golden 断言。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { resolveVectorEngine, SQLITE_VEC_DEFAULT_THRESHOLD } from "../utils/vector-engine.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/vector-engine-vectors.json"), "utf-8"),
) as {
  resolve: Array<{
    name: string;
    input: { chunkCount: number; vecExtensionAvailable: boolean; threshold?: number };
    expected: string;
  }>;
};

describe("vector engine contract (R12)", () => {
  it("resolves engines per shared vectors", () => {
    for (const vector of vectors.resolve) {
      expect(resolveVectorEngine(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("defaults threshold to 5000", () => {
    expect(SQLITE_VEC_DEFAULT_THRESHOLD).toBe(5000);
  });
});
