import { describe, expect, it, vi } from "vitest";
import {
  assertCoverDownloadUrlAllowed,
  resolveCoverDownloadOriginPolicy,
  type CoverHostLookup,
} from "../pipeline/cover-download-origin.js";

/**
 * 900 号：封面下载站 origin 策略单元面。模型返回 URL（不可信）发送前
 * 校验：协议仅 http/https、同 provider origin/显式 allowlist 放行、其余
 * host 环回/私网/保留地址与 DNS 解析到私网（rebinding 常见形）拒绝、
 * strict 收紧与 invalid fail-closed。DNS 一律注入（不触真网）。
 */

const PROVIDER = "http://127.0.0.1:9000/v1";
const defaultPolicy = resolveCoverDownloadOriginPolicy({});
const strictPolicy = resolveCoverDownloadOriginPolicy({ INKOS_COVER_DOWNLOAD_ORIGIN_POLICY: "strict" });

const publicLookup: CoverHostLookup = async () => [{ address: "93.184.216.34", family: 4 }];
const loopbackLookup: CoverHostLookup = async () => [{ address: "127.0.0.1", family: 4 }];

function countingLookup(records: Array<{ address: string; family: number }>): CoverHostLookup & { calls: string[] } {
  const lookup = Object.assign(async (hostname: string) => {
    lookup.calls.push(hostname);
    return records;
  }, { calls: [] as string[] });
  return lookup;
}

describe("resolveCoverDownloadOriginPolicy", () => {
  it("defaults to lenient mode with empty allowlist", () => {
    expect(resolveCoverDownloadOriginPolicy({})).toEqual({ mode: "default", extraOrigins: [] });
  });

  it("accepts strict regardless of case or surrounding whitespace", () => {
    expect(resolveCoverDownloadOriginPolicy({ INKOS_COVER_DOWNLOAD_ORIGIN_POLICY: "  STRICT " }).mode).toBe("strict");
  });

  it("marks unknown values invalid so enforcement fails closed", () => {
    expect(resolveCoverDownloadOriginPolicy({ INKOS_COVER_DOWNLOAD_ORIGIN_POLICY: "banana" })).toEqual({
      mode: "invalid",
      invalidValue: "banana",
      extraOrigins: [],
    });
  });

  it("parses the comma separated extra origins allowlist", () => {
    expect(resolveCoverDownloadOriginPolicy({
      INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS: " http://a.example , https://b.example ",
    }).extraOrigins).toEqual(["http://a.example", "https://b.example"]);
  });
});

describe("assertCoverDownloadUrlAllowed scheme and trust anchors", () => {
  it("allows only http and https schemes", async () => {
    await expect(assertCoverDownloadUrlAllowed("file:///etc/passwd", PROVIDER, defaultPolicy)).rejects.toMatchObject({
      code: "COVER_DOWNLOAD_ORIGIN_BLOCKED",
      reason: "scheme-not-allowed",
    });
    await expect(assertCoverDownloadUrlAllowed("data:image/png;base64,AAAA", PROVIDER, defaultPolicy)).rejects.toMatchObject({
      reason: "scheme-not-allowed",
    });
    await expect(assertCoverDownloadUrlAllowed("ftp://8.8.8.8/x.png", PROVIDER, defaultPolicy)).rejects.toMatchObject({
      reason: "scheme-not-allowed",
    });
  });

  it("keeps the provider origin trusted even when it is a loopback gateway (900: 本地网关场景)", async () => {
    // 同 origin 直接放行且不触 DNS（本地 provider 返回同 origin 图片 URL）。
    await expect(assertCoverDownloadUrlAllowed("http://127.0.0.1:9000/img.png", PROVIDER, defaultPolicy)).resolves.toBeUndefined();
  });

  it("rejects unparsable urls with a labeled reason", async () => {
    await expect(assertCoverDownloadUrlAllowed("notaurl", PROVIDER, defaultPolicy)).rejects.toMatchObject({
      reason: "unparsable-url",
    });
  });
});

