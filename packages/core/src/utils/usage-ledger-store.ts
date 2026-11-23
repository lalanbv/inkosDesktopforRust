/**
 * R55/1661 号：用量账本存储（Rust 镜像 = engine-rs/src/utils/usage_ledger.rs；
 * 纯函数=usage-budget.ts）。路径 = `{root}/.inkos/usage-log.jsonl` append-only
 * （读旧+拼+原子写；坏行宽容；仅 usage 精确值元数据）。
 *
 * 1666 号两点加固（prompt 版本账本同款，锁在 `ledger-append.flow.test`）：
 * 1. 追加按账本路径**进程内串行化**——并发追加此前会丢行（20 路并发实测只剩
 *    1 行），append-only 语义被破；chat 流水写点落地后多会话并行即触发。
 * 2. 读错误 ≠ 空账本——只有「缺失」才是空；读不出来（权限/IO）时读面上抛、
 *    追加中止。此前任何读错误都被当成空账本，随后原子写会用「只有新行」的
 *    内容覆盖掉整本历史。
 */
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { runSerially } from "./append-queue.js";
import { writeTextAtomic } from "./atomic-write.js";
import type { UsageLogRow } from "./usage-budget.js";

export function usageLedgerPath(root: string): string {
  return join(root, ".inkos", "usage-log.jsonl");
}

/** 读账本文本；缺失（或路径分量不是目录）= undefined，其余读错误上抛。 */
async function readLedgerText(path: string): Promise<string | undefined> {
  try {
    return await readFile(path, "utf-8");
  } catch (error) {
    const code = (error as NodeJS.ErrnoException | null)?.code;
    if (code === "ENOENT" || code === "ENOTDIR") return undefined;
    throw error;
  }
}

/**
 * 账本行形状守卫（Rust serde 同款：六个字段齐全、类型正确才算一行）。
 * 缺字段的「像行的对象」若被收下，投影求和会得到 NaN——把预算判定整个带偏。
 */
function isUsageLogRow(value: unknown): value is UsageLogRow {
  if (!value || typeof value !== "object") return false;
  const row = value as Record<string, unknown>;
  const isCount = (n: unknown) => typeof n === "number" && Number.isInteger(n) && n >= 0;
  return typeof row.ts === "string" && typeof row.serviceKey === "string" && typeof row.model === "string"
    && isCount(row.promptTokens) && isCount(row.completionTokens) && isCount(row.totalTokens);
}

function parseLedger(text: string): UsageLogRow[] {
  const rows: UsageLogRow[] = [];
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const parsed: unknown = JSON.parse(trimmed);
      if (isUsageLogRow(parsed)) rows.push(parsed);
    } catch {
      // 坏行跳过（宽容读）。
    }
  }
  return rows;
}

export async function loadUsageLedger(root: string): Promise<UsageLogRow[]> {
  const text = await readLedgerText(usageLedgerPath(root));
  return text === undefined ? [] : parseLedger(text);
}

export async function appendUsageRows(root: string, rows: ReadonlyArray<UsageLogRow>): Promise<void> {
  if (rows.length === 0) return;
  const path = usageLedgerPath(root);
  await runSerially(path, async () => {
    // 读错误在此上抛 → 追加中止，既有账本原样保留。
    const existing = await loadUsageLedger(root);
    const lines = [...existing, ...rows].map((row) => JSON.stringify(row));
    const { mkdir } = await import("node:fs/promises");
    await mkdir(join(root, ".inkos"), { recursive: true });
    await writeTextAtomic(path, lines.join("\n") + "\n");
  });
}
