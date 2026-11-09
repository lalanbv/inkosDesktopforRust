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
 *
 * 956 号：完全回滚语义——快照 manifest 记撤销决策面（overwriting=被覆盖/
 * introduced=恢复新增/removing=本次将删的已留档文件），rollback 端点
 * deleteIntroduced=1 时写回后一并删除恢复期间新增文件（每份待删文件删除前
 * 留档=完全回滚自身也有保险）；import 端点（Ignore 模式）契约面不变。
 */

import { lstat, mkdir, readFile, readdir, stat, unlink } from "node:fs/promises";
import { join } from "node:path";
import { writeTextAtomic } from "@actalk/inkos-core";
import { appendTarEntry } from "../lib/tar-write.js";

/** import 两段式共享执行体的结果：400 守卫，或 200 线格式（preview / ok+restored+snapshot）。 */
export type BackupImportOutcome =
  | { status: 400; error: string }
  | {
    status: 200;
    body: { preview: { fileCount: number; overwriting: string[]; newFiles: number; introduced?: string[] } }
      | { ok: true; restored: number; snapshot: boolean; removed?: number; deleteFailed?: Array<{ path: string; error: string }> };
  };

/**
 * 956 号：introduced 清单处理模式（Rust `IntroducedMode` 同语义）。import
 * 端点恒 "ignore"（外部包恢复永不透传不删除——契约面不变）；rollback 端点
 * "keep"（保守回滚：预览透传清单，确认不删）或 "delete"（完全回滚
 * deleteIntroduced=1：预览与确认同一先验——预览预言确认结果，确认写回后
 * 删除）。
 */
export type IntroducedMode = "ignore" | "keep" | "delete";

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
 * 将覆盖项/恢复新增项 = root 下同名文件的存在性（剥包根前缀后的真实路径）。
 * 955 号：manifest 条目不参与——写回本就跳过它（根目录同名文件不会被覆盖），
 * 954 前却计入覆盖面：根目录存在 backup-manifest.json 时预览误报「将覆盖」，
 * 且该文件以裸名入快照，与快照自身 manifest 同名并存——回滚时 manifest 查找
 * 先命中用户文件，先验失败=快照不可回滚。956 号：单次遍历双清单（覆盖面=存在，
 * introduced=不存在）——同一时刻同一存在性视图（Rust 同算法）。
 */
async function classifyTargets(
  root: string,
  entries: ReadonlyArray<{ name: string }>,
): Promise<{ overwriting: string[]; introduced: string[] }> {
  const overwriting: string[] = [];
  const introduced: string[] = [];
  for (const entry of entries) {
    if (isManifestEntry(entry.name)) continue;
    const name = restoreName(entry.name);
    if (await stat(join(root, name)).then(() => true).catch(() => false)) {
      overwriting.push(name);
    } else {
      introduced.push(name);
    }
  }
  return { overwriting, introduced };
}

/**
 * 956 号：回滚删除面路径安全判定（Rust `is_safe_relative_path` 同判据，纯串级
 * 跨平台一致；向量表 `__tests__/golden/backup-introduced-path-vectors.json` 双端
 * 断言同文件）。introduced 清单来自 manifest JSON 自由字符串，不经 tar 条目安全
 * 三则——不安全路径若放行可删 root 外任意文件。
 */
export function isSafeRelativePath(path: string): boolean {
  if (path === "" || path.includes("\0") || path.includes("\\") || path.startsWith("/")) return false;
  // 盘符形态（`C:/x` 绝对与 `C:x` 驱动器相对）一并拒绝。
  if (/^[A-Za-z]:/.test(path)) return false;
  // 空段（`a//b`）、当前段（`.`）、穿越段（`..`）一律拒绝——保守整拒不归一化。
  return !path.split("/").some((segment) => segment === "" || segment === "." || segment === "..");
}

/**
 * 956 号：快照 manifest 的 introduced 清单可信度三分（Rust `manifest_introduced_list`
 * 同语义）。kind 限定 pre-restore-snapshot——export 包即便被手工塞入该字段也不透传
 * 不执行。absent=无字段或 kind 不符（export 包/955- 旧快照）；malformed=字段存在但
 * 非纯 string 数组；present=可用清单。
 */
type IntroducedList = { kind: "absent" } | { kind: "malformed" } | { kind: "present"; paths: string[] };

function manifestIntroducedList(manifest: Record<string, unknown>): IntroducedList {
  if (manifest.kind !== "pre-restore-snapshot") return { kind: "absent" };
  const items = manifest.introduced;
  if (!Array.isArray(items)) return { kind: "absent" };
  if (!items.every((item): item is string => typeof item === "string")) return { kind: "malformed" };
  return { kind: "present", paths: items };
}