describe("assertCoverDownloadUrlAllowed private and reserved hosts", () => {
  const blocked = [
    "http://127.0.0.1/img.png",
    "http://10.0.0.1/img.png",
    "http://192.168.1.5/img.png",
    "http://172.16.0.9/img.png",
    "http://172.31.255.254/img.png",
    "http://169.254.169.254/latest/meta-data",
    "http://100.64.0.1/img.png",
    "http://0.1.2.3/img.png",
    "http://198.18.0.1/img.png",
    "http://224.0.0.1/img.png",
    "http://240.0.0.1/img.png",
    "http://255.255.255.255/img.png",
  ];
  it.each(blocked)("blocks private or reserved address %s before any request", async (url) => {
    await expect(assertCoverDownloadUrlAllowed(url, PROVIDER, defaultPolicy)).rejects.toMatchObject({
      code: "COVER_DOWNLOAD_ORIGIN_BLOCKED",
      reason: "private-or-reserved-address",
    });
  });

  it("allows public address literals without touching DNS", async () => {
    await expect(assertCoverDownloadUrlAllowed("http://8.8.8.8/img.png", PROVIDER, defaultPolicy)).resolves.toBeUndefined();
    await expect(assertCoverDownloadUrlAllowed("http://[2606:4700::1111]/img.png", PROVIDER, defaultPolicy)).resolves.toBeUndefined();
  });

  it("blocks non-literal encodings that WHATWG normalization resolves to loopback", async () => {
    // 2130706433 与 0x7f000001 均为 127.0.0.1 的十进制/十六进制变形。
    await expect(assertCoverDownloadUrlAllowed("http://2130706433/img.png", PROVIDER, defaultPolicy)).rejects.toMatchObject({
      reason: "private-or-reserved-address",
    });
    await expect(assertCoverDownloadUrlAllowed("http://0x7f000001/img.png", PROVIDER, defaultPolicy)).rejects.toMatchObject({
      reason: "private-or-reserved-address",
    });
  });

  it.each([
    "http://[::1]/img.png",
    "http://[::]/img.png",
    "http://[fc00::1]/img.png",
    "http://[fd00::1]/img.png",
    "http://[fe80::1]/img.png",
    "http://[::ffff:10.0.0.1]/img.png",
    "http://[::ffff:7f00:1]/img.png",
    "http://[2001:db8::1]/img.png",
  ])("blocks private or reserved IPv6 %s", async (url) => {
    await expect(assertCoverDownloadUrlAllowed(url, PROVIDER, defaultPolicy)).rejects.toMatchObject({
      reason: "private-or-reserved-address",
    });
  });
});

