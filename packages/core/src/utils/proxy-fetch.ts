import { ProxyAgent, fetch as undiciFetch } from "undici";

type ProxyEnv = Record<string, string | undefined>;
type FetchInitWithDispatcher = RequestInit & { dispatcher?: unknown };
type FetchInput = Parameters<typeof fetch>[0];

/**
 * 回环/未指定地址判定（恒不走代理）：`localhost` / `*.localhost` /
 * `127.0.0.0/8` / `::1` / `0.0.0.0` / `::`（未指定地址连接永不出本机，走代理必错）。
 *
 * 本地推理端点（Ollama `127.0.0.1:11434`、LM Studio `127.0.0.1:1234`）与用户代理
 * 同机共存是常态；reqwest/undici 采纳 env 代理（reqwest 另经 system-proxy 特性读
 * macOS SCDynamicStore 且不读 ExceptionsList），代理守护进程降级窗口会把回环
 * 请求全灭。旁路直连对齐浏览器/系统代理豁免惯用形；与 Rust 腿
 * `engine-rs/src/utils/loopback_proxy.rs::is_loopback_base_url` 镜像对齐（1573 号
 * 双栈单一语义，测试边界互为镜像）。
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
      // WHATWG hostname 对 IPv6 序列化带方括号（`http://[::1]/` → "[::1]"）。
      host === "[::1]" ||
      host === "::1" ||
      host === "[::]" ||
      host === "::" ||
      host === "0.0.0.0" ||
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
    // 全局 undici-types（随 Node 类型锁）与 npm undici 自带类型跨包不同版本，
    // Dispatcher 类不结构兼容——边界处显式收窄（运行时同一 npm undici，自洽）。
    dispatcher: new ProxyAgent(proxyUrl) as unknown as FetchInitWithDispatcher["dispatcher"],
  };
}

export function fetchWithProxy(
  input: Parameters<typeof fetch>[0],
  init: RequestInit = {},
  explicitProxyUrl?: string,
  env: ProxyEnv = process.env,
): ReturnType<typeof fetch> {
  // 回环旁路：本地推理端点直连（1572/1573，详见 isLoopbackTarget）。
  if (isLoopbackTarget(input)) return fetch(input, init);
  const proxyUrl = resolveProxyUrl(explicitProxyUrl, env);
  if (!proxyUrl) return fetch(input, init);
  // 代理模式必须走 undici 自家 fetch（版本一致配对）：dispatcher 处理协议跨
  // major 不稳定——1573 号实证（Node 26）：全局 fetch（内置 undici）× npm
  // undici@6 ProxyAgent 同步抛 "invalid onError"；undici@6/7 的自家
  // fetch×ProxyAgent 在 Node 26 整体挂死（连接从未发出）；仅 undici@8 配对
  // 全接口可用（engines >=22.19.0，恰为本项目 node_min）。自包内 fetch+agent
  // 由同一依赖版本构造，协议错位在构造上不可能发生。
  return undiciFetch(
    input as Parameters<typeof undiciFetch>[0],
    { ...init, dispatcher: new ProxyAgent(proxyUrl) } as unknown as Parameters<typeof undiciFetch>[1],
  ) as unknown as ReturnType<typeof fetch>;
}
