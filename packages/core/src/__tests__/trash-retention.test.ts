import { mkdir, mkdtemp, readdir, rm, stat, utimes, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  DEFAULT_TRASH_RETENTION_DAYS,
  listBookTrash,
  listChapterTrash,
  normalizeTrashRetentionDays,
  sweepExpiredTrash,
  trashDaysLeft,
  trashRetentionDaysFromRaw,
  trashSweepExpired,
} from "../state/trash.js";

// R60 第三批/1685 号：回收站保留期——有删除时刻记录的条目（书级条目、回滚批次）
// 过保留期自动真空删除；平铺条目没有时刻记录（mtime 是内容时刻，不是删除时刻），
// 不参与保留期。判定式两腿同一条（向量表两端同读）；清空只在服务启动装配点跑
//（读端点零写，1682 号定线）。

const here = dirname(fileURLToPath(import.meta.url));
const vectors: {
  readonly defaultDays: number;
  readonly normalization: readonly { readonly value: unknown; readonly days: number }[];
  readonly expiry: readonly {
    readonly name: string;
    readonly trashedAt: string;
    readonly days: number;
    readonly nowMillis: number;
    readonly expired: boolean;
  }[];
} = JSON.parse(readFileSync(join(here, "../../../studio/src/__tests__/golden/trash-retention-vectors.json"), "utf-8"));

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function tempRoot(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "inkos-trash-retention-"));
  roots.push(root);
  return root;
}

/** 本腿删出来的书级条目形态（根上有 work.json；列表函数认得它才会清）。 */
async function seedBookTrashEntry(root: string, entry: string): Promise<void> {
  await mkdir(join(root, ".trash", "books", entry), { recursive: true });
  await writeFile(join(root, ".trash", "books", entry, "work.json"), JSON.stringify({ id: entry.split("--")[0] }));
}

/** works 布局的书 + 章级回收站内容（批次/平铺）。 */
async function seedBookWithTrash(root: string, id: string, entries: { batch?: string; flat?: string }): Promise<string> {
  const bookDir = join(root, "works", id, "source");
  await mkdir(join(bookDir, "chapters", ".trash"), { recursive: true });
  await writeFile(join(bookDir, "book.json"), JSON.stringify({ id, title: id }));
  if (entries.batch !== undefined) {
    await mkdir(join(bookDir, "chapters", ".trash", entries.batch, "chapters"), { recursive: true });
    await writeFile(join(bookDir, "chapters", ".trash", entries.batch, "chapters", "0003_回.md"), "旧正文");
  }
  if (entries.flat !== undefined) {
    await writeFile(join(bookDir, "chapters", ".trash", entries.flat), "更旧的正文");
  }
  return bookDir;
}

describe("保留期·两腿同判定向量", () => {
  it("normalization：只有正整数认账，其余回落缺省 30", () => {
    expect(DEFAULT_TRASH_RETENTION_DAYS).toBe(vectors.defaultDays);
    for (const vector of vectors.normalization) {
      expect(normalizeTrashRetentionDays(vector.value), JSON.stringify(vector.value)).toBe(vector.days);
    }
  });

  it("expiry：恰等于 cutoff = 过期，差 1 秒 = 未过期", () => {
    for (const vector of vectors.expiry) {
      expect(trashSweepExpired(vector.trashedAt, vector.days, vector.nowMillis), vector.name).toBe(vector.expired);
    }
  });

  it("从 raw 配置取 retentionDays：缺失/形态不对回落缺省", () => {
    expect(trashRetentionDaysFromRaw({ trash: { retentionDays: 45 } })).toBe(45);
    expect(trashRetentionDaysFromRaw({})).toBe(DEFAULT_TRASH_RETENTION_DAYS);
    expect(trashRetentionDaysFromRaw({ trash: 5 })).toBe(DEFAULT_TRASH_RETENTION_DAYS);
    expect(trashRetentionDaysFromRaw({ trash: { retentionDays: "45" } })).toBe(DEFAULT_TRASH_RETENTION_DAYS);
    expect(trashRetentionDaysFromRaw(undefined)).toBe(DEFAULT_TRASH_RETENTION_DAYS);
  });

  it("daysLeft：向上取整；到点=0（已到期）；解不出时刻=null", () => {
    const now = Date.parse("2026-11-26T00:00:00.000Z");
    const at = Date.parse("2026-11-01T00:00:00.000Z");
    expect(trashDaysLeft(new Date(at).toISOString(), 30, now)).toBe(5); // 恰剩 5.0 个整天
    const almost = Date.parse("2026-10-27T12:00:00.000Z"); // 距到期（10-27→11-26）还剩半天
    expect(trashDaysLeft(new Date(almost).toISOString(), 30, now)).toBe(1);
    const expired = Date.parse("2026-10-27T00:00:00.000Z");
    expect(trashDaysLeft(new Date(expired).toISOString(), 30, now)).toBeLessThanOrEqual(0);
    expect(trashDaysLeft("不是时刻", 30, now)).toBeNull();
  });
});

