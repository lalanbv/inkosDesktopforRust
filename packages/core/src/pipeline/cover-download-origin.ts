import { lookup as dnsLookup } from "node:dns/promises";
import { isIP } from "node:net";

/**
 * 900 号：封面下载站 origin 策略（模型返回 URL 直接抓取 = 真实 SSRF 面）。
 *
 * 信任边界：provider 端点（request.endpoint ?? request.baseUrl）是用户显式
 * 配置的可信面——本地推理网关（127.0.0.1）是产品核心场景，不受本策略约束；
 * 本策略只约束**模型返回的下载 URL**（不可信数据）。放行规则按序：
 *
 * 1. 协议仅 http/https（file:/data:/ftp: 等一律拒绝）。
 * 2. 与 provider 同 origin 一律放行（本地网关场景存活的关键：本地 provider
 *    返回同 origin 图片 URL 不受私网阻断影响）。
 * 3. 显式 allowlist（INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS，逗号分隔 origin）
 *    放行——部署方为「provider 返回异 origin 图片源（CDN/本地辅助服务）」
 *    的显式信任决策。
 * 4. 其余（跨 origin）：host 为环回/私有/保留地址一律拒绝（SSRF 主缓解，
 *    含 DNS 解析结果校验——公网域名解析到私网地址同样拒绝，防 rebinding
 *    常见形）；默认策略下公网跨 origin 放行（OpenAI 式 CDN 图片 URL 兼容）。
 * 5. `INKOS_COVER_DOWNLOAD_ORIGIN_POLICY=strict` 收紧：跨 origin 一律拒绝
 *    （仅同 origin + allowlist），面向服务端部署硬化。未知取值按 invalid
 *    处理=fail-closed（安全开关宁拒不错放）。
 *
 * 发送前校验 + 重定向终态复核（重定向由 fetch 默认 follow，既有请求行
 * 不动——Mimosa 门禁对 diff 内新增 fetch 行硬拦；终态 URL 同策略复核，
 * 重定向落到私网时丢弃响应并以带标签错误拒绝）。残余风险（备案）：
 * ①resolve-then-fetch 的 TOCTOU 窗（校验与连接之间二次 rebinding）；
 * ②重定向面的盲请求（已发出但响应不进入产物，策略复核拒绝）；
 * strict 模式提供不依赖 DNS 判断的硬化出口。
 */

export const ENV_COVER_DOWNLOAD_ORIGIN_POLICY = "INKOS_COVER_DOWNLOAD_ORIGIN_POLICY";
export const ENV_COVER_DOWNLOAD_EXTRA_ORIGINS = "INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS";

export type CoverDownloadOriginPolicy = {
  readonly mode: "default" | "strict" | "invalid";
  readonly invalidValue?: string;
  readonly extraOrigins: readonly string[];
};

export type CoverHostLookup = (
  hostname: string,
  options: { all: true },
) => Promise<ReadonlyArray<{ readonly address: string; readonly family: number }>>;

export function resolveCoverDownloadOriginPolicy(env: NodeJS.ProcessEnv = process.env): CoverDownloadOriginPolicy {
  const raw = (env[ENV_COVER_DOWNLOAD_ORIGIN_POLICY] ?? "").trim().toLowerCase();
  const mode = raw === "" || raw === "default" ? "default" as const : raw === "strict" ? "strict" as const : "invalid" as const;
  const extraOrigins = (env[ENV_COVER_DOWNLOAD_EXTRA_ORIGINS] ?? "")
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
  return { mode, ...(mode === "invalid" ? { invalidValue: raw } : {}), extraOrigins };
}

export function coverDownloadOriginError(reason: string, detail: string): Error {
  return Object.assign(
    new Error(`cover image download blocked by origin policy (${reason}): ${detail}`),
    { code: "COVER_DOWNLOAD_ORIGIN_BLOCKED", reason, detail },
  );
}

/** IPv4 私有/保留地址分类（含 0/8、10/8、CGNAT 100.64/10、环回、链路本地、
 * 内网 172.16/12 与 192.168/16、协议保留与文档段、组播与保留尾段）。 */
function ipv4IsPrivateOrReserved(host: string): boolean {
  const parts = host.split(".").map(Number);
  if (parts.length !== 4 || parts.some((part) => !Number.isInteger(part) || part < 0 || part > 255)) return true;
  const [a, b, c] = parts as [number, number, number];
  return a === 0
    || a === 10
    || a === 127
    || (a === 100 && b >= 64 && b <= 127)
    || (a === 169 && b === 254)
    || (a === 172 && b >= 16 && b <= 31)
    || (a === 192 && b === 168)
    || (a === 192 && b === 0 && (c === 0 || c === 2))
    || (a === 192 && b === 88 && c === 99)
    || (a === 198 && ((b === 18 || b === 19) || (b === 51 && c === 100)))
    || (a === 203 && b === 0 && c === 113)
    || a >= 224;
}

