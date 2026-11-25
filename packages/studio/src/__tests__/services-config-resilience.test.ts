import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 1666 号：LLM 服务配置族两处。
 * 1. 项目没有 inkos.json——Rust 同族处理器一律 `load_raw_config(…)
 *    .unwrap_or(json!({}))`；Node 腿此前直接 `readFile`，读面/保存/env 导入/封面
 *    配置全 500，而「保存配置」恰是能建出该文件的操作。
 * 2. 内置服务的自定义端点（baseUrl）经归一不丢——此前只有对象形态的内置分支
 *    漏透传（Rust `ServiceConfigEntry` 不分内置与 custom 都保留）。
 */
describe("services config family", () => {
  const roots: string[] = [];
  afterEach(async () => {
    for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true });
  });
  async function freshRoot(): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "inkos-services-config-"));
    roots.push(root);
    return root;
  }
  const json = (method: string, body?: unknown) => ({
    method,
    headers: { "Content-Type": "application/json" },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });

  it("项目没有 inkos.json：服务配置与封面配置读面 200（空配置），保存即建出文件", async () => {
    const root = await freshRoot();
    const app = createStudioServer({} as never, root);

    const before = await app.request("/api/v1/services/config");
    expect(before.status).toBe(200);
    expect(await before.json()).toMatchObject({ services: [], service: null, defaultModel: null });
    const cover = await app.request("/api/v1/cover/config");
    expect(cover.status).toBe(200);
    expect(await cover.json()).toMatchObject({ service: null, configured: false });

    const proxy = { baseUrl: "https://proxy.example.com/v1", models: ["m1"] };
    const saved = await app.request("/api/v1/services/config", json("PUT", {
      services: { "custom:Proxy": proxy }, service: "custom:Proxy", defaultModel: "m1",
    }));
    expect(saved.status).toBe(200);
    const entry = { service: "custom", name: "Proxy", ...proxy };
    expect(JSON.parse(await readFile(join(root, "inkos.json"), "utf-8")).llm).toMatchObject({
      services: [entry], service: "custom:Proxy", defaultModel: "m1",
    });
    expect(await (await app.request("/api/v1/services/config")).json()).toMatchObject({
      services: [entry], service: "custom:Proxy", defaultModel: "m1",
    });
  });

  it("inkos.json 损坏：读面降级为空配置，写入被拒且不覆盖坏文件（全表见 config-file-state.test.ts）", async () => {
    const root = await freshRoot();
    const broken = "{ not json";
    await writeFile(join(root, "inkos.json"), broken);
    const app = createStudioServer({} as never, root);

    const degraded = await app.request("/api/v1/services/config");
    expect(degraded.status).toBe(200);
    expect(await degraded.json()).toMatchObject({ services: [], service: null, defaultModel: null });
    expect((await app.request("/api/v1/services/config", json("PUT", { defaultModel: "m1" }))).status).toBe(500);
    expect((await app.request("/api/v1/cover/config", json("PUT", { service: "kkaiapi" }))).status).toBe(500);
    expect(await readFile(join(root, "inkos.json"), "utf-8")).toBe(broken);
  });

  it("内置服务的自定义端点经归一不丢：读面可见、保存后仍在、顶层镜像取它", async () => {
    const root = await freshRoot();
    const proxy = { baseUrl: "https://proxy.example.com/v1", models: ["m1"] };
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "x", version: "0.1.0", language: "zh",
      llm: { services: { openai: proxy }, service: "openai", defaultModel: "m1" },
    }));
    const app = createStudioServer({} as never, root);

    const read = await (await app.request("/api/v1/services/config")).json();
    expect(read.services).toEqual([{ service: "openai", ...proxy }]);

    // 再存一个别的服务：既有内置条目（含端点）并入新清单原样保留，顶层镜像
    // （仍选 openai）取条目端点而非预设端点。
    expect((await app.request("/api/v1/services/config", json("PUT", {
      services: { "custom:Other": { baseUrl: "https://other.example.com/v1" } }, defaultModel: "m2",
    }))).status).toBe(200);
    const stored = JSON.parse(await readFile(join(root, "inkos.json"), "utf-8"));
    expect(stored.llm.services).toEqual([
      { service: "openai", ...proxy },
      { service: "custom", name: "Other", baseUrl: "https://other.example.com/v1" },
    ]);
    expect(stored.llm.baseUrl).toBe(proxy.baseUrl);
    expect(stored.llm.defaultModel).toBe("m2");
  });
});