describe("sweepExpiredTrash", () => {
  const NOW = Date.parse("2026-11-26T00:00:00.000Z"); // cutoff = 2026-10-27

  it("过期书级条目与回滚批次清掉，未过期与认不出的留着，平铺条目再老也不动", async () => {
    const root = await tempRoot();
    await seedBookTrashEntry(root, "gone--20260901000000"); // 过期
    await seedBookTrashEntry(root, "kept--20261125000000"); // 未过期
    await mkdir(join(root, ".trash", "books", "weird-form--20260101000000"), { recursive: true }); // 认不出（无 work.json）
    const bookDir = await seedBookWithTrash(root, "b1", {
      batch: "rollback-20260901000000-3", // 过期批次
    });
    const freshBookDir = await seedBookWithTrash(root, "b2", {
      batch: "rollback-20261125000000-3", // 未过期批次
      flat: "0009_旧形态.md",
    });
    // 平铺条目的 mtime 推到 2020 年——保留期不得拿内容时刻判它（删除时刻无记录）。
    await utimes(join(freshBookDir, "chapters", ".trash", "0009_旧形态.md"), new Date("2020-01-01"), new Date("2020-01-01"));

    const swept = await sweepExpiredTrash(root, [bookDir, freshBookDir], 30, NOW);
    expect(swept).toEqual({ books: 1, batches: 1 });

    const books = await listBookTrash(root);
    expect(books.map((entry) => entry.entry)).toEqual(["kept--20261125000000"]);
    // 认不出的形态不进列表也就不清扫——它必须原样留在磁盘上。
    await expect(stat(join(root, ".trash", "books", "weird-form--20260101000000"))).resolves.toBeTruthy();

    const b1Trash = await listChapterTrash(bookDir);
    expect(b1Trash).toEqual([]); // 过期批次整批清走（目录里唯一的条目）
    const b2Trash = await listChapterTrash(freshBookDir);
    expect(b2Trash).toHaveLength(2); // 未过期批次 + 平铺条目都还在
  });

  it("空回收站与不存在的目录都是零清扫、不抛", async () => {
    const root = await tempRoot();
    const swept = await sweepExpiredTrash(root, [join(root, "works", "ghost", "source")], 30, NOW);
    expect(swept).toEqual({ books: 0, batches: 0 });
  });

  it("批次被清走时批次目录整个消失（不留空壳）", async () => {
    const root = await tempRoot();
    const bookDir = await seedBookWithTrash(root, "b1", { batch: "rollback-20260601000000-2" });
    await sweepExpiredTrash(root, [bookDir], 30, NOW);
    const trashNames = await readdir(join(bookDir, "chapters", ".trash"));
    expect(trashNames).toEqual([]);
  });

  it("days=1 的紧窗口照常工作（清扫不是按天取整的粗筛）", async () => {
    const root = await tempRoot();
    await seedBookTrashEntry(root, "gone--20261125090000"); // 昨天 09:00
    await seedBookTrashEntry(root, "kept--20261125120000"); // 昨天 12:00
    const now = Date.parse("2026-11-26T10:00:00.000Z");
    const swept = await sweepExpiredTrash(root, [], 1, now); // cutoff = 昨天 10:00
    expect(swept).toEqual({ books: 1, batches: 0 });
    const books = await listBookTrash(root);
    expect(books.map((entry) => entry.entry)).toEqual(["kept--20261125120000"]);
  });
});
