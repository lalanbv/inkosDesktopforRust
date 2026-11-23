//! 1666 号：jsonl 账本「读旧+拼+原子写」的两点加固锁——usage 账本与 prompt
//! 版本账本共用。
//!
//! 1. **并发追加丢行**：append-only 语义下，两个追加各自读到 N 行、各写 N+1
//!    行 = 丢一行（chat 多会话并行 / 保存按钮连点都能触发）。同一文件路径的
//!    追加须在进程内串行化。
//! 2. **读错误 ≠ 空账本**：账本读不出来（权限/IO）时追加必须中止。此前任何
//!    读错误都被当成「空账本」，随后原子写会用「只有新行」的内容覆盖掉整本
//!    历史——一次读失败毁一本账。缺失（ENOENT）才是空账本。
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { appendUsageRows, loadUsageLedger, usageLedgerPath } from "../utils/usage-ledger-store.js";
import { appendPromptVersion, loadPromptVersions, promptOverridePath } from "../utils/prompt-pack-store.js";
import { runSerially } from "../utils/append-queue.js";

const PARALLEL = 20;
// chmod 000 仅对非 root 的 POSIX 进程有效——Windows 文件照样可读，root 无视权限位。
const cantMakeUnreadable = process.platform === "win32" || process.getuid?.() === 0;

let roots: string[] = [];
afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true });
});

async function freshRoot(label: string): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), `inkos-ledger-append-${label}-`));
  roots.push(root);
  return root;
}

async function appendUsageOnce(root: string, index: number) {
  await appendUsageRows(root, [{
    ts: `2026-11-24T00:00:00.000Z`, serviceKey: `svc-${index}`, model: "m",
    promptTokens: 1, completionTokens: 1, totalTokens: 2,
  }]);
}

async function appendVersionOnce(root: string, index: number) {
  await appendPromptVersion(root, "longform.writer", "put", `# v${index}`, `2026-11-24T00:00:${String(index % 60).padStart(2, "0")}.000Z`);
}

describe("usage 账本并发追加（1666）", () => {
  it(`${PARALLEL} 路并发追加后一行不少（append-only）`, async () => {
    const root = await freshRoot("usage");
    await Promise.all(Array.from({ length: PARALLEL }, (_, index) => appendUsageOnce(root, index)));
    const rows = await loadUsageLedger(root);
    expect(rows).toHaveLength(PARALLEL);
    // 文件内容与读面一致（坏行/空行不产生）。
    const lines = (await readFile(usageLedgerPath(root), "utf-8")).split("\n").filter(Boolean);
    expect(lines).toHaveLength(PARALLEL);
    expect(new Set(rows.map((row) => row.serviceKey)).size).toBe(PARALLEL);
  });

  it("既有账本上的并发追加保留全部旧行", async () => {
    const root = await freshRoot("usage-existing");
    await appendUsageRows(root, [
      { ts: "2026-11-24T00:00:00.000Z", serviceKey: "old", model: "m", promptTokens: 1, completionTokens: 1, totalTokens: 2 },
    ]);
    await Promise.all(Array.from({ length: PARALLEL }, (_, index) => appendUsageOnce(root, index)));
    const rows = await loadUsageLedger(root);
    expect(rows).toHaveLength(PARALLEL + 1);
    expect(rows[0]!.serviceKey).toBe("old");
  });

  // 账本文件 chmod 000：追加必须中止，不得覆盖。
  it.skipIf(cantMakeUnreadable)("账本读不出来（权限）→ 追加中止且不覆盖既有内容", async () => {
    const root = await freshRoot("usage-unreadable");
    await mkdir(join(root, ".inkos"), { recursive: true });
    const path = usageLedgerPath(root);
    const original = JSON.stringify({ ts: "2026-11-24T00:00:00.000Z", serviceKey: "keep", model: "m", promptTokens: 1, completionTokens: 1, totalTokens: 2 }) + "\n";
    await writeFile(path, original);
    await chmod(path, 0o000);
    try {
      await expect(appendUsageOnce(root, 1)).rejects.toMatchObject({ code: "EACCES" });
      await expect(loadUsageLedger(root)).rejects.toMatchObject({ code: "EACCES" });
    } finally {
      await chmod(path, 0o600);
    }
    // 旧行原样保留——没有被「只有新行」的内容覆盖。
    expect(await readFile(path, "utf-8")).toBe(original);
  });

  it("账本缺失 = 空账本（读面 []，追加创建文件）", async () => {
    const root = await freshRoot("usage-missing");
    expect(await loadUsageLedger(root)).toEqual([]);
    await appendUsageOnce(root, 0);
    expect(await loadUsageLedger(root)).toHaveLength(1);
  });

  // 缺字段的「像行的对象」若被收下，投影求和得 NaN → 预算判定整个带偏。
  it("坏行宽容=跳过（Rust serde 同款形状守卫）：缺字段/负数/非对象不入账", async () => {
    const root = await freshRoot("usage-badlines");
    await mkdir(join(root, ".inkos"), { recursive: true });
    const good = { ts: "2026-11-24T00:00:00.000Z", serviceKey: "ok", model: "m", promptTokens: 1, completionTokens: 1, totalTokens: 2 };
    await writeFile(usageLedgerPath(root), [
      JSON.stringify(good),
      JSON.stringify({ ...good, serviceKey: "missing-total", totalTokens: undefined }),
      JSON.stringify({ ...good, serviceKey: "negative", promptTokens: -1, totalTokens: 0 }),
      JSON.stringify({ ...good, serviceKey: "fraction", totalTokens: 1.5 }),
      JSON.stringify({ ...good, serviceKey: "ts-not-string", ts: 20261124 }),
      "null",
      "123",
      "not json",
      "",
    ].join("\n") + "\n");
    expect((await loadUsageLedger(root)).map((row) => row.serviceKey)).toEqual(["ok"]);
  });
});