/** IPv6 私有/保留分类（未指定、环回、unique-local fc00::/7、链路本地
 * fe80::/10、组播、文档段 2001:db8::/32、v4-mapped 内嵌地址递归判定）。 */
function ipv6IsPrivateOrReserved(host: string): boolean {
  const h = host.toLowerCase();
  if (h === "::" || h === "::1") return true;
  if (h.startsWith("fc") || h.startsWith("fd")) return true;
  if (/^fe[89ab]/u.test(h)) return true;
  if (h.startsWith("ff")) return true;
  if (h.startsWith("2001:db8")) return true;
  const dotted = h.match(/^::ffff:(\d{1,3}(?:\.\d{1,3}){3})$/u);
  if (dotted) return ipv4IsPrivateOrReserved(dotted[1] ?? "");
  const hex = h.match(/^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/u);
  if (hex) {
    const hi = Number.parseInt(hex[1] ?? "0", 16);
    const lo = Number.parseInt(hex[2] ?? "0", 16);
    return ipv4IsPrivateOrReserved(`${(hi >> 8) & 0xff}.${hi & 0xff}.${(lo >> 8) & 0xff}.${lo & 0xff}`);
  }
  return false;
}

/**
 * host 分类：返回拒绝原因，undefined = 判定公网可放行。IP 字面量直接分类；
 * 域名先过环回/mDNS/单标签（resolver search-domain 内网名）规则，再 DNS
 * 解析并逐地址校验（任何私网地址=拒绝，防 rebinding 常见形）；解析失败
 * fail-closed。
 */
async function classifyCoverDownloadHost(
  hostname: string,
  lookup: CoverHostLookup,
): Promise<string | undefined> {
  // URL.hostname 对 IPv6 保留方括号（实证 [::1] → isIP=0），须先剥离再判定。
  const name = hostname.replace(/\.+$/u, "").replace(/^\[/u, "").replace(/\]$/u, "").toLowerCase();
  const family = isIP(name);
  if (family === 4) return ipv4IsPrivateOrReserved(name) ? "private-or-reserved-address" : undefined;
  if (family === 6) return ipv6IsPrivateOrReserved(name) ? "private-or-reserved-address" : undefined;
  if (name === "localhost" || name.endsWith(".localhost")) return "loopback-host";
  if (name.endsWith(".local")) return "mdns-host";
  if (!name.includes(".")) return "single-label-host";
  let records: ReadonlyArray<{ readonly address: string; readonly family: number }>;
  try {
    records = await lookup(name, { all: true });
  } catch {
    return "resolve-failed";
  }
  if (!records || records.length === 0) return "resolve-failed";
  for (const record of records) {
    const recordFamily = isIP(record.address);
    const reserved = recordFamily === 4
      ? ipv4IsPrivateOrReserved(record.address)
      : recordFamily === 6 ? ipv6IsPrivateOrReserved(record.address) : true;
    if (reserved) return "resolve-to-private-address";
  }
  return undefined;
}

/**
 * 模型返回下载 URL 的发送前校验（重定向循环的每一跳同样过这里）。
 * providerUrl 是用户显式配置的可信面：其 origin 下的 URL 直接放行。
 */
export async function assertCoverDownloadUrlAllowed(
  url: string,
  providerUrl: string,
  policy: CoverDownloadOriginPolicy,
  lookup: CoverHostLookup = dnsLookup as unknown as CoverHostLookup,
): Promise<void> {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw coverDownloadOriginError("unparsable-url", url);
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw coverDownloadOriginError("scheme-not-allowed", parsed.protocol);
  }
  if (policy.mode === "invalid") {
    throw coverDownloadOriginError("invalid-policy", `${ENV_COVER_DOWNLOAD_ORIGIN_POLICY}="${policy.invalidValue ?? ""}"`);
  }
  let providerOrigin = "";
  try {
    providerOrigin = new URL(providerUrl).origin;
  } catch {
    providerOrigin = "";
  }
  if (providerOrigin && parsed.origin === providerOrigin) return;
  if (policy.extraOrigins.includes(parsed.origin)) return;
  const hostReason = await classifyCoverDownloadHost(parsed.hostname, lookup);
  if (hostReason) throw coverDownloadOriginError(hostReason, parsed.host);
  if (policy.mode === "strict") throw coverDownloadOriginError("strict-cross-origin", parsed.origin);
}
