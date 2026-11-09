/**
 * 954 号：备份域快照回滚——列举 + 服务端回滚执行。
 *
 * 938-953 十六轮备份域收官后，951 的 pre-restore 快照回滚（backups/
 * pre-restore-*.tar 灌回 import）只在 API/文件系统层可用——UI 提示
 * 「快照在 backups/」但无列举无回滚入口，回滚保险对用户「看得见摸
 * 不着」。本轮补齐用户可执行闭环：
 *
 * - `GET /api/v1/backup/snapshots`：列举 `backups/pre-restore-*.tar`
 *   （形态白名单过滤，名倒序=时间倒序），返回 `{snapshots:[{name,size}]}`。
 * - `POST /api/v1/backup/rollback[?confirm=1]`：服务端读快照字节走与
 *   import 完全相同的执行体（安全三则+manifest 先验+两段式 preview/
 *   confirm）——快照回滚过与外部备份包完全相同的检查，非语义旁路；
 *   回滚执行时对被覆盖文件再留快照（import 不变式自动保持）。
 *
 * 名字形态校验（`pre-restore-<纯数字毫秒>.tar`）是回滚面唯一放行形态，
 * 路径穿越无从谈起；列举同样按形态过滤（tmp 残留 `.pre-restore-*.tmp`
 * 天然不入列）。行格式双端（Rust `backup_routes.rs` 同语义）逐字对齐。
 */

import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { appendTarEntry } from "../lib/tar-write.js";

/** import 两段式共享执行体的结果：400 守卫，或 200 线格式（preview / ok+restored+snapshot）。 */
export type BackupImportOutcome =
  | { status: 400; error: string }
  | {
    status: 200;
    body: { preview: { fileCount: number; overwriting: string[]; newFiles: number } }
      | { ok: true; restored: number; snapshot: boolean };
  };

/** 快照文件名形态校验（`pre-restore-<纯数字毫秒>.tar`）。 */
export function parseSnapshotName(name: string): number | null {
  const match = name.match(/^pre-restore-(\d+)\.tar$/);
  if (!match) return null;
  const millis = Number(match[1]);
  return Number.isSafeInteger(millis) ? millis : null;
}

