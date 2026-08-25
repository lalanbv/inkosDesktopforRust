import { describe, expect, it } from "vitest";
import { Hono } from "hono";
import {
  createLoopbackGuardMiddleware,
  guardOptionsFromEnv,
  hostWithoutPort,
  isLoopbackHost,
  originIsAllowed,
  parseGuardEnabled,
} from "./loopback-guard.js";

describe("hostWithoutPort", () => {
  it("strips ports for IPv4/hostname and keeps IPv6 brackets", () => {
    expect(hostWithoutPort("localhost:8787")).toBe("localhost");
    expect(hostWithoutPort("127.0.0.1")).toBe("127.0.0.1");
    expect(hostWithoutPort("[::1]:8787")).toBe("[::1]");
    expect(hostWithoutPort("[::1]")).toBe("[::1]");
    expect(hostWithoutPort("example.com:80")).toBe("example.com");
  });
});

describe("isLoopbackHost", () => {
  it("accepts loopback literals and rejects others", () => {
    for (const host of ["localhost", "LOCALHOST", "127.0.0.1", "[::1]", "[::]", "::1"]) {
      expect(isLoopbackHost(host)).toBe(true);
    }
    for (const host of ["example.com", "127.0.0.2", "0.0.0.0", ""]) {
      expect(isLoopbackHost(host)).toBe(false);
    }
  });
});

describe("originIsAllowed", () => {
  it("allows loopback hosts with any port/scheme and tauri origins", () => {
    for (const origin of [
      "http://localhost:4567",
      "http://127.0.0.1:7788",
      "https://localhost",
      "tauri://localhost",
      "http://[::1]:9000",
    ]) {
      expect(originIsAllowed(origin, [])).toBe(true);
    }
  });

  it("rejects remote, null and malformed origins", () => {
    for (const origin of [
      "http://evil.com",
      "https://attacker.example:443",
      "null",
      "",
      "http://127.0.0.2:8080",
    ]) {
      expect(originIsAllowed(origin, [])).toBe(false);
    }
  });

  it("honors exact-match extra allowlist", () => {
    const extras = ["https://custom.example"];
    expect(originIsAllowed("https://custom.example", extras)).toBe(true);
    expect(originIsAllowed("https://custom.example.evil", extras)).toBe(false);
  });
});

describe("parseGuardEnabled / guardOptionsFromEnv", () => {
  it("defaults to enabled and understands disable tokens", () => {
    expect(parseGuardEnabled(undefined)).toBe(true);
    expect(parseGuardEnabled("1")).toBe(true);
    expect(parseGuardEnabled("")).toBe(true);
    for (const off of ["0", "false", "off", "OFF", " false "]) {
      expect(parseGuardEnabled(off)).toBe(false);
    }
  });

  it("parses extra origins from comma list with blank trimming", () => {
    const options = guardOptionsFromEnv({
      INKOS_ENGINE_LOOPBACK_GUARD: "0",
      INKOS_ENGINE_ALLOWED_ORIGINS: " https://a.example , ,http://b.example ",
    });
    expect(options.enabled).toBe(false);
    expect(options.extraOrigins).toEqual(["https://a.example", "http://b.example"]);
  });
});

describe("createLoopbackGuardMiddleware (Hono integration)", () => {
  function app(options?: Parameters<typeof createLoopbackGuardMiddleware>[0]) {
    const hono = new Hono();
    hono.use("/*", createLoopbackGuardMiddleware(options));
    hono.use("/*", async (c, next) => {
      await next();
      c.header("access-control-allow-origin", "*");
    });
    hono.get("/api/v1/health", (c) => c.json({ ok: true }));
    return hono;
  }

  it("returns 403 for remote origin without CORS headers", async () => {
    const response = await app().request("http://localhost/api/v1/health", {
      headers: { origin: "http://evil.com" },
    });
    expect(response.status).toBe(403);
    expect(response.headers.get("access-control-allow-origin")).toBeNull();
    await expect(response.json()).resolves.toEqual({ error: "origin not allowed" });
  });

  it("allows loopback origins with any port and tauri scheme", async () => {
    for (const origin of ["http://localhost:4567", "http://127.0.0.1:7788", "tauri://localhost"]) {
      const response = await app().request("http://localhost/api/v1/health", {
        headers: { origin },
      });
      expect(response.status).toBe(200);
    }
  });

  it("rejects null origin", async () => {
    const response = await app().request("http://localhost/api/v1/health", {
      headers: { origin: "null" },
    });
    expect(response.status).toBe(403);
  });

  it("rejects rebinding-style host header", async () => {
    // app.request 测试形态不自动从 URL 推导 Host（真实 @hono/node-server 才有），
    // 显式注入以模拟 DNS rebinding 请求形态。
    const response = await app().request("http://localhost/api/v1/health", {
      headers: { host: "attacker.com:8787" },
    });
    expect(response.status).toBe(403);
  });

  it("allows loopback hosts and header-less requests", async () => {
    for (const host of ["localhost:8787", "127.0.0.1:8787", "[::1]:8787"]) {
      const response = await app().request("http://localhost/api/v1/health", {
        headers: { host },
      });
      expect(response.status).toBe(200);
    }
    const direct = await app().request("http://127.0.0.1:8787/api/v1/health");
    expect(direct.status).toBe(200);
  });

  it("passes everything when disabled", async () => {
    const response = await app({ enabled: false }).request("http://evil.com/api/v1/health", {
      headers: { origin: "http://evil.com" },
    });
    expect(response.status).toBe(200);
  });

  it("honors extra origin allowlist", async () => {
    const response = await app({ extraOrigins: ["https://embed.example"] }).request(
      "http://localhost/api/v1/health",
      { headers: { origin: "https://embed.example" } },
    );
    expect(response.status).toBe(200);
  });
});