describe("assertCoverDownloadUrlAllowed hostnames via injected DNS", () => {
  it("blocks public-looking hostnames whose resolution lands on a private address (rebinding 常见形)", async () => {
    const lookup = countingLookup([{ address: "127.0.0.1", family: 4 }]);
    await expect(assertCoverDownloadUrlAllowed("http://cdn.rebind.example/img.png", PROVIDER, defaultPolicy, lookup))
      .rejects.toMatchObject({ reason: "resolve-to-private-address" });
    expect(lookup.calls).toEqual(["cdn.rebind.example"]);
  });

  it("blocks when any resolved record is private even if another is public", async () => {
    const lookup = countingLookup([{ address: "93.184.216.34", family: 4 }, { address: "10.0.0.1", family: 4 }]);
    await expect(assertCoverDownloadUrlAllowed("http://cdn.example/img.png", PROVIDER, defaultPolicy, lookup))
      .rejects.toMatchObject({ reason: "resolve-to-private-address" });
  });

  it("fails closed when resolution errors or returns nothing", async () => {
    await expect(assertCoverDownloadUrlAllowed("http://cdn.example/img.png", PROVIDER, defaultPolicy, async () => {
      throw new Error("serpdwn");
    })).rejects.toMatchObject({ reason: "resolve-failed" });
    await expect(assertCoverDownloadUrlAllowed("http://cdn.example/img.png", PROVIDER, defaultPolicy, async () => []))
      .rejects.toMatchObject({ reason: "resolve-failed" });
  });

  it("allows hostnames that resolve to public addresses only", async () => {
    await expect(assertCoverDownloadUrlAllowed("http://cdn.example/img.png", PROVIDER, defaultPolicy, publicLookup)).resolves.toBeUndefined();
  });

  it("blocks loopback, mDNS and single-label hostnames before DNS is consulted", async () => {
    for (const [hostname, reason] of [
      ["localhost", "loopback-host"],
      ["box.local", "mdns-host"],
      ["intranet", "single-label-host"],
    ] as const) {
      const lookup = countingLookup([{ address: "93.184.216.34", family: 4 }]);
      await expect(assertCoverDownloadUrlAllowed(`http://${hostname}/img.png`, PROVIDER, defaultPolicy, lookup))
        .rejects.toMatchObject({ reason });
      expect(lookup.calls).toEqual([]);
    }
    // 「localhost.example」是公网域名形态，必须走 DNS 而非误伤。
    await expect(assertCoverDownloadUrlAllowed("http://localhost.example/img.png", PROVIDER, defaultPolicy, publicLookup)).resolves.toBeUndefined();
  });
});

describe("assertCoverDownloadUrlAllowed allowlist and strict mode", () => {
  it("lets explicitly allowlisted origins bypass the private host check", async () => {
    const policy = resolveCoverDownloadOriginPolicy({ INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS: "http://127.0.0.1:9999" });
    await expect(assertCoverDownloadUrlAllowed("http://127.0.0.1:9999/img.png", PROVIDER, policy)).resolves.toBeUndefined();
    await expect(assertCoverDownloadUrlAllowed("http://127.0.0.1:9998/img.png", PROVIDER, policy)).rejects.toMatchObject({
      reason: "private-or-reserved-address",
    });
  });

  it("strict mode rejects public cross origin urls while keeping same origin and allowlist intact", async () => {
    await expect(assertCoverDownloadUrlAllowed("http://8.8.8.8/img.png", PROVIDER, strictPolicy)).rejects.toMatchObject({
      reason: "strict-cross-origin",
    });
    await expect(assertCoverDownloadUrlAllowed("http://127.0.0.1:9000/img.png", PROVIDER, strictPolicy)).resolves.toBeUndefined();
    const allowlistedStrict = resolveCoverDownloadOriginPolicy({
      INKOS_COVER_DOWNLOAD_ORIGIN_POLICY: "strict",
      INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS: "http://127.0.0.1:9999",
    });
    await expect(assertCoverDownloadUrlAllowed("http://127.0.0.1:9999/img.png", PROVIDER, allowlistedStrict)).resolves.toBeUndefined();
  });

  it("treats an invalid policy value as fail closed even for public hosts", async () => {
    const invalid = resolveCoverDownloadOriginPolicy({ INKOS_COVER_DOWNLOAD_ORIGIN_POLICY: "banana" });
    await expect(assertCoverDownloadUrlAllowed("http://8.8.8.8/img.png", PROVIDER, invalid)).rejects.toMatchObject({
      reason: "invalid-policy",
    });
  });
});

describe("downloadGeneratedCoverImage integration shape (900)", () => {
  it("pre-send policy runs before the request and final-url recheck runs after redirects", async () => {
    // 经由真实 generateImageFromPrompt 链路已在 cover-model-reference.flow
    // 套件覆盖（loopback 拒绝/重定向终态拒绝/allowlist 放行三面）；此处
    // 只锚定 resolve 装配面与 env 键名契约。
    vi.stubEnv("INKOS_COVER_DOWNLOAD_ORIGIN_POLICY", "strict");
    try {
      expect(resolveCoverDownloadOriginPolicy().mode).toBe("strict");
    } finally {
      vi.unstubAllEnvs();
    }
  });
});
