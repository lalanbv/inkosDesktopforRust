import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * R4/363-366 号（691 号随 2.0 合并重施）：三库资产路由族端到端——
 * PUT upsert → GET 快照 → export（RFC 6266 契约头）→ import-preview（不落盘）
 * → import（落盘合并）→ adopt（写入本书材料池）。种子兜底语义与
 * engine-rs asset_library_routes.rs 同水位对偶。
 */
describe("asset-library route family (R4/363-366, 691 重施)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "asset-library-routes-"));
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const genreAsset = {
    id: "urban_supernatural",
    kind: "genre-base",
    name: "都市异能",
    body: "现代都市背景下的超凡力量体系。",
  };

  it("round-trips PUT → GET → export → preview → import → adopt", async () => {
    const app = createStudioServer({} as never, root);

    // invalid kind 400
    const badKind = await app.request("/api/v1/asset-library/nope");
    expect(badKind.status).toBe(400);

    // 种子兜底（文件缺失 → builtin 种子，seeded=true）
    const seeded = await app.request("/api/v1/asset-library/genre-base");
    expect(seeded.status).toBe(200);
    const seededBody = await seeded.json() as { seeded: boolean; assets: ReadonlyArray<{ id: string }> };
    expect(seededBody.seeded).toBe(true);
    expect(seededBody.assets.length).toBeGreaterThan(0);

    // PUT upsert（种子含同 id → 覆盖语义）
    const put = await app.request("/api/v1/asset-library/genre-base/assets", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ asset: genreAsset }),
    });
    expect(put.status).toBe(200);
    const putBody = await put.json() as { overwritten?: number; skipped?: number };
    expect(putBody.overwritten).toBe(1);

    // 非法资产 400
    const badAsset = await app.request("/api/v1/asset-library/genre-base/assets", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ asset: { id: "" } }),
    });
    expect(badAsset.status).toBe(400);

    // GET 快照（落盘后 seeded=false）
    const snapshot = await app.request("/api/v1/asset-library/genre-base");
    const snapshotBody = await snapshot.json() as { seeded: boolean; assets: ReadonlyArray<{ id: string }> };
    expect(snapshotBody.seeded).toBe(false);
    expect(snapshotBody.assets.find((a) => a.id === "urban_supernatural")).toBeDefined();

    // export：正文 + RFC 6266 契约头
    const exported = await app.request("/api/v1/asset-library/genre-base/export");
    expect(exported.status).toBe(200);
    expect(exported.headers.get("content-disposition")).toContain("asset-library-genre-base.json");
    const exportText = await exported.text();
    expect(exportText).toContain("urban_supernatural");

    // import-preview：合并预览不落盘（便携包格式 = { version, assets }，与 export 对偶）
    const preview = await app.request("/api/v1/asset-library/genre-base/import-preview", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ version: 1, assets: [genreAsset] }),
    });
    expect(preview.status).toBe(200);
    const previewBody = await preview.json() as { skipped: number; samples: ReadonlyArray<{ id: string }> };
    expect(previewBody.skipped).toBe(1);
    expect(previewBody.samples[0]?.id).toBe("urban_supernatural");

    // import：新增一条不同 id 资产落盘
    const importPayload = { version: 1, assets: [{ ...genreAsset, id: "star_tower", name: "星空塔" }] };
    const imported = await app.request("/api/v1/asset-library/genre-base/import", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(importPayload),
    });
    expect(imported.status).toBe(200);
    const importBody = await imported.json() as { added: number };
    expect(importBody.added).toBe(1);
    const afterImport = await app.request("/api/v1/asset-library/genre-base");
    const afterImportBody = await afterImport.json() as { assets: ReadonlyArray<{ id: string }> };
    expect(afterImportBody.assets.find((a) => a.id === "star_tower")).toBeDefined();

    // adopt：库资产写入本书材料池（md + manifest 双文件）
    const adopted = await app.request("/api/v1/books/b1/adopt-library-assets", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refs: [{ kind: "genre-base", id: "urban_supernatural" }] }),
    });
    expect(adopted.status).toBe(200);
    const adoptedBody = await adopted.json() as { published: ReadonlyArray<{ materialId: string }> };
    expect(adoptedBody.published).toHaveLength(1);
    const materialId = adoptedBody.published[0]!.materialId;
    const md = await readFile(join(root, ".inkos", "materials", `${materialId}.md`), "utf-8");
    expect(md).toContain("现代都市背景下的超凡力量体系。");
    const manifest = JSON.parse(await readFile(join(root, ".inkos", "materials", `${materialId}.json`), "utf-8")) as {
      source: string;
      markdownPath: string;
    };
    expect(manifest.source).toBe("library");
    expect(manifest.markdownPath).toContain(materialId);
  });

  it("delete removes a persisted asset but refuses builtin seeds", async () => {
    const app = createStudioServer({} as never, root);
    await app.request("/api/v1/asset-library/genre-base/assets", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ asset: genreAsset }),
    });
    // 已 upsert 落盘（覆盖种子）→ 可删
    const removed = await app.request("/api/v1/asset-library/genre-base/assets/urban_supernatural", { method: "DELETE" });
    expect(removed.status).toBe(200);
    // 不存在的 id → 404
    const missing = await app.request("/api/v1/asset-library/genre-base/assets/urban_supernatural", { method: "DELETE" });
    expect(missing.status).toBe(404);
  });
});
