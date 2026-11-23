/**
 * R54/1662 号：prompt 覆盖与版本账本存储（Rust 镜像 =
 * engine-rs/src/prompts/prompt_pack.rs 版本账本段；Node 回退腿端点消费）。
 * - 覆盖路径 = `{root}/prompt/<dirs>/<name>.md`（点分 id 映射，Rust
 *   prompt_override_path 同款）。
 * - 版本账本 = `{root}/prompt/.versions.jsonl` append-only（行
 *   {ts, promptId, action: put|delete|restore, content}；读旧+拼+原子写；
 *   坏行宽容）。
 */
import { mkdir, readFile, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { runSerially } from "./append-queue.js";
import { writeTextAtomic } from "./atomic-write.js";

export interface PromptVersionRecord {
  readonly ts: string;
  readonly promptId: string;
  readonly action: "put" | "delete" | "restore";
  readonly content: string;
}

/** 点分 id → 覆盖文件路径（trim+lower 归一；Rust prompt_override_path 镜像）。 */
export function promptOverridePath(root: string, promptId: string): string {
  const normalized = promptId.trim().toLowerCase();
  const parts = normalized.split(".");
  if (parts.length < 2) {
    return join(root, "prompt", `${normalized}.md`);
  }
  const last = parts[parts.length - 1]!;
  const dirs = parts.slice(0, -1);
  return join(root, "prompt", ...dirs, `${last}.md`);
}

function versionsPath(root: string): string {
  return join(root, "prompt", ".versions.jsonl");
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

/** 版本行形状守卫（Rust serde 同款：四个字段齐全且为串才算一行）。 */
function isPromptVersionRecord(value: unknown): value is PromptVersionRecord {
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return typeof record.ts === "string" && typeof record.promptId === "string"
    && typeof record.action === "string" && typeof record.content === "string";
}

export async function loadPromptVersions(root: string): Promise<PromptVersionRecord[]> {
  const text = await readLedgerText(versionsPath(root));
  if (text === undefined) return [];
  const rows: PromptVersionRecord[] = [];
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const parsed: unknown = JSON.parse(trimmed);
      if (isPromptVersionRecord(parsed)) rows.push(parsed);
    } catch {
      // 坏行跳过（宽容读）。
    }
  }
  return rows;
}

/**
 * 1666 号两点加固（usage 账本同款，锁在 `ledger-append.flow.test`）：
 * 1. 追加按账本路径进程内串行化——并发追加此前会丢行（20 路并发实测只剩
 *    1 行）；丢掉的是版本行，对应版本从此无法回滚。
 * 2. 读错误 ≠ 空账本——读不出来时上抛、追加中止，不用「只有新行」的内容
 *    覆盖整本历史。
 */
export async function appendPromptVersion(
  root: string,
  promptId: string,
  action: PromptVersionRecord["action"],
  content: string,
  ts: string,
): Promise<void> {
  const row: PromptVersionRecord = { ts, promptId, action, content };
  const path = versionsPath(root);
  await runSerially(path, async () => {
    const existing = await loadPromptVersions(root);
    const lines = [...existing, row].map((record) => JSON.stringify(record));
    await mkdir(dirname(path), { recursive: true });
    await writeTextAtomic(path, lines.join("\n") + "\n");
  });
}

/** 某 prompt 版本列表（ts 降序=最新在前）。 */
export async function listPromptVersions(root: string, promptId: string): Promise<PromptVersionRecord[]> {
  const rows = (await loadPromptVersions(root)).filter((record) => record.promptId === promptId);
  return rows.sort((a, b) => (a.ts < b.ts ? 1 : a.ts > b.ts ? -1 : 0));
}

/** 读指定版本（promptId+ts 定位；找不到 undefined）。 */
export async function readPromptVersion(
  root: string,
  promptId: string,
  ts: string,
): Promise<PromptVersionRecord | undefined> {
  return (await loadPromptVersions(root)).find(
    (record) => record.promptId === promptId && record.ts === ts,
  );
}

/** 覆盖写（原子）——restore 复写与 PUT 共用。 */
export async function writePromptOverride(root: string, promptId: string, content: string): Promise<void> {
  const path = promptOverridePath(root, promptId);
  await mkdir(dirname(path), { recursive: true });
  await writeTextAtomic(path, content);
}

/** 移除覆盖（ENOENT 静默——回 builtin）。 */
export async function removePromptOverride(root: string, promptId: string): Promise<void> {
  try {
    await rm(promptOverridePath(root, promptId));
  } catch {
    // ENOENT 静默（Rust 同款）。
  }
}
