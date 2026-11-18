import { ProxyAgent } from "undici";

type ProxyEnv = Record<string, string | undefined>;
type FetchInitWithDispatcher = RequestInit & { dispatcher?: unknown };
type FetchInput = Parameters<typeof fetch>[0];

/**
 * 回环目标判定（恒不走代理）：`localhost` / `*.localhost` / `127.0.0.0/8` / `::1`。
 *
 * 本地推理端点（Ollama `127.0.0.1:11434`、LM Studio `127.0.0.1:1234`）与用户代理
 * 同机共存是常态；代理对回环地址没有合理转发路径（代理回连本机回环端口通常被
 * 拒或形成自环），旁路直连对齐浏览器/系统代理豁免惯用形（1572 号 Rust 腿三站
 * 同族先例：系统代理把回环探活送进代理恒超时）。
 */
function isLoopbackTarget(input: FetchInput): boolean {
  try {
    const raw =
      typeof input === "string"
        ? input
        : input instanceof URL
          ? input.href
          : (input as Request).url;
    if (typeof raw !== "string" || raw.length === 0) return false;
    const host = new URL(raw).hostname.toLowerCase();
    return (
      host === "localhost" ||
      host.endsWith(".localhost") ||
      host === "[::1]" ||
      host === "::1" ||
      /^127\.\d+\.\d+\.\d+$/.test(host)
    );
  } catch {
    return false;
  }
}

export function resolveProxyUrl(explicitProxyUrl?: string, env: ProxyEnv = process.env): string | undefined {
  const candidate = [
    explicitProxyUrl,
    env.INKOS_LLM_PROXY_URL,
    env.HTTPS_PROXY,
    env.https_proxy,
    env.HTTP_PROXY,
    env.http_proxy,
  ].find((value) => typeof value === "string" && value.trim().length > 0)?.trim();

  if (!candidate) return undefined;
  const parsed = new URL(candidate);
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error(`Unsupported proxy protocol: ${parsed.protocol}`);
  }
  return candidate;
}

export function buildProxyFetchInit(
  init: RequestInit = {},
  explicitProxyUrl?: string,
  env: ProxyEnv = process.env,
): FetchInitWithDispatcher {
  const proxyUrl = resolveProxyUrl(explicitProxyUrl, env);
  if (!proxyUrl) return init;
  return {
    ...init,
    dispatcher: new ProxyAgent(proxyUrl),
  };
}

export function fetchWithProxy(
  input: Parameters<typeof fetch>[0],
  init: RequestInit = {},
  explicitProxyUrl?: string,
  env: ProxyEnv = process.env,
): ReturnType<typeof fetch> {
  const initOut = isLoopbackTarget(input) ? init : buildProxyFetchInit(init, explicitProxyUrl, env);
  return fetch(input, initOut);
}
