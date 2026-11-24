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
/**
 * 版本以 (promptId, ts) 定位——同一 prompt 不得出现同 ts 两行（Rust
 * `unique_version_ts` 同款）。同一毫秒内的两次写入（并发保存 / 回滚后紧接
 * 保存）会让后一行被前一行遮蔽：`readPromptVersion` 只认一条、列表里同 ts 的
 * 两行「旧的排在前」。撞键时把新行的 ts 顺推 1ms 直到不撞（顺推后的顺序 =
 * 写入顺序）。只对 ISO 毫秒形态的 ts 顺推，其余原样写入。
 */
function uniqueVersionTs(
  existing: ReadonlyArray<PromptVersionRecord>,
  promptId: string,
  ts: string,
): string {
  const time = Date.parse(ts);
  // `Date.parse` 对 "1" 这类串也给日期——回写不等即不是 ISO 毫秒形态。
  if (!Number.isFinite(time) || new Date(time).toISOString() !== ts) return ts;
  const taken = new Set(
    existing.filter((record) => record.promptId === promptId).map((record) => record.ts),
  );
  let candidate = time;
  while (taken.has(new Date(candidate).toISOString())) candidate += 1;
  return new Date(candidate).toISOString();
}

export async function appendPromptVersion(
  root: string,
  promptId: string,
  action: PromptVersionRecord["action"],
  content: string,
  ts: string,
): Promise<void> {
  const path = versionsPath(root);
  await runSerially(path, async () => {
    const existing = await loadPromptVersions(root);
    // 1666 号：ts 在串行段内据最新账本定——撞键判定必须发生在读到最新账本之后。
    const row: PromptVersionRecord = { ts: uniqueVersionTs(existing, promptId, ts), promptId, action, content };
    const lines = [...existing, row].map((record) => JSON.stringify(record));
    await mkdir(dirname(path), { recursive: true });
    await writeTextAtomic(path, lines.join("\n") + "\n");
  });
}

/** 某 prompt 版本列表（ts 降序=最新在前；同 ts 的历史行按写入序倒排）。 */
export async function listPromptVersions(root: string, promptId: string): Promise<PromptVersionRecord[]> {
  const rows = (await loadPromptVersions(root))
    .map((record, index) => ({ record, index }))
    .filter(({ record }) => record.promptId === promptId);
  rows.sort((a, b) => (a.record.ts < b.record.ts ? 1 : a.record.ts > b.record.ts ? -1 : b.index - a.index));
  return rows.map(({ record }) => record);
}

/** 读指定版本（promptId+ts 定位；找不到 undefined；同 ts 的历史行取最后写入的）。 */
export async function readPromptVersion(
  root: string,
  promptId: string,
  ts: string,
): Promise<PromptVersionRecord | undefined> {
  return (await loadPromptVersions(root))
    .filter((record) => record.promptId === promptId && record.ts === ts)
    .at(-1);
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
