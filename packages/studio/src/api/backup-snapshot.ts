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
 *
 * 955 号审查修复：快照唯一名分配（同毫秒二次恢复不再覆盖前一份快照）+
 * per-root 恢复串行锁（并发确认恢复不交错）+ 名规范形态纯串级判定（双端
 * 共享向量表锁定）+ 列举数值序 + 回滚面=列举面（仅常规文件）+ 恢复写回与
 * 快照落盘走 writeTextAtomic（镜像 Rust 779/782 号原子写）。
 */

import { lstat, mkdir, readFile, readdir, stat } from "node:fs/promises";
import { join } from "node:path";
import { writeTextAtomic } from "@actalk/inkos-core";
import { appendTarEntry } from "../lib/tar-write.js";

/** import 两段式共享执行体的结果：400 守卫，或 200 线格式（preview / ok+restored+snapshot）。 */
export type BackupImportOutcome =
  | { status: 400; error: string }
  | {
    status: 200;
    body: { preview: { fileCount: number; overwriting: string[]; newFiles: number } }
      | { ok: true; restored: number; snapshot: boolean };
  };

/**
 * 955 号：快照名规范形态（`pre-restore-<D>.tar`，D = `0` 或无前导零的 1-16 位
 * ASCII 十进制数字）——回滚面唯一放行形态。双端纯串级判定逐字对齐（Rust
 * `is_snapshot_name`；向量表 `__tests__/golden/backup-snapshot-name-vectors.json`
 * 双端断言同文件）。954 版按数值解析判定（此处安全整数 vs Rust u64）在 16-19
 * 位名上双端分叉（此处判非法名、Rust 判缺失且列举可见），本形态不依赖任何
 * 数值语义；规范形态=名与毫秒一一对应。
 */
const SNAPSHOT_NAME_PATTERN = /^pre-restore-(?:0|[1-9]\d{0,15})\.tar$/;

export function isSnapshotName(name: string): boolean {
  return SNAPSHOT_NAME_PATTERN.test(name);
}

/**
 * 955 号：快照名数值序比较——规范形态同前后缀，(长度, 字典序) 即毫秒数值序
 * （954 版纯字典序在位数不同的名上错序）。Rust `compare_snapshot_names` 同算法。
 */
export function compareSnapshotNames(a: string, b: string): number {
  return a.length - b.length || (a < b ? -1 : a > b ? 1 : 0);
}

/** 955 号：快照名探测上限（同毫秒连续占用的病态防护；正常 1 次命中）。 */
const SNAPSHOT_NAME_PROBE_LIMIT = 1000;

/**
 * 955 号：快照唯一名分配（Rust `allocate_snapshot_millis` 对偶）——自 nowMillis
 * 起探测首个未占用毫秒名（任意类型占用均跳过：同名目录也会令 rename 失败）。
 * 调用方须持恢复串行锁（探测→就位之间无进程内竞争者）；非 ENOENT 的探测错误
 * 直接上抛，不把不可访问误判为占用而空转。
 */
export async function allocateSnapshotName(backupsDir: string, nowMillis: number): Promise<string> {
  for (let offset = 0; offset < SNAPSHOT_NAME_PROBE_LIMIT; offset += 1) {
    const name = `pre-restore-${nowMillis + offset}.tar`;
    try {
      await lstat(join(backupsDir, name));
    } catch (error) {
      if ((error as { code?: unknown } | null)?.code === "ENOENT") return name;
      throw error;
    }
  }
  throw new Error("no free snapshot name");
}

/**
 * 955 号：per-root 恢复串行锁（Rust `restore_lock` 对偶）——确认段（覆盖面盘点
 * →快照→写回）整体互斥：954 前两次并发确认恢复在 await 点交错写回成混合态，
 * 且同毫秒共用快照名互相覆盖。尾 Promise 链排队；链上 Promise 恒 resolve（任务
 * 失败不毒化队列）；队空即删条目（Map 不随恢复次数增长）。
 */
const restoreTails = new Map<string, Promise<void>>();

async function withRestoreLock<T>(root: string, task: () => Promise<T>): Promise<T> {
  const previous = restoreTails.get(root) ?? Promise.resolve();
  let release: () => void = () => undefined;
  const current = new Promise<void>((resolve) => {
    release = resolve;
  });
  const tail = previous.then(() => current);
  restoreTails.set(root, tail);
  try {
    await previous;
    return await task();
  } finally {
    release();
    if (restoreTails.get(root) === tail) restoreTails.delete(root);
  }
}

// 444 号：manifest 查找与恢复路径按「能工作的契约」对齐 Rust 移植
// （backup_routes.rs）——导出以 `inkos-backup/` 为包根前缀，导入须剥前缀
// 落回项目根；此前裸名精确查找 + 原样写回 = 导出→导入端到端断裂。
const BACKUP_PACKAGE_ROOT = "inkos-backup";
const BARE_MANIFEST_NAME = "backup-manifest.json";
const PACKAGED_MANIFEST_NAME = `${BACKUP_PACKAGE_ROOT}/${BARE_MANIFEST_NAME}`;

function isManifestEntry(name: string): boolean {
  return name === PACKAGED_MANIFEST_NAME || name === BARE_MANIFEST_NAME;
}

function restoreName(name: string): string {
  return name.startsWith(`${BACKUP_PACKAGE_ROOT}/`) ? name.slice(BACKUP_PACKAGE_ROOT.length + 1) : name;
}

