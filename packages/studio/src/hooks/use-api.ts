import { useState, useEffect, useCallback, useRef } from "react";
import { localizeKnownRuntimeMessage } from "../lib/error-copy";

const BASE = "/api/v1";
const API_INVALIDATE_EVENT = "inkos:api-invalidate";

interface ApiInvalidateDetail {
  readonly paths: ReadonlyArray<string>;
}

export function buildApiUrl(path: string): string | null {
  const normalized = String(path ?? "").trim();
  if (!normalized) return null;
  if (normalized.startsWith(`${BASE}/`) || normalized === BASE) {
    return normalized;
  }
  return normalized.startsWith("/") ? `${BASE}${normalized}` : `${BASE}/${normalized}`;
}

// 1426 号：后端不可达分类器。fetch 在网络层失败（引擎未启动/已退出/端口无人
// 监听）时抛 TypeError，但各引擎 message 不同：Blink(Edge/Chrome)=Failed to
// fetch、WebKit(Safari/WKWebView)=Load failed、undici(Node)=fetch failed、
// Firefox=NetworkError。这类错误与 inkos.json 无关——启动门若仍引导用户去
// 检查配置文件就是误导（1426 实证：inkos.json 合法仍见此卡）。
// 1427 号补充（活体实证）：dev 工作流（vite dev :4567 → 代理 :4569）在后端
// ECONNREFUSED 时返回 HTTP 500 + 空 text/plain 体（非 TypeError）——
// readErrorMessage 回退成裸 `${status} ${statusText}`。裸网关类 5xx（500/502/
// 503/504 + 通用 statusText 整串锚定）只在中间代理产生：双端引擎自身 500 恒
// 带 JSON 详情（如 18801 实采 PROJECT_CONFIG_INVALID 人话串），裸 5xx 即
// 「上游不可达」签名，同样不该怪 inkos.json。
// 注意：useApi 只透传 message（Error 实例在 hook 内部已降为字符串），故按
// message 判定而非 instanceof TypeError。
// 网络层样例为四大引擎固定文案（引擎常量，仓内无生产函数可派生——按 1007 号
// 「不可派生才手打并注明理由」契约纪律备案手打）；裸 5xx 样例为 1427 活体
// curl 实采生产派生。
// 1429 号补（逐功能审查发现的同族残余）：readErrorMessage 回退串
// `${status} ${statusText}`.trim() 在 statusText 为空（HTTP/2 响应无 reason
// phrase，或不回 reason phrase 的裸代理）时恰为裸状态码（"500 "→trim→"500"），
// 整串锚定不命中→仍误指 inkos.json。statusText 段改可选后裸码与带短语两形
// 同锚定；引擎自身 500 恒带 JSON 详情（不落回退串），裸码仍只在中间层产生，
// 放宽不改变分流边界（带详情/前后缀依旧不误判）。
const BACKEND_UNREACHABLE_PATTERN =
  /failed to fetch|load failed|fetch failed|networkerror|network error|^5(00|02|03|04)( (Internal Server Error|Bad Gateway|Service Unavailable|Gateway Timeout))?$/i;

export function isBackendUnreachableMessage(message: string | null | undefined): boolean {
  if (!message) return false;
  return BACKEND_UNREACHABLE_PATTERN.test(message);
}

export function deriveInvalidationPaths(path: string): ReadonlyArray<string> {
  const normalized = buildApiUrl(path);
  if (!normalized) return [];

  if (
    normalized === "/api/v1/books/create" ||
    normalized === "/api/v1/fanfic/init" ||
    normalized === "/api/v1/spinoff/init" ||
    normalized === "/api/v1/imitation/init"
  ) {
    return ["/api/v1/books"];
  }

  if (normalized === "/api/v1/project") {
    return ["/api/v1/project"];
  }

  if (normalized.startsWith("/api/v1/project/")) {
    return ["/api/v1/project", normalized];
  }

  const bookAction = normalized.match(/^\/api\/v1\/books\/([^/]+)\/write-next$/);
  if (bookAction) {
    return ["/api/v1/books", `/api/v1/books/${bookAction[1]}`];
  }

  if (/^\/api\/v1\/daemon\/(start|stop)$/.test(normalized)) {
    return ["/api/v1/daemon"];
  }

  return [];
}

