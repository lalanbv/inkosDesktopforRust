/**
 * R51/1653 号：日产出账本存储（append-only jsonl，Rust 镜像 =
 * engine-rs/src/utils/writing_log.rs）。
 *
 * 路径 = `{bookDir}/.inkos/writing-log.jsonl`（书级 .inkos 子目录——双根
 * 对称：TS bookDir=works/{id}/source、Rust=books/{id}）；行 = `WritingLogRow`
 * 四键（ts=UTC ISO）。追加语义 = 读旧 + 拼接 + 原子写（append-only 语义、
 * 原子性两全——章保存频率下 O(n) 重写可接受）；读取宽容（坏行跳过）。
 * 仅元数据（无正文=隐私红线，施工图 §2.2）。
 */
import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { writeTextAtomic } from "./atomic-write.js";
import type { WritingLogRow } from "./writing-goal.js";

export function writingLogPath(bookDir: string): string {
  return join(bookDir, ".inkos", "writing-log.jsonl");
}

/** 读账本（无文件=空账本；坏行跳过——jsonl 宽容读）。 */
export async function loadWritingLog(bookDir: string): Promise<WritingLogRow[]> {
  let text: string;
  try {
    text = await readFile(writingLogPath(bookDir), "utf-8");
  } catch {
    return [];
  }
  const rows: WritingLogRow[] = [];
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      rows.push(JSON.parse(trimmed) as WritingLogRow);
    } catch {
      // 坏行跳过（宽容读）。
    }
  }
  return rows;
}

/** 追加账本行（读旧+拼接+原子写；空批次 no-op）。 */
export async function appendWritingLog(
  bookDir: string,
  rows: ReadonlyArray<WritingLogRow>,
): Promise<void> {
  if (rows.length === 0) return;
  const existing = await loadWritingLog(bookDir);
  const lines = [...existing, ...rows].map((row) => JSON.stringify(row));
  await mkdir(join(bookDir, ".inkos"), { recursive: true });
  await writeTextAtomic(writingLogPath(bookDir), lines.join("\n") + "\n");
}
