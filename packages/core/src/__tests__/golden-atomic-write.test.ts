//! 404 号：持久层原子写 golden 断言（v5 五轮规划 P2，第 33 守门域）。
//!
//! 唯一事实源 = `golden/atomic-write-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_atomic_write_diff.rs` 读同一文件差分。
//! 断言：常量契约 + 重试决策纯函数。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  ATOMIC_WRITE_MAX_RETRIES,
  ATOMIC_WRITE_RETRY_DELAY_MS,
  ATOMIC_WRITE_RETRYABLE_CODES,
  atomicWriteRetryDecision,
  writeTextAtomic,
} from "../utils/atomic-write.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/atomic-write-vectors.json"), "utf-8"),
) as {
  contract: { maxRetries: number; retryDelayMs: number; retryableCodes: string[] };
  retryDecision: Array<{
    name: string;
    input: { errorCode: string | null; attempt: number };
    expected: boolean;
  }>;
};

describe("atomic write (R29/404)", () => {
  it("keeps the WNV-derived retry contract constants", () => {
    expect(ATOMIC_WRITE_MAX_RETRIES).toBe(vectors.contract.maxRetries);
    expect(ATOMIC_WRITE_RETRY_DELAY_MS).toBe(vectors.contract.retryDelayMs);
    expect([...ATOMIC_WRITE_RETRYABLE_CODES]).toEqual(vectors.contract.retryableCodes);
  });

  it("decides rename-lock retries per shared vectors", () => {
    for (const vector of vectors.retryDecision) {
      expect(
        atomicWriteRetryDecision(vector.input.errorCode, vector.input.attempt),
        vector.name,
      ).toBe(vector.expected);
    }
  });

  it("retries locked renames with linear backoff and lands the content (WNW v6.2.1)", async () => {
    const dir = await mkdtemp(join(tmpdir(), "atomic-write-"));
    const target = join(dir, "book.json");
    const lockError = Object.assign(new Error("EBUSY: resource busy"), { code: "EBUSY" });
    let renameAttempts = 0;
    const sleeps: number[] = [];

    await writeTextAtomic(target, '{"v":1}', {
      rename: async (from, to) => {
        renameAttempts += 1;
        if (renameAttempts <= 2) throw lockError;
        await (await import("node:fs/promises")).rename(from, to);
      },
      sleep: async (ms) => {
        sleeps.push(ms);
      },
    });

    expect(renameAttempts).toBe(3); // 首次 + 2 次锁重试
    expect(sleeps).toEqual([ATOMIC_WRITE_RETRY_DELAY_MS, ATOMIC_WRITE_RETRY_DELAY_MS * 2]);
    expect(await readFile(target, "utf-8")).toBe('{"v":1}');
    await rm(dir, { recursive: true, force: true });
  });

  it("gives up on non-retryable errors, removes the tmp file, and throws", async () => {
    const dir = await mkdtemp(join(tmpdir(), "atomic-write-"));
    const target = join(dir, "book.json");
    const fatal = Object.assign(new Error("ENOSPC"), { code: "ENOSPC" });

    await expect(
      writeTextAtomic(target, "x", {
        rename: async () => {
          throw fatal;
        },
        sleep: async () => {},
      }),
    ).rejects.toThrow("ENOSPC");

    const leftovers = (await readdir(dir)).filter((name) => name.includes(".tmp-"));
    expect(leftovers).toEqual([]); // 放弃时清理临时文件
    await rm(dir, { recursive: true, force: true });
  });
});
