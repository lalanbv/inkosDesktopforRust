import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
  ASSET_LIBRARY_VERSION,
  GENRE_BASE_SEEDS,
  PROGRESSION_MODE_SEEDS,
  mergeAssetLibrary,
  sortAssets,
  validateLibraryAsset,
  type AssetKind,
  type AssetMergeResult,
  type LibraryAsset,
} from "@actalk/inkos-core";

/**
 * R4/363 号：三库存储层（项目级 `.inkos/asset-library/{kind}.json`）。
 *
 * - 每库一文件，形状 `{version, kind, assets[]}`；
 * - 文件缺失/损坏 → 返回内置种子（内存态，`seeded:true`），不自动落盘——
 *   用户数据文件只在首次写入时创建；
 * - 写入走 merge 幂等（362 号契约），直接全量替换落盘由 saveAssets 承担。
 */

export const LIBRARY_KINDS = ["genre-base", "progression-mode", "world-sample"] as const;
export type LibraryKind = (typeof LIBRARY_KINDS)[number];

export function isLibraryKind(value: string): value is LibraryKind {
  return (LIBRARY_KINDS as ReadonlyArray<string>).includes(value);
}

function seedsFor(kind: LibraryKind): ReadonlyArray<LibraryAsset> {
  if (kind === "genre-base") return GENRE_BASE_SEEDS;
  if (kind === "progression-mode") return PROGRESSION_MODE_SEEDS;
  return [];
}

function libraryPath(root: string, kind: LibraryKind): string {
  return join(root, ".inkos", "asset-library", `${kind}.json`);
}

export interface LibrarySnapshot {
  readonly kind: LibraryKind;
  readonly assets: ReadonlyArray<LibraryAsset>;
  /** true = 文件缺失/损坏，当前为内置种子（内存态）。 */
  readonly seeded: boolean;
}

export async function listAssets(root: string, kind: LibraryKind): Promise<LibrarySnapshot> {
  try {
    const raw = await readFile(libraryPath(root, kind), "utf-8");
    const parsed = JSON.parse(raw) as { version?: unknown; kind?: unknown; assets?: unknown };
    if (
      parsed.version === ASSET_LIBRARY_VERSION
      && parsed.kind === kind
      && Array.isArray(parsed.assets)
    ) {
      const assets: LibraryAsset[] = [];
      for (const item of parsed.assets) {
        const { asset } = validateLibraryAsset(item);
        if (asset) assets.push(asset);
      }
      return { kind, assets: sortAssets(assets), seeded: false };
    }
  } catch {
    // 文件缺失或损坏 → 种子兜底。
  }
  return { kind, assets: sortAssets([...seedsFor(kind)]), seeded: true };
}

export async function saveAssets(root: string, kind: LibraryKind, assets: ReadonlyArray<LibraryAsset>): Promise<void> {
  const path = libraryPath(root, kind);
  await mkdir(join(path, ".."), { recursive: true });
  const payload = { version: ASSET_LIBRARY_VERSION, kind, assets: sortAssets([...assets]) };
  await writeFile(path, JSON.stringify(payload, null, 2), "utf-8");
}

/** 单条资产 upsert：validate → merge（幂等）→ 落盘。 */
export async function upsertAsset(
  root: string,
  kind: LibraryKind,
  raw: unknown,
): Promise<{ result?: AssetMergeResult & { kind: LibraryKind; seeded: boolean }; errors?: ReadonlyArray<string> }> {
  const { asset, errors } = validateLibraryAsset(raw);
  if (!asset) return { errors };
  const snapshot = await listAssets(root, kind);
  const merged = mergeAssetLibrary(snapshot.assets, [asset]);
  await saveAssets(root, kind, merged.merged);
  return { result: { ...merged, kind, seeded: false } };
}

/** 删除资产：只作用于已落盘数据（内置种子不可删）。 */
export async function deleteAsset(
  root: string,
  kind: LibraryKind,
  id: string,
): Promise<{ ok: boolean; reason?: string; assets?: ReadonlyArray<LibraryAsset> }> {
  const snapshot = await listAssets(root, kind);
  if (snapshot.seeded) {
    return { ok: false, reason: "builtin seeds cannot be deleted; save the library first" };
  }
  const remaining = snapshot.assets.filter((asset) => asset.id !== id);
  if (remaining.length === snapshot.assets.length) {
    return { ok: false, reason: `asset not found: ${id}` };
  }
  await saveAssets(root, kind, remaining);
  return { ok: true, assets: remaining };
}

/** kind 字符串 → AssetKind（存储层值域与契约枚举一致）。 */
export function toAssetKind(kind: LibraryKind): AssetKind {
  return kind as AssetKind;
}