/**
 * 956 号：delete 模式先验（preview 与 confirm 同一段——预览预言确认结果）：
 * absent/malformed/含不安全路径一律拒绝。Rust confirm 段同判据逐字对齐。
 */
function deleteListOrError(
  manifest: Record<string, unknown>,
): { error: string } | { paths: string[] } {
  const list = manifestIntroducedList(manifest);
  if (list.kind === "absent") return { error: "snapshot manifest lacks introduced list" };
  if (list.kind === "malformed") return { error: "unsafe introduced path" };
  const bad = list.paths.find((path) => !isSafeRelativePath(path));
  if (bad !== undefined) return { error: `unsafe introduced path: ${bad}` };
  return { paths: list.paths };
}

/**
 * import 处理器核心（443/444 号两段式语义）；import 与 rollback 共用。
 * 956 号：`mode` 见 {@link IntroducedMode}。
 */
export async function runBackupImport(
  root: string,
  bytes: Uint8Array,
  confirm: boolean,
  mode: IntroducedMode = "ignore",
): Promise<BackupImportOutcome> {
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
  let manifest: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(new TextDecoder().decode(manifestEntry.bytes));
    if (typeof parsed !== "object" || parsed === null) {
      return { status: 400, error: "backup-manifest.json unreadable" };
    }
    manifest = parsed as Record<string, unknown>;
    if (manifest.version !== 1 || typeof manifest.generator !== "string"
      || !(manifest.generator as string).startsWith("inkos-desktop")) {
      return { status: 400, error: "unsupported backup generator/version" };
    }
  } catch {
    return { status: 400, error: "backup-manifest.json unreadable" };
  }

  if (!confirm) {
    // 955 号：确认段持 per-root 恢复串行锁——覆盖面盘点须在锁内（锁外盘点与
    // 写回之间他者恢复新建的文件会漏出快照=回滚面缺口）；预览只读不持锁。
    const { overwriting, newFiles, fileCount } = await (async () => {
      const targets = await classifyTargets(root, entries);
      const fileCount = countDataEntries(entries);
      return { overwriting: targets.overwriting, newFiles: fileCount - targets.overwriting.length, fileCount };
    })();
    // 956 号：回滚预览透传 introduced 清单（快照记录的恢复期间新增文件，供
    // UI 呈现「可一并删除」决策面）。仅回滚端点且清单可用且全路径安全才透传
    // ——不安全路径绝不入 UI（用户看不到删除区就不会选择删除；即便绕过 UI
    // 强发 confirm，delete 模式先验仍 400）。delete 模式 preview=confirm 同一
    // 先验（预览预言确认结果）。
    const preview: { fileCount: number; overwriting: string[]; newFiles: number; introduced?: string[] } = {
      fileCount, overwriting, newFiles,
    };
    if (mode !== "ignore") {
      if (mode === "delete") {
        const verified = deleteListOrError(manifest);
        if ("error" in verified) return { status: 400, error: verified.error };
        preview.introduced = verified.paths;
      } else {
        const list = manifestIntroducedList(manifest);
        if (list.kind === "present" && list.paths.every((path) => isSafeRelativePath(path))) {
          preview.introduced = list.paths;
        }
      }
    }
    return { status: 200, body: { preview } };
  }

  // 955 号：确认段持 per-root 恢复串行锁——覆盖面盘点须在锁内（锁外盘点与
  // 写回之间他者恢复新建的文件会漏出快照=回滚面缺口）；预览只读不持锁。
  return withRestoreLock<BackupImportOutcome>(root, async () => {
    const { overwriting, introduced } = await classifyTargets(root, entries);

    // 956 号：完全回滚删除面先验——必须在任何盘面变更前（写回一旦发生不可
    // 回退，先验失败须整体 400 无副作用）。仅 delete 模式进入；absent（export
    // 包/955- 旧快照）、malformed、含不安全路径的清单一律拒绝——不安全路径若
    // 放行可删 root 外任意文件。重复项去重（篡改清单的重复名不得重复留档/计数）。
    let deleteList: string[] | null = null;
    if (mode === "delete") {
      const verified = deleteListOrError(manifest);
      if ("error" in verified) return { status: 400, error: verified.error };
      // 去重（篡改清单的重复名不得重复留档/计数）。
      deleteList = [...new Set(verified.paths)];
    }

    // 956 号：完全回滚时待删文件须先留档（每个破坏性操作都有保险，951 号不
    // 变式——删除前内容入本次快照，回滚该快照即复原=完全回滚自身也有保险）。
    // 仅常规文件可删（lstat 不跟随，与 955 回滚面同判据：目录/链接/特殊文件
    // 不删且报 deleteFailed）；已不存在=终态已达成直接跳过；其余读取失败不留
    // 档即不删，同样报 deleteFailed（用户要求删除而未删，不得静默）。
    const deletable: string[] = [];
    const deletableFiles: Uint8Array[] = [];
    const deleteFailed: Array<{ path: string; error: string }> = [];
    for (const name of deleteList ?? []) {
      const target = join(root, name);
      const info = await lstat(target).catch(() => null);
      if (!info) continue; // 已不存在=终态一致
      if (!info.isFile()) {
        deleteFailed.push({ path: name, error: "not a regular file" });
        continue;
      }
      const data = await readFile(target).catch((error: unknown) => {
        deleteFailed.push({ path: name, error: String(error) });
        return null;
      });
      if (data) {
        deletable.push(name);
        deletableFiles.push(new Uint8Array(data));
      }
    }

    // 恢复前自动快照（回滚保险）。956 号：本次操作改变盘面即留快照——覆盖/
    // 新建/删除任一非空。此前仅 overwriting 非空才留：纯新增恢复无任何回滚
    // 保险；且纯新增恢复产的快照（无数据条目）被完全回滚时若不计 deletable，
    // 待删文件将被无保险删除。
    const hasSnapshot = overwriting.length > 0 || introduced.length > 0 || deletable.length > 0;
    if (hasSnapshot) {
      const snapshotFiles: Array<{ name: string; bytes: Uint8Array }> = [];
      for (const name of overwriting) {
        const data = await readFile(join(root, name)).catch(() => null);
        if (data) snapshotFiles.push({ name, bytes: new Uint8Array(data) });
      }
      // 956 号：完全回滚（delete 模式）时待删文件将真实删除——删除前先入快
      // 照（内容=删除前时刻；回滚本保险快照即可恢复它们=完全回滚自身也有保
      // 险）。条目在 overwriting 之后。
      for (const [index, name] of deletable.entries()) {
        snapshotFiles.push({ name, bytes: deletableFiles[index]! });
      }
      // 951 号：快照附带 manifest（裸名形态，双端 import 管道均兼容裸名查
      // 找与写回跳过）——pre-restore 快照可直接经 backup/import?confirm=1
      // 回灌=回滚保险从「留档」到「可执行」闭环。956 号：manifest 补撤销决
      // 策面（三数组恒在，形状统一）——撤销本次操作=写回快照数据条目（
      // overwriting 与 removing 的删除前内容）+ 可选删除 introduced（本次操
      // 作新建的文件）=精确逆运算。removing 仅 delete 模式非空。
      snapshotFiles.push({
        name: BARE_MANIFEST_NAME,
        bytes: new TextEncoder().encode(JSON.stringify({
          version: 1,
          generator: "inkos-desktop",
          kind: "pre-restore-snapshot",
          overwriting,
          introduced,
          removing: deletable,
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
    // 956 号：完全回滚删除执行（写回全部完成后）——名单=已留档的 deletable
    // （不留档不删）。unlink 前被外部删除=终态一致计入；其他失败收集
    // deleteFailed 且保持 200——写回已成功，谎报 500 会诱导用户重试（重复快
    // 照+写回）。空目录不删（只删文件——不可能误删用户目录内容）。
    let removed = 0;
    for (const name of deletable) {
      try {
        await unlink(join(root, name));
        removed += 1;
      } catch (error) {
        if ((error as { code?: unknown } | null)?.code === "ENOENT") {
          removed += 1;
        } else {
          deleteFailed.push({ path: name, error: String(error) });
        }
      }
    }
    const body: Extract<BackupImportOutcome, { status: 200 }>["body"] = {
      ok: true, restored, snapshot: hasSnapshot,
    };
    if (deleteList !== null) {
      (body as { removed?: number }).removed = removed;
      if (deleteFailed.length > 0) {
        (body as { deleteFailed?: Array<{ path: string; error: string }> }).deleteFailed = deleteFailed;
      }
    }
    return { status: 200, body };
  });
}

/**
 * 快照回滚：校验形态 → 读快照字节 → 走 import 同一执行体。955 号：回滚面=
 * 列举面——仅放行常规文件（符号链接/目录按缺失处理，与 listBackupSnapshots
 * 同判据：954 版 readFile 跟随链接，列举不可见的链接名可被直接回滚）。
 * 956 号：`deleteIntroduced` 透传删除语义（rollback 是唯一放行端点）。
 */
export async function runBackupRollback(
  root: string,
  name: string,
  confirm: boolean,
  deleteIntroduced = false,
): Promise<BackupImportOutcome> {
  if (!isSnapshotName(name)) return { status: 400, error: "invalid snapshot name" };
  const path = join(root, "backups", name);
  const info = await lstat(path).catch(() => null);
  if (!info?.isFile()) return { status: 400, error: "snapshot not found" };
  const bytes = await readFile(path).catch(() => null);
  if (!bytes) return { status: 400, error: "snapshot not found" };
  return runBackupImport(root, bytes, confirm, deleteIntroduced ? "delete" : "keep");
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