export function invalidateApiPaths(paths: ReadonlyArray<string>): void {
  if (!paths.length || typeof window === "undefined") {
    return;
  }

  window.dispatchEvent(new CustomEvent<ApiInvalidateDetail>(API_INVALIDATE_EVENT, {
    detail: { paths: [...new Set(paths)] },
  }));
}

async function readErrorMessage(res: Response): Promise<string> {
  const contentType = res.headers.get("content-type") ?? "";
  if (contentType.includes("application/json")) {
    try {
      const json = await res.json() as { error?: unknown };
      if (typeof json.error === "string" && json.error.trim()) {
        return localizeKnownRuntimeMessage(json.error);
      }
      if (
        json.error &&
        typeof json.error === "object" &&
        "message" in json.error &&
        typeof (json.error as { message?: unknown }).message === "string" &&
        (json.error as { message: string }).message.trim()
      ) {
        return localizeKnownRuntimeMessage((json.error as { message: string }).message);
      }
    } catch {
      // fall through
    }
  }
  return localizeKnownRuntimeMessage(`${res.status} ${res.statusText}`.trim());
}

export class ApiResponseError extends Error {
  constructor(message: string, readonly status: number, readonly payload: unknown) { super(message); }
}

export async function fetchJson<T>(
  path: string,
  init: RequestInit = {},
  deps?: { readonly fetchImpl?: typeof fetch },
): Promise<T> {
  const url = buildApiUrl(path);
  if (!url) {
    throw new Error("API path is required");
  }

  const fetchImpl = deps?.fetchImpl ?? fetch;
  const res = await fetchImpl(url, init);

  if (!res.ok) {
    const payload = await res.clone().json().catch(() => undefined);
    throw new ApiResponseError(await readErrorMessage(res), res.status, payload);
  }

  if (res.status === 204) {
    return undefined as T;
  }

  const contentType = res.headers.get("content-type") ?? "";
  if (!contentType.includes("application/json")) {
    const text = await res.text();
    if (!text.trim()) {
      return undefined as T;
    }
    return JSON.parse(text) as T;
  }

  return await res.json() as T;
}

export function useApi<T>(path: string) {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // 1008 号：响应竞态守卫（latest-wins）。refetch 是多入口共同漏斗（挂载
  // effect / path 变化 / API 失效事件 / 消费者手动调用），旧请求晚到不得覆盖
  // 新请求的 data / error / loading 终态。seq 比较 + 三分支卫兵，早退不掉
  // loading（新请求在途时由新请求自己的 finally 收口）。
  const seqRef = useRef(0);

  const refetch = useCallback(async () => {
    const seq = ++seqRef.current;
    const url = buildApiUrl(path);
    if (!url) {
      setData(null);
      setError(null);
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const json = await fetchJson<T>(url);
      if (seq !== seqRef.current) return;
      setData(json);
    } catch (e) {
      if (seq !== seqRef.current) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (seq === seqRef.current) setLoading(false);
    }
  }, [path]);

  useEffect(() => {
    refetch();
  }, [refetch]);

  useEffect(() => {
    const url = buildApiUrl(path);
    if (!url || typeof window === "undefined") {
      return;
    }

    const handleInvalidate = (event: Event) => {
      const detail = (event as CustomEvent<ApiInvalidateDetail>).detail;
      if (!detail?.paths.includes(url)) return;
      void refetch();
    };

    window.addEventListener(API_INVALIDATE_EVENT, handleInvalidate);
    return () => {
      window.removeEventListener(API_INVALIDATE_EVENT, handleInvalidate);
    };
  }, [path, refetch]);

  return { data, loading, error, refetch, mutate: setData };
}

export async function postApi<T>(path: string, body?: unknown): Promise<T> {
  const result = await fetchJson<T>(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: body ? JSON.stringify(body) : undefined,
  });
  invalidateApiPaths(deriveInvalidationPaths(path));
  return result;
}

export async function putApi<T>(path: string, body?: unknown): Promise<T> {
  const result = await fetchJson<T>(path, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: body ? JSON.stringify(body) : undefined,
  });
  invalidateApiPaths(deriveInvalidationPaths(path));
  return result;
}