/**
 * 955 号：数据条目数（manifest 元数据除外）——预览计数面=写回面=快照面同一
 * 集合。954 前预览把 manifest 计入 fileCount 与 newFiles：真实导出包预览显示
 * 「新增 1」而实际不写任何新文件，且预览总数比确认后的恢复数多 1（真引擎活体
 * 实测：6 条目包预览 fileCount 6 / newFiles 1，确认 restored 5）。
 */
function countDataEntries(entries: ReadonlyArray<{ name: string }>): number {
  let count = 0;
  for (const entry of entries) {
    if (!isManifestEntry(entry.name)) count += 1;
  }
  return count;
}

/**
 * 将覆盖项 = root 下已存在的同名文件（剥包根前缀后的真实路径）。955 号：manifest
 * 条目不参与——写回本就跳过它（根目录同名文件不会被覆盖），954 前却计入覆盖面：
 * 根目录存在 backup-manifest.json 时预览误报「将覆盖」，且该文件以裸名入快照，
 * 与快照自身 manifest 同名并存——回滚时 manifest 查找先命中用户文件，先验失败
 * =快照不可回滚。
 */
async function collectOverwriting(root: string, entries: ReadonlyArray<{ name: string }>): Promise<string[]> {
  const overwriting: string[] = [];
  for (const entry of entries) {
    if (isManifestEntry(entry.name)) continue;
    const name = restoreName(entry.name);
    if (await stat(join(root, name)).then(() => true).catch(() => false)) {
      overwriting.push(name);
    }
  }
  return overwriting;
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
  // 955 号：manifest 选取与 Rust 对齐——包根前缀名优先、裸名兜底（954 前此处
  // 取首个任一名条目，两名并存的包双端可能选中不同 manifest）。
  const manifestEntry = entries.find((entry) => entry.name === PACKAGED_MANIFEST_NAME)
    ?? entries.find((entry) => entry.name === BARE_MANIFEST_NAME);
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

  if (!confirm) {
    const overwriting = await collectOverwriting(root, entries);
    const fileCount = countDataEntries(entries);
    return {
      status: 200,
      body: { preview: { fileCount, overwriting, newFiles: fileCount - overwriting.length } },
    };
  }

  // 955 号：确认段持 per-root 恢复串行锁——覆盖面盘点须在锁内（锁外盘点与
  // 写回之间他者恢复新建的文件会漏出快照=回滚面缺口）；预览只读不持锁。
  return withRestoreLock<BackupImportOutcome>(root, async () => {
    const overwriting = await collectOverwriting(root, entries);
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
        name: BARE_MANIFEST_NAME,
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
      // 955 号：唯一名分配（持锁探测——954 前同毫秒二次恢复覆盖前一份快照，
      // 回滚保险静默丢失）+ 原子落盘（tmp+fsync+rename，镜像 Rust 782 号：
      // 撕裂快照=保险失效）。时钟早于纪元回退 0（Rust current_millis 同语义，
      // 负数名不合规范形态=快照不可列举不可回滚）。
      const snapshotName = await allocateSnapshotName(backupsDir, Math.max(0, Date.now()));
      await writeTextAtomic(join(backupsDir, snapshotName), Buffer.concat(chunks));
    }

    // 写回（快照已兜底）。manifest 是元数据不落盘；restored 计实际写回文件数
    // （与 Rust backup_routes.rs 同契约）。
    let restored = 0;
    for (const entry of entries) {
      if (isManifestEntry(entry.name)) continue;
      const target = join(root, restoreName(entry.name));
      await mkdir(join(target, ".."), { recursive: true });
      // 955 号：恢复是覆盖写——撕裂=毁掉现有文件换成残缺恢复件，原子替换
      // （镜像 Rust 779 号 write_file_atomic_sync；954 前 TS 腿直写 writeFile）。
      await writeTextAtomic(target, entry.bytes);
      restored += 1;
    }
    return { status: 200, body: { ok: true, restored, snapshot: overwriting.length > 0 } };
  });
}

/**
 * 快照回滚：校验形态 → 读快照字节 → 走 import 同一执行体。955 号：回滚面=
 * 列举面——仅放行常规文件（符号链接/目录按缺失处理，与 listBackupSnapshots
 * 同判据：954 版 readFile 跟随链接，列举不可见的链接名可被直接回滚）。
 */
export async function runBackupRollback(root: string, name: string, confirm: boolean): Promise<BackupImportOutcome> {
  if (!isSnapshotName(name)) return { status: 400, error: "invalid snapshot name" };
  const path = join(root, "backups", name);
  const info = await lstat(path).catch(() => null);
  if (!info?.isFile()) return { status: 400, error: "snapshot not found" };
  const bytes = await readFile(path).catch(() => null);
  if (!bytes) return { status: 400, error: "snapshot not found" };
  return runBackupImport(root, bytes, confirm);
}

/**
 * 列举 `backups/pre-restore-*.tar`（规范形态+常规文件过滤，数值倒序=时间倒序）。
 * 目录缺失=空列表；tmp 残留、同名目录、符号链接天然被过滤（lstat 不跟随链接，
 * 与 Rust DirEntry::metadata 同判据）。
 */
export async function listBackupSnapshots(root: string): Promise<Array<{ name: string; size: number }>> {
  const items: Array<{ name: string; size: number }> = [];
  const dir = join(root, "backups");
  const children = await readdir(dir).catch(() => null);
  for (const name of children ?? []) {
    if (!isSnapshotName(name)) continue;
    const info = await lstat(join(dir, name)).catch(() => null);
    if (info?.isFile()) items.push({ name, size: info.size });
  }
  items.sort((a, b) => compareSnapshotNames(b.name, a.name));
  return items;
}
