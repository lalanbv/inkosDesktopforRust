//! R4/363 号：三库存储层单测（临时目录 roundtrip + 种子兜底 + 幂等 upsert + 删除守卫）。
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  deleteAsset,
  isLibraryKind,
  listAssets,
  upsertAsset,
} from "./asset-library-store";

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "inkos-asset-library-"));
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

const genreAsset = {
  id: "urban_supernatural",
  kind: "genre-base",
  name: "都市异能",
  body: "现代都市背景下的超凡力量体系。",
};

describe("asset library store (R4)", () => {
  it("falls back to builtin seeds when file missing", async () => {
    const snapshot = await listAssets(root, "genre-base");
    expect(snapshot.seeded).toBe(true);
    expect(snapshot.assets.length).toBe(3);
    const progression = await listAssets(root, "progression-mode");
    expect(progression.seeded).toBe(true);
    expect(progression.assets.length).toBe(3);
  });

  it("upserts idempotently and persists to disk", async () => {
    // 种子兜底列表已含同 id 资产（body 不同）→ 首次 upsert 走覆盖语义。
    const first = await upsertAsset(root, "genre-base", genreAsset);
    expect(first.result?.overwritten).toBe(1);
    const second = await upsertAsset(root, "genre-base", genreAsset);
    expect(second.result?.skipped).toBe(1);

    const snapshot = await listAssets(root, "genre-base");
    expect(snapshot.seeded).toBe(false);
    expect(snapshot.assets.some((asset) => asset.id === "urban_supernatural")).toBe(true);

    const raw = JSON.parse(readFileSync(join(root, ".inkos", "asset-library", "genre-base.json"), "utf-8"));
    expect(raw.version).toBe(1);
    expect(raw.kind).toBe("genre-base");
  });

  it("overwrites on changed content and rejects invalid assets", async () => {
    await upsertAsset(root, "genre-base", genreAsset);
    const revised = await upsertAsset(root, "genre-base", { ...genreAsset, body: "修订后的正文。" });
    expect(revised.result?.overwritten).toBe(1);
    const snapshot = await listAssets(root, "genre-base");
    expect(snapshot.assets.find((asset) => asset.id === "urban_supernatural")?.body).toBe("修订后的正文。");

    const invalid = await upsertAsset(root, "genre-base", { id: "Bad-ID", kind: "genre-base", name: "坏", body: "x" });
    expect(invalid.errors?.length).toBeGreaterThan(0);
  });

  it("guards deletes on seeds and missing ids", async () => {
    const seededGuard = await deleteAsset(root, "progression-mode", "hook_cycle");
    expect(seededGuard.ok).toBe(false);

    await upsertAsset(root, "world-sample", { id: "ws_one", kind: "world-sample", name: "样本", body: "正文" });
    const missing = await deleteAsset(root, "world-sample", "ws_missing");
    expect(missing.ok).toBe(false);
    const removed = await deleteAsset(root, "world-sample", "ws_one");
    expect(removed.ok).toBe(true);
    expect(removed.assets).toHaveLength(0);
  });

  it("validates kind names", () => {
    expect(isLibraryKind("genre-base")).toBe(true);
    expect(isLibraryKind("other")).toBe(false);
  });
});
