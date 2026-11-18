import { afterEach, describe, expect, it, vi } from "vitest";

// vitest 5：vi.fn 的实现须为可构造的 function（箭头实现无法被 new）。
const proxyAgentMock = vi.fn(function (this: unknown, url: string) {
  return { kind: "proxy-agent", url };
});
// undici 自家 fetch 桩：代理模式由 fetchWithProxy 走版本一致配对（1573）。
const undiciFetchMock = vi.fn().mockResolvedValue({ ok: true, via: "undici" });

vi.mock("undici", () => ({
  ProxyAgent: proxyAgentMock,
  fetch: undiciFetchMock,
}));

describe("proxy fetch helpers", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("prefers explicit llm proxyUrl over environment proxy variables", async () => {
    const { fetchWithProxy, resolveProxyUrl } = await import("../utils/proxy-fetch.js");
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);

    const env = {
      INKOS_LLM_PROXY_URL: "http://inkos-env-proxy:9910",
      HTTPS_PROXY: "http://standard-proxy:9910",
    };

    expect(resolveProxyUrl("http://explicit-proxy:9910", env)).toBe("http://explicit-proxy:9910");
    await fetchWithProxy("https://api.example/v1/chat/completions", { method: "POST" }, "http://explicit-proxy:9910", env);

    expect(proxyAgentMock).toHaveBeenCalledWith("http://explicit-proxy:9910");
    // 代理模式走 undici 自家 fetch（版本一致配对），不经全局 fetch。
    expect(fetchMock).not.toHaveBeenCalled();
    expect(undiciFetchMock).toHaveBeenCalledWith(
      "https://api.example/v1/chat/completions",
      expect.objectContaining({
        method: "POST",
        dispatcher: { kind: "proxy-agent", url: "http://explicit-proxy:9910" },
      }),
    );
  });

  it("uses INKOS_LLM_PROXY_URL before standard HTTPS_PROXY/HTTP_PROXY env vars", async () => {
    const { fetchWithProxy, resolveProxyUrl } = await import("../utils/proxy-fetch.js");
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);

    const env = {
      INKOS_LLM_PROXY_URL: "http://inkos-proxy:9910",
      HTTPS_PROXY: "http://standard-proxy:9910",
      HTTP_PROXY: "http://http-proxy:9910",
    };

    expect(resolveProxyUrl(undefined, env)).toBe("http://inkos-proxy:9910");
    await fetchWithProxy("https://api.example/v1/models", {}, undefined, env);

    expect(proxyAgentMock).toHaveBeenCalledWith("http://inkos-proxy:9910");
    expect(fetchMock).not.toHaveBeenCalled();
    expect(undiciFetchMock).toHaveBeenCalledWith(
      "https://api.example/v1/models",
      expect.objectContaining({
        dispatcher: { kind: "proxy-agent", url: "http://inkos-proxy:9910" },
      }),
    );
  });

  it("does not attach a dispatcher when no proxy is configured", async () => {
    const { fetchWithProxy, resolveProxyUrl } = await import("../utils/proxy-fetch.js");
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);

    expect(resolveProxyUrl(undefined, {})).toBeUndefined();
    await fetchWithProxy("https://api.example/v1/models", { headers: { Authorization: "Bearer test" } }, undefined, {});

    expect(proxyAgentMock).not.toHaveBeenCalled();
    expect(undiciFetchMock).not.toHaveBeenCalled();
    expect(fetchMock).toHaveBeenCalledWith(
      "https://api.example/v1/models",
      { headers: { Authorization: "Bearer test" } },
    );
  });

  // 1572 号同族回归锁：回环目标恒旁路代理（本地 Ollama/LM Studio 与 HTTPS_PROXY
  // 共存时，代理无法回连本机回环端口 → 本地推理端点恒挂）。Rust 腿先例见
  // src-tauri supervisor::health_probe 文档。
  it("bypasses proxy for 127.0.0.1 loopback target even with proxy env set", async () => {
    const { fetchWithProxy } = await import("../utils/proxy-fetch.js");
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);

    const env = { HTTPS_PROXY: "http://standard-proxy:9910" };
    await fetchWithProxy("http://127.0.0.1:11434/v1/models", {}, undefined, env);

    expect(proxyAgentMock).not.toHaveBeenCalled();
    expect(undiciFetchMock).not.toHaveBeenCalled();
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:11434/v1/models",
      expect.not.objectContaining({ dispatcher: expect.anything() }),
    );
  });

  it("bypasses proxy for localhost and IPv6/unspecified targets including URL-object and explicit-proxy forms", async () => {
    const { fetchWithProxy } = await import("../utils/proxy-fetch.js");
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);

    const env = { HTTPS_PROXY: "http://standard-proxy:9910" };
    for (const input of [
      "http://localhost:1234/v1/models",
      "http://[::1]:11434/v1/models",
      "http://[::]:9200/v1/models", // 未指定 IPv6（1573 扩）
      "http://0.0.0.0:11434/v1/models", // 未指定地址（1573 扩）
      new URL("http://localhost:4567/api/v1/health"),
    ]) {
      await fetchWithProxy(input, {}, "http://explicit-proxy:9910", env);
    }

    expect(proxyAgentMock).not.toHaveBeenCalled();
    expect(undiciFetchMock).not.toHaveBeenCalled();
    expect(fetchMock).toHaveBeenCalledTimes(5);
  });

  it("still proxies non-loopback private addresses (bypass scope is loopback-only)", async () => {
    const { fetchWithProxy } = await import("../utils/proxy-fetch.js");
    const fetchMock = vi.fn().mockResolvedValue({ ok: true });
    vi.stubGlobal("fetch", fetchMock);

    const env = { HTTPS_PROXY: "http://standard-proxy:9910" };
    await fetchWithProxy("http://192.168.1.10:8080/v1/models", {}, undefined, env);

    expect(fetchMock).not.toHaveBeenCalled();
    expect(proxyAgentMock).toHaveBeenCalledWith("http://standard-proxy:9910");
    expect(undiciFetchMock).toHaveBeenCalledWith(
      "http://192.168.1.10:8080/v1/models",
      expect.objectContaining({
        dispatcher: { kind: "proxy-agent", url: "http://standard-proxy:9910" },
      }),
    );
  });

  // 1573 号：buildProxyFetchInit 契约保持（导出面稳定，直连消费者可用）。
  it("buildProxyFetchInit keeps its exported contract (dispatcher attached only with proxy)", async () => {
    const { buildProxyFetchInit } = await import("../utils/proxy-fetch.js");
    const plain = buildProxyFetchInit({ method: "POST" }, undefined, {});
    expect(plain).toEqual({ method: "POST" });

    const proxied = buildProxyFetchInit({ method: "POST" }, "http://p:9910", {});
    expect(proxyAgentMock).toHaveBeenCalledWith("http://p:9910");
    expect(proxied.dispatcher).toEqual({ kind: "proxy-agent", url: "http://p:9910" });
  });
});
