/**
 * 本地回环守卫（170 号 W-A2）——Node sidecar 侧信任边界中间件。
 *
 * 与 engine-rs `src/server/loopback_guard.rs` 同规则、同 env 名（双端同水位）：
 * - `Origin` 头存在：`scheme://host[:port]` 剥端口后 host 必须是回环主机
 *   （localhost / 127.0.0.1 / [::1] / [::]，端口不限），或命中白名单；否则 403。
 * - `Host` 头存在：剥端口后必须是回环主机，否则 403（防 DNS rebinding）。
 * - 两头皆缺（supertest / curl / duel 形态）→ 放行，契约测试零改动。
 *
 * env：
 * - `INKOS_ENGINE_LOOPBACK_GUARD`：默认开；`0`/`false`/`off`（大小写不敏感）关。
 * - `INKOS_ENGINE_ALLOWED_ORIGINS`：逗号分隔的精确 Origin 白名单（追加项）。
 */

const LOOPBACK_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]", "[::]", "::1"]);

/** `host[:port]` 剥端口（IPv6 `[::1]:8787` 保留方括号整体）。 */
export function hostWithoutPort(host: string): string {
  if (host.startsWith("[")) {
    const end = host.indexOf("]");
    return end === -1 ? host : host.slice(0, end + 1);
  }
  const idx = host.lastIndexOf(":");
  return idx === -1 ? host : host.slice(0, idx);
}

export function isLoopbackHost(host: string): boolean {
  return LOOPBACK_HOSTS.has(host.toLowerCase());
}

/** Origin 判定：白名单精确命中，或 host 为回环；无 `://` 结构（含 `null`）拒绝。 */
export function originIsAllowed(origin: string, extras: readonly string[]): boolean {
  if (extras.includes(origin)) return true;
  const schemeEnd = origin.indexOf("://");
  if (schemeEnd === -1) return false;
  const rest = origin.slice(schemeEnd + 3);
  let hostPort = rest;
  for (const stop of ["/", "?", "#"]) {
    const stopIdx = hostPort.indexOf(stop);
    if (stopIdx !== -1) hostPort = hostPort.slice(0, stopIdx);
  }
  return isLoopbackHost(hostWithoutPort(hostPort));
}

/** `INKOS_ENGINE_LOOPBACK_GUARD` 值解析：`0`/`false`/`off` → 关，其余（含未设）→ 开。 */
export function parseGuardEnabled(value: string | undefined): boolean {
  if (value === undefined) return true;
  return !["0", "false", "off"].includes(value.trim().toLowerCase());
}

export interface LoopbackGuardOptions {
  readonly enabled?: boolean;
  readonly extraOrigins?: readonly string[];
}

/** 守卫配置（与 Rust 侧 `LoopbackGuardConfig::from_env` 同源语义）。 */
export function guardOptionsFromEnv(env: NodeJS.ProcessEnv): LoopbackGuardOptions {
  return {
    enabled: parseGuardEnabled(env["INKOS_ENGINE_LOOPBACK_GUARD"]),
    extraOrigins: (env["INKOS_ENGINE_ALLOWED_ORIGINS"] ?? "")
      .split(",")
      .map((item) => item.trim())
      .filter((item) => item.length > 0),
  };
}

/** Hono 中间件工厂：注册在 `cors()` 之前（先守卫后 CORS，403 不带 CORS 头）。 */
export function createLoopbackGuardMiddleware(options: LoopbackGuardOptions = {}) {
  const enabled = options.enabled ?? true;
  const extraOrigins = options.extraOrigins ?? [];
  return async (c: { req: { header(name: string): string | undefined }; json(body: unknown, status: 403): unknown }, next: () => Promise<void>) => {
    if (!enabled) {
      await next();
      return;
    }
    const origin = c.req.header("origin");
    if (origin !== undefined && !originIsAllowed(origin, extraOrigins)) {
      return c.json({ error: "origin not allowed" }, 403);
    }
    const host = c.req.header("host");
    if (host !== undefined && !isLoopbackHost(hostWithoutPort(host))) {
      return c.json({ error: "host not allowed" }, 403);
    }
    await next();
  };
}