describe("runSerially（1666）", () => {
  it("同键严格先后、不同键互不阻塞；前序失败不阻断后续，错误原样上抛", async () => {
    const order: string[] = [];
    let release: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const first = runSerially("key", async () => {
      order.push("a:start");
      await gate;
      order.push("a:end");
      throw new Error("boom");
    });
    const second = runSerially("key", async () => { order.push("b"); });
    await runSerially("other-key", async () => { order.push("c"); });
    expect(order).toEqual(["a:start", "c"]);
    release!();
    await expect(first).rejects.toThrow("boom");
    await second;
    expect(order).toEqual(["a:start", "c", "a:end", "b"]);
  });
});

describe("prompt 版本账本并发追加（1666）", () => {
  it(`${PARALLEL} 路并发追加后一行不少，回滚目标可按 ts 定位`, async () => {
    const root = await freshRoot("versions");
    await Promise.all(Array.from({ length: PARALLEL }, (_, index) => appendVersionOnce(root, index)));
    const versions = await loadPromptVersions(root);
    expect(versions).toHaveLength(PARALLEL);
    // 每个版本都能按 (promptId, ts) 读回——丢行会让对应版本无法回滚。
    const { readPromptVersion } = await import("../utils/prompt-pack-store.js");
    for (let index = 0; index < PARALLEL; index += 1) {
      const record = await readPromptVersion(root, "longform.writer", `2026-11-24T00:00:${String(index % 60).padStart(2, "0")}.000Z`);
      // ts 秒位循环 60——20 路互不重复。
      expect(record?.content, `version ${index}`).toBe(`# v${index}`);
    }
    expect(promptOverridePath(root, "longform.writer")).toBe(join(root, "prompt", "longform", "writer.md"));
  });

  it.skipIf(cantMakeUnreadable)("版本账本读不出来（权限）→ 追加中止且不覆盖既有内容", async () => {
    const root = await freshRoot("versions-unreadable");
    await mkdir(join(root, "prompt"), { recursive: true });
    const path = join(root, "prompt", ".versions.jsonl");
    const original = JSON.stringify({ ts: "2026-11-24T00:00:00.000Z", promptId: "longform.writer", action: "put", content: "# keep" }) + "\n";
    await writeFile(path, original);
    await chmod(path, 0o000);
    try {
      await expect(appendVersionOnce(root, 1)).rejects.toMatchObject({ code: "EACCES" });
      await expect(loadPromptVersions(root)).rejects.toMatchObject({ code: "EACCES" });
    } finally {
      await chmod(path, 0o600);
    }
    expect(await readFile(path, "utf-8")).toBe(original);
  });

  it("版本账本缺失 = 空账本", async () => {
    const root = await freshRoot("versions-missing");
    expect(await loadPromptVersions(root)).toEqual([]);
  });

  it("坏行宽容=跳过：四个字段不齐或非串的行不入账", async () => {
    const root = await freshRoot("versions-badlines");
    await mkdir(join(root, "prompt"), { recursive: true });
    const good = { ts: "2026-11-24T00:00:00.000Z", promptId: "longform.writer", action: "put", content: "# ok" };
    await writeFile(join(root, "prompt", ".versions.jsonl"), [
      JSON.stringify(good),
      JSON.stringify({ ...good, content: undefined }),
      JSON.stringify({ ...good, ts: 20261124 }),
      JSON.stringify({ ...good, promptId: null }),
      "[]",
      "garbage",
    ].join("\n") + "\n");
    const rows = await loadPromptVersions(root);
    expect(rows.map((row) => row.content)).toEqual(["# ok"]);
  });
});
