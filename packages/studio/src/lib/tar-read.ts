/**
 * R18/386 等待轮：tar 归档读取解析（导入恢复侧技术验证，纯函数零依赖）。
 *
 * 与导出侧 `buildTarArchive` 对偶：USTAR 512B/块遍历 + gunzip 解层。
 * 950 号起兼读跨端形态：GNU LongLink（typeflag 'L'，Rust 导出侧
 * tar::Builder 对超 100 字节路径的产出）与 POSIX USTAR prefix 字段。
 * 安全三则（备份导入专用，一律作用于 'L' 覆盖/prefix 拼接后的最终名）：
 * 1. 条目名拒绝 `..` 与绝对路径（201 号路径段守卫同语义）；
 * 2. typeflag 仅接受 `'0'`（普通文件）与 `'5'`（目录）及 'L'（长名扩展，
 *    不产出文件；PAX 'x' 无产出者维持拒绝，出现时再按需支持）；
 * 3. 总解压字节数上限（缺省 512MB，防 zip 炸弹，'L' payload 同受限）。
 */

export const TAR_BLOCK = 512;
export const TAR_MAX_TOTAL_BYTES = 512 * 1024 * 1024;

export interface TarEntry {
  readonly name: string;
  readonly bytes: Uint8Array;
}

const decoder = new TextDecoder();

function parseOctal(bytes: Uint8Array, start: number, length: number): number {
  const text = decoder.decode(bytes.subarray(start, start + length)).replace(/\0/g, "").trim();
  if (!text) return 0;
  const value = parseInt(text, 8);
  return Number.isFinite(value) ? value : 0;
}

function cleanName(bytes: Uint8Array, start: number, length: number): string {
  return decoder.decode(bytes.subarray(start, start + length)).replace(/\0/g, "").trim();
}

export interface UntarResult {
  readonly entries: ReadonlyArray<TarEntry>;
  /** 被安全规则拒绝的条目名（路径越界/类型不支持）。 */
  readonly rejected: ReadonlyArray<string>;
}

/**
 * 解析（可能 gzip 的）tar 归档。安全三则内置；超过解压上限抛错。
 * 输入可传 raw tar 或 gzip 层（自动探测 0x1f8b 魔数）。
 */
export function untar(
  input: Uint8Array,
  options: { maxTotalBytes?: number } = {},
): UntarResult {
  let bytes = input;
  // gzip 魔数 0x1f 0x8b。
  if (bytes.length >= 2 && bytes[0] === 0x1f && bytes[1] === 0x8b) {
    // Node 环境解压（浏览器调用方应传 raw tar）。
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const { gunzipSync } = require("node:zlib") as typeof import("node:zlib");
    bytes = gunzipSync(bytes);
  }

  const entries: TarEntry[] = [];
  const rejected: string[] = [];
  let total = 0;
  let offset = 0;
  // GNU LongLink（typeflag 'L'）挂起名：payload 为紧随条目的完整名。
  // Rust 导出侧（tar::Builder + Header::new_gnu）对超 100 字节路径产出
  // 该形态——不支持则 Rust 备份包在导入端被整包拒收（950 号）。
  let pendingLongName: string | null = null;

  while (offset + TAR_BLOCK <= bytes.length) {
    const header = bytes.subarray(offset, offset + TAR_BLOCK);
    if (header.every((byte) => byte === 0)) break; // 双零块结束
    offset += TAR_BLOCK;

    const size = parseOctal(header, 124, 12);
    const typeflag = String.fromCharCode(header[156] ?? 48);
    const padding = (TAR_BLOCK - (size % TAR_BLOCK)) % TAR_BLOCK;

    if (typeflag === "L") {
      // 长名扩展条目：payload 即下一条目名（GNU 形态以 \0 结尾）。
      // payload 计入解压上限（与其他条目同受 zip 炸弹防护）。
      total += size;
      if (total > (options.maxTotalBytes ?? TAR_MAX_TOTAL_BYTES)) {
        throw new Error(`tar exceeds max total bytes (${TAR_MAX_TOTAL_BYTES})`);
      }
      pendingLongName = decoder.decode(bytes.subarray(offset, offset + size)).replace(/\0+$/, "");
      offset += size + padding;
      continue;
    }

    const rawName = cleanName(header, 0, 100).replace(/^\.?\//, "");
    // 953 号审查修复：以来源判定（而非值比较）决定是否拼 prefix——若用值
    // 比较，畸形包中 'L' payload 恰等于截断名时会误拼 prefix 得错误名。
    const fromLongName = pendingLongName !== null;
    let name = fromLongName ? pendingLongName! : rawName;
    pendingLongName = null;
    // POSIX USTAR prefix 字段（magic "ustar\0"）拼接：>100 字节路径可由
    // prefix(≤155)+name(≤100) 表达——GNU 头（magic "ustar \0"）该区无
    // prefix 语义，不得拼接。
    if (!fromLongName && header[257] === 0x75 && header[258] === 0x73 && header[259] === 0x74
      && header[260] === 0x61 && header[261] === 0x72 && header[262] === 0) {
      const prefix = cleanName(header, 345, 155).replace(/^\.?\//, "");
      if (prefix) name = `${prefix}/${name}`;
    }

    // 安全检查一律作用于最终名（'L' 覆盖与 prefix 拼接之后）——长名/
    // prefix 通道不得绕过路径守卫。
    if (name.includes("..") || name.startsWith("/")) {
      rejected.push(name);
      // 跳过 payload 与 padding，继续解析后续条目。
      offset += Math.ceil(size / TAR_BLOCK) * TAR_BLOCK;
      continue;
    }
    if (typeflag !== "0" && typeflag !== "5" && typeflag !== "\0") {
      rejected.push(name || `(typeflag ${JSON.stringify(typeflag)})`);
      offset += Math.ceil(size / TAR_BLOCK) * TAR_BLOCK;
      continue;
    }

    total += size;
    if (total > (options.maxTotalBytes ?? TAR_MAX_TOTAL_BYTES)) {
      throw new Error(`tar exceeds max total bytes (${TAR_MAX_TOTAL_BYTES})`);
    }

    if (typeflag === "5") {
      continue; // 目录条目不产出文件
    }

    const payload = bytes.subarray(offset, offset + size);
    entries.push({ name, bytes: new Uint8Array(payload) });
    offset += size;
    // payload 的 512 块 padding
    offset += padding;
  }

  return { entries, rejected };
}