/** import 处理器核心（443/444 号两段式语义）；import 与 rollback 共用。 */
export async function runBackupImport(root: string, bytes: Uint8Array, confirm: boolean): Promise<BackupImportOutcome> {
  const { untar } = await import("../lib/tar-read.js");
  let entries: ReadonlyArray<{ name: string; bytes: Uint8Array }>;
  let rejected: ReadonlyArray<string>;
  try {
    const result = untar(bytes);
    entries = result.entries;
    rejected = result.rejected;
  } catch (error) {
    return { status: 400, error: String(error) };
  }
  if (rejected.length > 0) {
    return { status: 400, error: `unsafe entries rejected: ${rejected.join(", ")}` };
  }
  // 444 号：manifest 查找与恢复路径按「能工作的契约」对齐 Rust 移植
  // （backup_routes.rs）——导出以 `inkos-backup/` 为包根前缀，导入须剥前缀
  // 落回项目根；此前裸名精确查找 + 原样写回 = 导出→导入端到端断裂。
  const BACKUP_PACKAGE_ROOT = "inkos-backup";
  const manifestName = `${BACKUP_PACKAGE_ROOT}/backup-manifest.json`;
  const restoreName = (name: string): string =>
    name.startsWith(`${BACKUP_PACKAGE_ROOT}/`) ? name.slice(BACKUP_PACKAGE_ROOT.length + 1) : name;
  const manifestEntry = entries.find((entry) => entry.name === manifestName || entry.name === "backup-manifest.json");
  if (!manifestEntry) return { status: 400, error: "backup-manifest.json missing" };
  try {
    const manifest = JSON.parse(new TextDecoder().decode(manifestEntry.bytes));
    if (manifest.version !== 1 || typeof manifest.generator !== "string"
      || !manifest.generator.startsWith("inkos-desktop")) {
      return { status: 400, error: "unsupported backup generator/version" };
    }
  } catch {
    return { status: 400, error: "backup-manifest.json unreadable" };
  }

  // 将覆盖项 = root 下已存在的同名文件（剥包根前缀后的真实路径）。
  const overwriting: string[] = [];
  for (const entry of entries) {
    const name = restoreName(entry.name);
    if (await stat(join(root, name)).then(() => true).catch(() => false)) {
      overwriting.push(name);
    }
  }
  if (!confirm) {
    return {
      status: 200,
      body: { preview: { fileCount: entries.length, overwriting, newFiles: entries.length - overwriting.length } },
    };
  }

  // 恢复前自动快照：将覆盖的现有文件打包留存（回滚保险）。
  if (overwriting.length > 0) {
    const snapshotFiles: Array<{ name: string; bytes: Uint8Array }> = [];
    for (const name of overwriting) {
      const data = await readFile(join(root, name)).catch(() => null);
      if (data) snapshotFiles.push({ name, bytes: new Uint8Array(data) });
    }
    // 951 号：快照附带 manifest（裸名形态，双端 import 管道均兼容裸名查
    // 找与写回跳过）——pre-restore 快照可直接经 backup/import?confirm=1
    // 回灌=回滚保险从「留档」到「可执行」闭环。部分回滚边界：恢复引入
    // 的新文件不在快照中（保守不引入删除语义，备案）。
    snapshotFiles.push({
      name: "backup-manifest.json",
      bytes: new TextEncoder().encode(JSON.stringify({
        version: 1,
        generator: "inkos-desktop",
        kind: "pre-restore-snapshot",
        overwriting,
      })),
    });
    const backupsDir = join(root, "backups");
    await mkdir(backupsDir, { recursive: true });
    const chunks: Buffer[] = [];
    // 953 号审查修复：快照条目走与 export 同一三通道（appendTarEntry）——
    // 此前手写头对 >100 字节名经 Buffer.copy 静默截断，长路径包恢复时
    // 快照条目名截断 → 回滚写错位置（950 起 export 可产长路径包，患激活；
    // Rust 侧 tar::Builder 自动 GNU 'L' 无此患）。manifest 条目（20 字节）
    // 走单头分支，形态等价。
    for (const file of snapshotFiles) {
      appendTarEntry(chunks, file.name, Buffer.from(file.bytes));
    }
    chunks.push(Buffer.alloc(1024));
    const snapshotName = `pre-restore-${Date.now()}.tar`;
    await writeFile(join(backupsDir, snapshotName), Buffer.concat(chunks), "utf-8");
  }

  // 写回（快照已兜底）。manifest 是元数据不落盘；restored 计实际写回文件数
  // （与 Rust backup_routes.rs 同契约）。
  let restored = 0;
  for (const entry of entries) {
    if (entry.name === manifestName || entry.name === "backup-manifest.json") continue;
    const target = join(root, restoreName(entry.name));
    await mkdir(join(target, ".."), { recursive: true });
    await writeFile(target, entry.bytes);
    restored += 1;
  }
  return { status: 200, body: { ok: true, restored, snapshot: overwriting.length > 0 } };
}

/** 快照回滚：校验形态 → 读快照字节 → 走 import 同一执行体。 */
export async function runBackupRollback(root: string, name: string, confirm: boolean): Promise<BackupImportOutcome> {
  if (parseSnapshotName(name) === null) return { status: 400, error: "invalid snapshot name" };
  const bytes = await readFile(join(root, "backups", name)).catch(() => null);
  if (!bytes) return { status: 400, error: "snapshot not found" };
  return runBackupImport(root, bytes, confirm);
}

/** 列举 `backups/pre-restore-*.tar`（形态过滤+名倒序=时间倒序，millis 等长名字典序=数值序）。 */
export async function listBackupSnapshots(root: string): Promise<Array<{ name: string; size: number }>> {
  const items: Array<{ name: string; size: number }> = [];
  const dir = join(root, "backups");
  const children = await readdir(dir, { withFileTypes: true }).catch(() => null);
  for (const child of children ?? []) {
    if (!child.isFile() || parseSnapshotName(child.name) === null) continue;
    const info = await stat(join(dir, child.name)).catch(() => null);
    if (info) items.push({ name: child.name, size: info.size });
  }
  items.sort((a, b) => (a.name < b.name ? 1 : a.name > b.name ? -1 : 0));
  return items;
}
