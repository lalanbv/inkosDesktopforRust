import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ingestMaterial } from "../materials/ingest.js";

/**
 * 635/689 号：materials ingest SSRF 初判防线接线验证——
 * 缺省拦截回环目标（守卫先于 fetch，fetch 桩被调用即失败）；
 * allowPrivateEgress=true（researchSearch.allowPrivateEgress 透传形态）放行，
 * 与 engine-rs materials.rs 同水位对偶。
 */
describe("ingest_material egress guard (635/689)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "ingest-egress-"));
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const fetchStubThatMustNotRun = async (): Promise<Response> => {
    throw new Error("fetch must not be reached when the egress guard blocks");
  };

  it("blocks loopback URL sources by default before any fetch", async () => {
    await expect(
      ingestMaterial(root, { sourceKind: "url", url: "http://127.0.0.1:9/secret" }, {
        fetch: fetchStubThatMustNotRun,
      }),
    ).rejects.toThrow("Fetch blocked");
  });

  it("blocks private-range literal URL sources before any fetch", async () => {
    await expect(
      ingestMaterial(root, { sourceKind: "url", url: "http://10.1.2.3/x" }, {
        fetch: fetchStubThatMustNotRun,
      }),
    ).rejects.toThrow("Fetch blocked");
  });

  it("allows URL sources with allowPrivateEgress=true (config exemption passthrough)", async () => {
    const html = "<html><body><h1>intranet doc</h1><p>内部知识库正文。</p></body></html>";
    const stub = (async () =>
      new Response(html, { status: 200, headers: { "content-type": "text/html" } })) as unknown as typeof fetch;
    const asset = await ingestMaterial(
      root,
      { sourceKind: "url", url: "http://127.0.0.1:9/intranet-doc" },
      { fetch: stub, allowPrivateEgress: true },
    );
    expect(asset.kind).toBe("webpage");
    expect(asset.source).toBe("http://127.0.0.1:9/intranet-doc");
  });
});
