//! 403 号：运行遥测 golden 断言（v5 五轮规划 P0，第 32 守门域）。
//!
//! 唯一事实源 = `golden/run-log-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_run_log_diff.rs` 读同一文件差分。
//! 两组断言：环形缓冲 append/evict、投影聚合与 limit 截尾。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  DEFAULT_RUN_LOG_CAPACITY,
  RunLogBuffer,
  projectRunLog,
  type RunLogEntry,
  type RunLogSnapshot,
} from "../utils/run-log.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/run-log-vectors.json"), "utf-8"),
) as {
  capacity: {
    default: number;
    evictCase: {
      capacity: number;
      appended: Array<Record<string, unknown>>;
      expected: RunLogSnapshot;
    };
  };
  project: Array<{
    name: string;
    input: { snapshot: RunLogSnapshot; limit: number | null };
    expected: unknown;
  }>;
};

describe("run log (R26/403)", () => {
  it("keeps the v5 default capacity of 200", () => {
    expect(DEFAULT_RUN_LOG_CAPACITY).toBe(vectors.capacity.default);
  });

  it("evicts oldest entries beyond capacity per shared vectors", () => {
    const evictCase = vectors.capacity.evictCase;
    const buffer = new RunLogBuffer(evictCase.capacity);
    for (const entry of evictCase.appended) {
      buffer.append(entry as unknown as RunLogEntry);
    }
    expect(buffer.snapshot()).toEqual(evictCase.expected);
  });

  it("projects aggregates and tail limit per shared vectors", () => {
    for (const vector of vectors.project) {
      const got = projectRunLog(vector.input.snapshot, vector.input.limit ?? undefined);
      expect(got, vector.name).toEqual(vector.expected);
    }
  });
});
