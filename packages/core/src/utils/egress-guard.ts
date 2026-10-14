// ── Egress host guard（SSRF 初判防线，556/635 号；689 号随上游 2.0 合并重施）──
//
// URL 类出站（fetchUrl / materials ingest）的地址来自搜索结果或模型经外部数据
// 间接触达，host 面在连接前拒绝回环/私网/链路本地/保留段。
// 与 engine-rs web_search.rs / materials.rs 同水位对偶：分类器纯函数逐项镜像
//（v4 八位组判/v6 前缀判/mapped 还原），域名 resolve 后对全部地址判；resolve
// 失败不拦（连接阶段自然失败）。闸位定性同为初判：DNS rebinding 与 30x 重定向
// 旁路需连接级 pin 才能封死，备案 R40+ 评估。
//
// 豁免开关=researchSearch.allowPrivateEgress（项目配置透传，不从 LLM 工具参数
// 来——防提示注入自开豁免；Rust 侧 materials.rs 同纪律）。

/** Extract host from http(s) URL (strip userinfo/port; IPv6 inside brackets). Exported for tests + engine-rs mirror parity. */
export function urlHost(url: string): string | null {
  let rest: string;
  if (url.startsWith("http://")) rest = url.slice("http://".length);
  else if (url.startsWith("https://")) rest = url.slice("https://".length);
  else return null;
  const authorityEnd = rest.search(/[/?#]/);
  const authority = authorityEnd === -1 ? rest : rest.slice(0, authorityEnd);
  const at = authority.lastIndexOf("@");
  const hostPort = at === -1 ? authority : authority.slice(at + 1);
  if (hostPort.startsWith("[")) {
    const end = hostPort.indexOf("]");
    const host = end === -1 ? "" : hostPort.slice(1, end);
    return host || null;
  }
  const host = hostPort.split(":")[0] ?? "";
  return host || null;
}

/** Local-name literal check (pre-resolution fast path). */
export function isBlockedHostLiteral(host: string): boolean {
  const h = host.replace(/\.+$/, "").toLowerCase();
  return h === "localhost" || h.endsWith(".localhost") || h === "local" || h.endsWith(".local");
}

function isBlockedV4(ip: string): boolean {
  const octets = ip.split(".").map(Number);
  if (octets.length !== 4 || octets.some((o) => !Number.isInteger(o) || o < 0 || o > 255)) return false;
  const [a, b, c, d] = octets;
  return (
    a === 127 ||
    a === 10 ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 169 && b === 254) ||
    a === 0 ||
    (a === 255 && b === 255 && c === 255 && d === 255)
  );
}

/** IP segment check — categories mirror engine-rs is_blocked_ip exactly. */
export function isBlockedIp(ip: string): boolean {
  if (ip.includes(":")) {
    const lower = ip.toLowerCase();
    const mapped = /^::ffff:(\d{1,3}(?:\.\d{1,3}){3})$/.exec(lower);
    if (mapped) return isBlockedV4(mapped[1]);
    if (lower === "::" || lower === "::1") return true;
    const g0 = parseInt((lower.split(":")[0] ?? ""), 16);
    if (Number.isFinite(g0)) {
      if ((g0 & 0xfe00) === 0xfc00) return true; // ULA fc00::/7
      if ((g0 & 0xffc0) === 0xfe80) return true; // link-local fe80::/10
    }
    return false;
  }
  return isBlockedV4(ip);
}

/**
 * Pre-connection egress check: IP literals judged directly; hostnames
 * literal-screened then resolved and every address judged. Resolve failure
 * does not block (the connection fails naturally) — the guard only rejects
 * clear local literals and resolved private targets.
 */
export async function assertPublicEgressHost(url: string): Promise<void> {
  const host = urlHost(url);
  if (!host) throw new Error("unsupported URL scheme or missing host");
  if (isBlockedHostLiteral(host)) throw new Error(`host '${host}' is a local network name`);
  if (isBlockedIp(host)) throw new Error(`host '${host}' is a loopback/private address`);
  const { lookup } = await import("node:dns/promises");
  let resolved: Array<{ address: string; family: number }>;
  try {
    resolved = await lookup(host, { all: true });
  } catch {
    return; // resolve 失败不拦
  }
  for (const { address } of resolved) {
    if (isBlockedIp(address)) {
      throw new Error(`host '${host}' resolves to loopback/private address ${address}`);
    }
  }
}
