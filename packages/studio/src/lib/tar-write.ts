/**
 * tar 归档写侧（与 `lib/tar-read.ts` 对偶）——954 号自 `api/server.ts`
 * 迁出为独立模块：备份 export、pre-restore 快照构造（backup-snapshot.ts）
 * 与交互电影 source 导出共用同一打包面（单一真源）。
 *
 * 950 号长路径三通道——≤100 单头；100<x≤255 且存在 '/' 边界走 POSIX
 * USTAR prefix 拆分（读侧 Rust tar::Archive 与本仓 untar 均支持）；
 * 其余（>255 或无边界）走 GNU LongLink（typeflag 'L'，Rust 导出侧
 * tar::Builder 同形态，双端读侧均以 'L' payload 为准）。此前 >100 直接
 * 抛错=export 对真实长路径（works/<id>/revisions/<rev>/...）不可用。
 */

import { readFile, readdir, stat } from "node:fs/promises";
import { join } from "node:path";

export async function buildTarArchive(sourceDir: string, packageRootName: string): Promise<Buffer> {
  const files = await listArchiveFiles(sourceDir);
  const chunks: Buffer[] = [];
  for (const file of files) {
    const payload = await readFile(join(sourceDir, file));
    const archiveName = normalizeArchivePath(join(packageRootName, file));
    appendTarEntry(chunks, archiveName, payload);
  }
  chunks.push(Buffer.alloc(1024));
  return Buffer.concat(chunks);
}

export function appendTarEntry(chunks: Buffer[], archiveName: string, payload: Buffer): void {
  const split = splitUstarPath(archiveName);
  if (split) {
    chunks.push(createTarHeader(split.name, payload.byteLength, split.prefix));
  } else if (Buffer.byteLength(archiveName, "utf-8") <= 100) {
    chunks.push(createTarHeader(archiveName, payload.byteLength));
  } else {
    const longName = Buffer.from(`${archiveName}\0`, "utf-8");
    chunks.push(createTarHeader("././@LongLink", longName.byteLength, "", "L"));
    chunks.push(longName);
    const longPadding = (512 - (longName.byteLength % 512)) % 512;
    if (longPadding > 0) chunks.push(Buffer.alloc(longPadding));
    // 真条目 name 字段放截断名（GNU tar 形态；读取侧以 'L' payload 为准）。
    chunks.push(createTarHeader(truncateTarName(archiveName), payload.byteLength));
  }
  chunks.push(payload);
  const padding = (512 - (payload.byteLength % 512)) % 512;
  if (padding > 0) chunks.push(Buffer.alloc(padding));
}

/** POSIX USTAR 拆分：>100 字节路径按 '/' 边界拆 prefix(≤155)+name(≤100)。 */
function splitUstarPath(path: string): { prefix: string; name: string } | null {
  const bytes = Buffer.from(path, "utf-8");
  if (bytes.byteLength <= 100) return null;
  for (let i = bytes.byteLength - 1; i > 0; i--) {
    if (bytes[i] !== 0x2f) continue; // '/'
    if (i <= 155 && bytes.byteLength - i - 1 <= 100) {
      return { prefix: bytes.subarray(0, i).toString("utf-8"), name: bytes.subarray(i + 1).toString("utf-8") };
    }
  }
  return null;
}

/** 字节安全截断至 USTAR name 字段上限（不切 UTF-8 多字节中间）。 */
function truncateTarName(path: string): string {
  const bytes = Buffer.from(path, "utf-8");
  if (bytes.byteLength <= 100) return path;
  let end = 100;
  while (end > 0 && (bytes[end] & 0xc0) === 0x80) end--;
  return bytes.subarray(0, end).toString("utf-8");
}

export async function listArchiveFiles(dir: string, prefix = ""): Promise<string[]> {
  const entries = await readdir(dir, { withFileTypes: true });
  const files: string[] = [];
  for (const entry of entries) {
    if (entry.name === ".DS_Store") continue;
    const relativePath = prefix ? join(prefix, entry.name) : entry.name;
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...await listArchiveFiles(fullPath, relativePath));
    } else if (entry.isFile()) {
      files.push(normalizeArchivePath(relativePath));
    } else {
      const info = await stat(fullPath).catch(() => null);
      if (info?.isFile()) files.push(normalizeArchivePath(relativePath));
    }
  }
  return files.sort((a, b) => a.localeCompare(b));
}

function normalizeArchivePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/^\/+/g, "");
}

function createTarHeader(name: string, size: number, prefix = "", typeflag: "0" | "L" = "0"): Buffer {
  const header = Buffer.alloc(512, 0);
  writeTarString(header, 0, 100, name);
  writeTarOctal(header, 100, 8, 0o644);
  writeTarOctal(header, 108, 8, 0);
  writeTarOctal(header, 116, 8, 0);
  writeTarOctal(header, 124, 12, size);
  writeTarOctal(header, 136, 12, Math.floor(Date.now() / 1000));
  header.fill(0x20, 148, 156);
  header[156] = typeflag.charCodeAt(0);
  writeTarString(header, 257, 6, "ustar");
  writeTarString(header, 263, 2, "00");
  if (prefix) writeTarString(header, 345, 155, prefix);
  let checksum = 0;
  for (const byte of header) checksum += byte;
  writeTarOctal(header, 148, 8, checksum);
  return header;
}

function writeTarString(header: Buffer, offset: number, length: number, value: string): void {
  const encoded = Buffer.from(value);
  if (encoded.byteLength > length) {
    throw new Error(`Archive path is too long for tar header: ${value}`);
  }
  encoded.copy(header, offset);
}

function writeTarOctal(header: Buffer, offset: number, length: number, value: number): void {
  const text = value.toString(8).padStart(length - 1, "0").slice(-(length - 1));
  header.write(text, offset, length - 1, "ascii");
  header[offset + length - 1] = 0;
}
