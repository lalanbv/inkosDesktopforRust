/**
 * R55/1661 号：用量账本存储（Rust 镜像 = engine-rs/src/utils/usage_ledger.rs；
 * 纯函数=usage-budget.ts）。路径 = `{root}/.inkos/usage-log.jsonl` append-only
 * （读旧+拼+原子写；坏行宽容；仅 usage 精确值元数据）。
 */
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { writeTextAtomic } from "./atomic-write.js";
import type { UsageLogRow } from "./usage-budget.js";

export function usageLedgerPath(root: string): string {
  return join(root, ".inkos", "usage-log.jsonl");
}

export async function loadUsageLedger(root: string): Promise<UsageLogRow[]> {
  let text: string;
  try {
    text = await readFile(usageLedgerPath(root), "utf-8");
  } catch {
    return [];
  }
  const rows: UsageLogRow[] = [];
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      rows.push(JSON.parse(trimmed) as UsageLogRow);
    } catch {
      // 坏行跳过（宽容读）。
    }
  }
  return rows;
}

export async function appendUsageRows(root: string, rows: ReadonlyArray<UsageLogRow>): Promise<void> {
  if (rows.length === 0) return;
  const existing = await loadUsageLedger(root);
  const lines = [...existing, ...rows].map((row) => JSON.stringify(row));
  const { mkdir } = await import("node:fs/promises");
  await mkdir(join(root, ".inkos"), { recursive: true });
  await writeTextAtomic(usageLedgerPath(root), lines.join("\n") + "\n");
}
