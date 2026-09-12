//! R4/362 号：三库资产生态契约 golden 断言（题材基底首批）。
//!
//! 唯一事实源 = `golden/asset-library-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_asset_library_diff.rs` 读同一文件差分。
//! 五组断言：shape 校验（合法/缺正文/坏id）、内容指纹（Python FNV 已知值
//! 锚定）、幂等合并（跳过/覆盖/追加稳定序）、便携包（导出稳定序+容错导入）、
//! 内置题材种子。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  assetContentHash,
  buildAssetLibraryExport,
  GENRE_BASE_SEEDS,
  PROGRESSION_MODE_SEEDS,
  renderAssetGuidanceBlock,
  mergeAssetLibrary,
  parseAssetLibraryImport,
  validateLibraryAsset,
  type LibraryAsset,
} from "../utils/asset-library.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/asset-library-vectors.json"), "utf-8"),
) as {
  validate: Array<{ name: string; input: unknown; valid: boolean; errorField?: string }>;
  hashes: Array<{ name: string; input: Record<string, unknown>; expected: string }>;
  merge: Array<{
    name: string;
    existing: Array<Record<string, unknown>>;
    incoming: Array<Record<string, unknown>>;
    expected: {
      mergedIds: string[];
      mergedBodies?: string[];
      skipped: number;
      overwritten: number;
      added: number;
    };
  }>;
  package: {
    name: string;
    exportInput: Array<Record<string, unknown>>;
    exportOrder: string[];
    importAssets: Array<Record<string, unknown>>;
    importValidCount: number;
    importInvalidCount: number;
    importOrder: string[];
  };
  seeds: { ids: string[]; count: number };
  progressionSeeds: { ids: string[]; count: number };
  guidance: {
    name: string;
    language: "zh" | "en";
    assets: Array<Record<string, unknown>>;
    expected: string;
  };
};

function asAsset(raw: Record<string, unknown>): LibraryAsset {
  return validateLibraryAsset(raw).asset as LibraryAsset;
}

function assetIds(assets: ReadonlyArray<LibraryAsset>): string[] {
  return assets.map((asset) => `${asset.kind}:${asset.id}`);
}

describe("asset library contract (R4)", () => {
  it("validates asset shapes per shared vectors", () => {
    for (const vector of vectors.validate) {
      const result = validateLibraryAsset(vector.input);
      expect(result.errors.length === 0, vector.name).toBe(vector.valid);
      if (vector.errorField) {
        expect(
          result.errors.some((message) => message.startsWith(vector.errorField!)),
          vector.name,
        ).toBe(true);
      }
    }
  });

  it("hashes canonical forms per shared vectors", () => {
    for (const vector of vectors.hashes) {
      const asset = asAsset(vector.input);
      expect(assetContentHash(asset), vector.name).toBe(vector.expected);
    }
  });

  it("merges idempotently per shared vectors", () => {
    for (const vector of vectors.merge) {
      const result = mergeAssetLibrary(
        vector.existing.map(asAsset),
        vector.incoming.map(asAsset),
      );
      expect(assetIds(result.merged), vector.name).toEqual(vector.expected.mergedIds);
      if (vector.expected.mergedBodies) {
        expect(result.merged.map((asset) => asset.body), vector.name)
          .toEqual(vector.expected.mergedBodies);
      }
      expect(result.skipped, vector.name).toBe(vector.expected.skipped);
      expect(result.overwritten, vector.name).toBe(vector.expected.overwritten);
      expect(result.added, vector.name).toBe(vector.expected.added);
    }
  });

  it("exports in stable order and imports tolerantly per shared vectors", () => {
    const pkg = vectors.package;
    const exported = buildAssetLibraryExport(pkg.exportInput.map(asAsset));
    const parsedExport = JSON.parse(exported) as { version: number; assets: LibraryAsset[] };
    expect(assetIds(parsedExport.assets), pkg.name).toEqual(pkg.exportOrder);
    expect(parsedExport.version).toBe(1);

    const importPayload = JSON.stringify({ version: 1, assets: pkg.importAssets });
    const imported = parseAssetLibraryImport(importPayload);
    expect(imported.assets, pkg.name).toHaveLength(pkg.importValidCount);
    expect(assetIds(imported.assets), pkg.name).toEqual(pkg.importOrder);
    expect(imported.errors, pkg.name).toHaveLength(pkg.importInvalidCount);

    // 版本不符整包拒绝。
    const wrongVersion = parseAssetLibraryImport(JSON.stringify({ version: 99, assets: [] }));
    expect(wrongVersion.assets).toHaveLength(0);
    expect(wrongVersion.errors.length).toBeGreaterThan(0);
  });

  it("ships genre-base seeds matching the contract", () => {
    expect(GENRE_BASE_SEEDS).toHaveLength(vectors.seeds.count);
    expect(GENRE_BASE_SEEDS.map((asset) => asset.id).sort()).toEqual([...vectors.seeds.ids].sort());
    for (const seed of GENRE_BASE_SEEDS) {
      expect(validateLibraryAsset(seed).errors, seed.id).toHaveLength(0);
      expect(seed.kind).toBe("genre-base");
    }
  });

  it("renders guidance blocks per shared vectors", () => {
    const vector = vectors.guidance;
    const assets = vector.assets.map(asAsset);
    expect(renderAssetGuidanceBlock(assets, vector.language), vector.name).toBe(vector.expected);
    expect(renderAssetGuidanceBlock([], vector.language)).toBeUndefined();
  });

  it("ships progression-mode seeds matching the contract", () => {
    expect(PROGRESSION_MODE_SEEDS).toHaveLength(vectors.progressionSeeds.count);
    expect(PROGRESSION_MODE_SEEDS.map((asset) => asset.id).sort())
      .toEqual([...vectors.progressionSeeds.ids].sort());
    for (const seed of PROGRESSION_MODE_SEEDS) {
      expect(validateLibraryAsset(seed).errors, seed.id).toHaveLength(0);
      expect(seed.kind).toBe("progression-mode");
    }
  });
});
