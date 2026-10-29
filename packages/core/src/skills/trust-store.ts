/**
 * 外部技能来源信任库（768 号，决策①A：外部源确认+项目默认信任）。
 *
 * 仅 INKOS_SKILL_DIRS 显式 external 来源需要确认；user/project 惯例目录
 * 默认信任（用户自维护文件，B/C 分支差异见施工预备 cbac45d4）。
 * 信任状态落 `{projectRoot}/.inkos/trusted-sources.json`（761 号 durable
 * 原子写）。路径语义：按 env 配置串归一（trim + 去尾分隔符）后原样比对——
 * 双端读同一 env 串，跨端一致；不做符号链接归一（跨端 /tmp 形态分歧，
 * 双端同规避免行为分叉）。
 */
import { dirname, join } from "node:path";
import { mkdir, readFile } from "node:fs/promises";
import { writeFileDurable } from "../utils/durable-fs.js";

const TRUST_FILE_RELATIVE = join(".inkos", "trusted-sources.json");

export interface TrustedSourceEntry {
  readonly path: string;
  readonly confirmedAt: string;
}

/** env 配置串归一：trim + 去尾分隔符（根路径 "/" 除外）。 */
export function normalizeTrustedSourcePath(path: string): string {
  const trimmed = path.trim();
  if (trimmed.length > 1) return trimmed.replace(/[\\/]+$/, "");
  return trimmed;
}

function parseEntries(raw: unknown): TrustedSourceEntry[] {
  if (!raw || typeof raw !== "object") return [];
  const entries = (raw as { entries?: unknown }).entries;
  if (!Array.isArray(entries)) return [];
  const out: TrustedSourceEntry[] = [];
  for (const entry of entries) {
    if (!entry || typeof entry !== "object") continue;
    const path = (entry as { path?: unknown }).path;
    const confirmedAt = (entry as { confirmedAt?: unknown }).confirmedAt;
    if (typeof path === "string" && typeof confirmedAt === "string") {
      out.push({ path, confirmedAt });
    }
  }
  return out;
}

export async function readTrustedSources(projectRoot: string): Promise<TrustedSourceEntry[]> {
  try {
    const raw = await readFile(join(projectRoot, TRUST_FILE_RELATIVE), "utf-8");
    return parseEntries(JSON.parse(raw));
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return [];
    throw error;
  }
}

export async function isTrustedSource(projectRoot: string, path: string): Promise<boolean> {
  const normalized = normalizeTrustedSourcePath(path);
  return (await readTrustedSources(projectRoot)).some((entry) => entry.path === normalized);
}

async function writeEntries(projectRoot: string, entries: TrustedSourceEntry[]): Promise<void> {
  const file = join(projectRoot, TRUST_FILE_RELATIVE);
  await mkdir(dirname(file), { recursive: true });
  await writeFileDurable(file, `${JSON.stringify({ version: 1, entries }, null, 2)}\n`);
}

/** 新增信任来源（幂等；已存在不刷新 confirmedAt）。返回写后全量条目。 */
export async function addTrustedSource(
  projectRoot: string,
  path: string,
): Promise<TrustedSourceEntry[]> {
  const normalized = normalizeTrustedSourcePath(path);
  if (!normalized) throw new Error("trusted source path is required.");
  const entries = await readTrustedSources(projectRoot);
  if (!entries.some((entry) => entry.path === normalized)) {
    entries.push({ path: normalized, confirmedAt: new Date().toISOString() });
    await writeEntries(projectRoot, entries);
  }
  return entries;
}

/** 移除信任来源（幂等）。返回写后全量条目。 */
export async function removeTrustedSource(
  projectRoot: string,
  path: string,
): Promise<TrustedSourceEntry[]> {
  const normalized = normalizeTrustedSourcePath(path);
  const entries = (await readTrustedSources(projectRoot)).filter(
    (entry) => entry.path !== normalized,
  );
  await writeEntries(projectRoot, entries);
  return entries;
}
