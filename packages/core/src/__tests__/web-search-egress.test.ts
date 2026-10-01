import { describe, expect, it } from "vitest";

import {
  assertPublicEgressHost,
  isBlockedHostLiteral,
  isBlockedIp,
  urlHost,
} from "../utils/web-search.js";

/**
 * Egress host guard（SSRF 初判防线，556 号）——与 engine-rs
 * src/utils/web_search.rs 测试矩阵同水位对偶（离线用例，零网络依赖）。
 */
describe("web-search egress guard", () => {
  it("urlHost extracts authority (userinfo/port/IPv6 stripped)", () => {
    expect(urlHost("http://example.com/a")).toBe("example.com");
    expect(urlHost("https://user:pw@Example.com:8443/x?y#z")).toBe("Example.com");
    expect(urlHost("http://127.0.0.1:3000/")).toBe("127.0.0.1");
    expect(urlHost("http://[::1]:8080/x")).toBe("::1");
    expect(urlHost("ftp://example.com")).toBeNull();
    expect(urlHost("not a url")).toBeNull();
    expect(urlHost("http:///no-host")).toBeNull();
  });

  it("blocks local name literals", () => {
    for (const host of ["localhost", "a.localhost", "svc.local", "LOCALHOST", "host.local."]) {
      expect(isBlockedHostLiteral(host), host).toBe(true);
    }
    expect(isBlockedHostLiteral("example.com")).toBe(false);
    expect(isBlockedHostLiteral("notlocal.io")).toBe(false);
  });

  it("blocks loopback/private/link-local/reserved IPs (categories mirror engine-rs)", () => {
    for (const ip of [
      "127.0.0.1",
      "10.1.2.3",
      "172.16.0.1",
      "172.31.255.255",
      "192.168.1.1",
      "169.254.1.1",
      "0.0.0.0",
      "255.255.255.255",
      "::1",
      "fc00::1",
      "fd12::1",
      "fe80::1",
      "::ffff:10.0.0.1",
      "::ffff:127.0.0.1",
    ]) {
      expect(isBlockedIp(ip), ip).toBe(true);
    }
    for (const ip of ["8.8.8.8", "1.1.1.1", "172.15.0.1", "2606:4700::1111"]) {
      expect(isBlockedIp(ip), ip).toBe(false);
    }
  });

  it("assertPublicEgressHost blocks before any network use", async () => {
    await expect(assertPublicEgressHost("http://127.0.0.1/x")).rejects.toThrow("loopback/private");
    await expect(assertPublicEgressHost("http://localhost/x")).rejects.toThrow("local network");
    await expect(assertPublicEgressHost("http://[::1]:8080/x")).rejects.toThrow("loopback/private");
    await expect(assertPublicEgressHost("ftp://example.com/x")).rejects.toThrow("unsupported URL scheme");
    await expect(assertPublicEgressHost("http://169.254.169.254/latest/meta-data")).rejects.toThrow("loopback/private");
  });

  it("assertPublicEgressHost allows public IP literals offline", async () => {
    // 公网 IP 字面量纯本地判定放行（不发起真实请求）。
    await expect(assertPublicEgressHost("http://8.8.8.8/x")).resolves.toBeUndefined();
  });
});
