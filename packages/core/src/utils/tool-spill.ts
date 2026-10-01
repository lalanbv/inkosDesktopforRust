/**
 * Tool result spill（R42，557 号）——engine-rs `interaction/spill.rs` 同水位
 * 对偶：超阈值工具结果不整块塞 LLM 消息，改为「头尾保留 + 全文落盘 +
 * 可恢复通知」。五条边界语义对齐 dsh spill-policy（详见 Rust 侧模块头注）：
 * 豁免防回环（read/ls/grep）/ 通知预留进预算 / 尽力而为降级 / 编译期常量
 * （上游运行时校验不适用）/ PTC 委托先行不适用。
 *
 * 落盘 `<projectRoot>/.inkos/spills/session-<sha256[0..12]>/<8hex>-<tool>.txt`
 * （wx+0600、目录 0700）；读回通道 = read 工具 `.inkos/spills/` 前缀放行
 * （agent-tools resolveReadPath 分支，与 Rust tool_read_book 同批）。
 */

import { createHash, randomBytes } from "node:crypto";
import { mkdir, open } from "node:fs/promises";
import { join } from "node:path";

/** 结果文本超过该 UTF-8 字节数触发 spill（< 8000 字符截断的最小字节量）。 */
export const SPILL_THRESHOLD_BYTES = 6000;
/** 头部保留字节。 */
export const SPILL_HEAD_BYTES = 2800;
/** 尾部保留字节。 */
export const SPILL_TAIL_BYTES = 1600;
/** 读回通道工具豁免（防回环）。 */
export const SPILL_EXEMPT_TOOLS: ReadonlyArray<string> = ["read", "ls", "grep"];

const GAP = "\n\n[...]\n\n";

/** spill 判定（豁免/错误结果/阈值三维；与 Rust should_spill 同语义）。 */
export function shouldSpill(toolName: string, text: string, isError: boolean): boolean {
  if (isError || SPILL_EXEMPT_TOOLS.includes(toolName)) return false;
  return Buffer.byteLength(text, "utf8") > SPILL_THRESHOLD_BYTES;
}

/** 会话作用域目录名（dsh sessionDir 同构：sha256 前 12 hex）。 */
export function sessionDirName(sessionId?: string): string {
  if (!sessionId) return "session-adhoc";
  return `session-${createHash("sha256").update(sessionId, "utf8").digest("hex").slice(0, 12)}`;
}

/**
 * UTF-8 字节预算内的最大码元前缀（tail 取后缀）——dsh fitText 的二分 +
 * surrogate 边界修复同构；预算为行为近似而非码点契约（Rust char 边界回退
 * 产出差 ≤1 码元）。导出 = 测试载体。
 */
export function sliceUtf8Bytes(text: string, maxBytes: number, tail = false): string {
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    const candidate = tail ? text.slice(text.length - mid) : text.slice(0, mid);
    if (Buffer.byteLength(candidate, "utf8") <= maxBytes) lo = mid;
    else hi = mid - 1;
  }
  // surrogate pair 完整性：cut 落在 pair 中间则让出一码元（dsh textSlice 同形）。
  if (tail && lo > 0 && lo < text.length) {
    const prev = text.charCodeAt(text.length - lo - 1);
    const next = text.charCodeAt(text.length - lo);
    if (prev >= 0xd800 && prev <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) lo -= 1;
  }
  if (!tail && lo > 0 && lo < text.length) {
    const prev = text.charCodeAt(lo - 1);
    const next = text.charCodeAt(lo);
    if (prev >= 0xd800 && prev <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) lo -= 1;
  }
  return tail ? text.slice(text.length - lo) : text.slice(0, lo);
}

/**
 * 落盘全文并返回通知用的项目相对路径；wx 独占写防碰撞（unix 权限 0600，
 * 目录 0700）。失败抛错由调用方降级。
 */
async function saveSpill(projectRoot: string, sessionId: string | undefined, toolName: string, text: string): Promise<string> {
  const dirName = sessionDirName(sessionId);
  const dir = join(projectRoot, ".inkos", "spills", dirName);
  await mkdir(dir, { recursive: true, mode: 0o700 });
  const random = randomBytes(4).toString("hex");
  const safeTool = toolName.replace(/[^a-z0-9_]/g, "_");
  const fileName = `${random}-${safeTool}.txt`;
  const handle = await open(join(dir, fileName), "wx", 0o600);
  try {
    await handle.writeFile(text, "utf-8");
  } finally {
    await handle.close();
  }
  return `.inkos/spills/${dirName}/${fileName}`;
}

/**
 * spill 一个超大结果：头尾保留 + GAP + 省略通知（可恢复引用）。
 * 落盘失败保留原文本并返回（尽力而为，绝不因 spill 丢结果）。
 */
export async function spillToolText(projectRoot: string, sessionId: string | undefined, toolName: string, text: string): Promise<string> {
  const total = Buffer.byteLength(text, "utf8");
  const head = sliceUtf8Bytes(text, SPILL_HEAD_BYTES);
  const tail = sliceUtf8Bytes(text, SPILL_TAIL_BYTES, true);
  try {
    const relativePath = await saveSpill(projectRoot, sessionId, toolName, text);
    const omitted = total - Buffer.byteLength(head, "utf8") - Buffer.byteLength(tail, "utf8");
    return `${head}${GAP}${tail}\n\n(Omitted ${omitted} bytes. Full result stored at: ${relativePath}. Use the read tool on this path to view the full text.)`;
  } catch {
    return text;
  }
}
