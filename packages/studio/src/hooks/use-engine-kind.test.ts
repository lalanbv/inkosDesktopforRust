// @vitest-environment jsdom
//! 1668：服务腿判型——probeEngineKind 四格矩阵 + 整页共享探测。
import { afterEach, describe, expect, it, vi } from "vitest";
import { engineKind, probeEngineKind, resetEngineKindCache } from "./use-engine-kind";

const response = (status: number) => ({ ok: status >= 200 && status < 300, status }) as Response;

afterEach(() => {
  resetEngineKindCache();
});

describe("probeEngineKind（1668 号）", () => {
  it("health 200 = 产品引擎；404 = Node 回退腿", async () => {
    expect(await probeEngineKind(vi.fn(async () => response(200)))).toBe("rust");
    expect(await probeEngineKind(vi.fn(async () => response(404)))).toBe("node");
  });

  it("其它状态与网络错误不下结论——判错型比不标更糟", async () => {
    expect(await probeEngineKind(vi.fn(async () => response(500)))).toBe("unknown");
    expect(await probeEngineKind(vi.fn(async () => response(403)))).toBe("unknown");
    expect(await probeEngineKind(vi.fn(async () => { throw new TypeError("Failed to parse URL"); }))).toBe("unknown");
  });

  it("探测带 no-store（防中间层缓存假型）且只打 /api/v1/health", async () => {
    const fetchImpl = vi.fn(async () => response(200));
    await probeEngineKind(fetchImpl);
    expect(fetchImpl).toHaveBeenCalledTimes(1);
    expect(fetchImpl).toHaveBeenCalledWith("/api/v1/health", { cache: "no-store" });
  });

  it("整页共享一次探测；重置后重新探测", async () => {
    const fetchImpl = vi.fn(async () => response(404));
    vi.stubGlobal("fetch", fetchImpl);
    try {
      expect(await engineKind()).toBe("node");
      expect(await engineKind()).toBe("node");
      expect(fetchImpl).toHaveBeenCalledTimes(1);
      resetEngineKindCache();
      await engineKind();
      expect(fetchImpl).toHaveBeenCalledTimes(2);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
