import { z } from "zod";

/**
 * R4 三库资产生态（362 号契约层首批，二轮 P1；355 号 §4 R4）。
 *
 * 题材基底 / 推进模式 / 世界样本三库共用统一 shape：`{id, kind, name, body,
 * expectations, taboos, samples}`——题材基底=题材写法期望与禁忌；推进模式=
 * 情节推进范式；世界样本=可借鉴的既有世界观样本（导入时落"通用样本≠本书
 * 世界"两段式，随 364 号接线）。导演方向候选与 BookCreate 可选挂载（接线批）。
 *
 * 便携 JSON 包：`{version:1, assets:[...]}`——导出稳定排序（kind+id 码元序），
 * 导入逐条校验、非法条目跳过不阻断；`assetContentHash`（FNV-1a hex16）驱动
 * 幂等合并（同 id 同内容跳过 / 同 id 异内容覆盖）。
 *
 * 双端：`engine-rs/src/utils/asset_library.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/asset-library-vectors.json`（TS 断言
 * `golden-asset-library.test.ts`；Rust 差分 `tests/golden_asset_library_diff.rs`）。
 */

export const ASSET_LIBRARY_VERSION = 1;

export const AssetKindSchema = z.enum(["genre-base", "progression-mode", "world-sample"]);
export type AssetKind = z.infer<typeof AssetKindSchema>;

export const LibraryAssetSchema = z.object({
  /** slug 标识（snake_case，≤64 码元；库内 + kind 内唯一）。 */
  id: z
    .string()
    .min(1)
    .max(64)
    .regex(/^[a-z0-9_]+$/, "id must be snake_case slug"),
  kind: AssetKindSchema,
  name: z.string().min(1).max(80),
  /** 资产正文（写法/范式/样本说明，≤4000 码元）。 */
  body: z.string().min(1).max(4000),
  /** 读者期待清单（题材承诺）。 */
  expectations: z.array(z.string().min(1).max(200)).max(20).default([]),
  /** 禁忌清单（该题材/模式的雷区）。 */
  taboos: z.array(z.string().min(1).max(200)).max(20).default([]),
  /** 示例片段（≤10 段，各 ≤600 码元）。 */
  samples: z.array(z.string().min(1).max(600)).max(10).default([]),
});

export type LibraryAsset = z.infer<typeof LibraryAssetSchema>;

export interface AssetValidateResult {
  readonly asset?: LibraryAsset;
  readonly errors: ReadonlyArray<string>;
}

/** 单条资产校验：zod safeParse 包装，非法返回错误清单（不抛错、不截断）。 */
export function validateLibraryAsset(raw: unknown): AssetValidateResult {
  const parsed = LibraryAssetSchema.safeParse(raw);
  if (parsed.success) {
    return { asset: parsed.data, errors: [] };
  }
  return {
    errors: parsed.error.issues.map(
      (issue) => `${issue.path.join(".") || "(root)"}: ${issue.message}`,
    ),
  };
}

const ARRAY_FIELD_SEP = "\u001f";

/** 内容指纹的 canonical 形式（双端逐字一致；字段含 `|` 不影响确定性）。 */
export function assetCanonicalForm(asset: LibraryAsset): string {
  return [
    asset.kind,
    asset.id,
    asset.name,
    asset.body,
    asset.expectations.join(ARRAY_FIELD_SEP),
    asset.taboos.join(ARRAY_FIELD_SEP),
    asset.samples.join(ARRAY_FIELD_SEP),
  ].join("|");
}

const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const FNV_MASK = 0xffffffffffffffffn;

function fnv1aHex16(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let hash = FNV_OFFSET;
  for (const byte of bytes) {
    hash ^= BigInt(byte);
    hash = (hash * FNV_PRIME) & FNV_MASK;
  }
  return hash.toString(16).padStart(16, "0");
}

/** 资产内容指纹（不含 kind 之外的排序位置；同内容必同指纹——幂等合并依据）。 */
export function assetContentHash(asset: LibraryAsset): string {
  return fnv1aHex16(assetCanonicalForm(asset));
}

/** 库稳定排序：kind 升序 → id 升序（纯码元比较，禁 localeCompare）。 */
export function sortAssets(assets: ReadonlyArray<LibraryAsset>): LibraryAsset[] {
  return [...assets].sort(
    (a, b) => (a.kind < b.kind ? -1 : a.kind > b.kind ? 1 : 0) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
  );
}

/** 便携包形状。 */
export interface AssetLibraryPackage {
  readonly version: number;
  readonly assets: ReadonlyArray<LibraryAsset>;
}

/** 导出便携 JSON 包：稳定排序 + 固定键序（双端逐字节一致）。 */
export function buildAssetLibraryExport(assets: ReadonlyArray<LibraryAsset>): string {
  const pkg: AssetLibraryPackage = { version: ASSET_LIBRARY_VERSION, assets: sortAssets(assets) };
  return JSON.stringify(pkg, null, 2);
}

export interface AssetImportResult {
  readonly assets: ReadonlyArray<LibraryAsset>;
  /** 非法条目错误（index + 原因）。 */
  readonly errors: ReadonlyArray<string>;
}

/**
 * 便携包导入：逐条校验，非法条目跳过记错误（不阻断合法条目）；版本不符整包拒绝。
 * 导入产物按库序排稳（kind+id）。
 */
export function parseAssetLibraryImport(json: string): AssetImportResult {
  let pkg: unknown;
  try {
    pkg = JSON.parse(json);
  } catch (error) {
    return { assets: [], errors: [`(json): ${String(error)}`] };
  }
  const record = pkg as { version?: unknown; assets?: unknown };
  if (record?.version !== ASSET_LIBRARY_VERSION) {
    return { assets: [], errors: [`(version): expected ${ASSET_LIBRARY_VERSION}`] };
  }
  if (!Array.isArray(record.assets)) {
    return { assets: [], errors: ["(assets): expected array"] };
  }
  const assets: LibraryAsset[] = [];
  const errors: string[] = [];
  record.assets.forEach((raw, index) => {
    const { asset, errors: itemErrors } = validateLibraryAsset(raw);
    if (asset) {
      assets.push(asset);
    } else {
      errors.push(...itemErrors.map((message) => `assets[${index}] ${message}`));
    }
  });
  return { assets: sortAssets(assets), errors };
}

export interface AssetMergeResult {
  readonly merged: ReadonlyArray<LibraryAsset>;
  /** 同 id 同内容跳过数（幂等）。 */
  readonly skipped: number;
  /** 同 id 异内容覆盖数（incoming 赢）。 */
  readonly overwritten: number;
  /** 新增数。 */
  readonly added: number;
}

/**
 * 幂等合并：以 existing（id → 资产）为底座逐条并 incoming——
 * 同 id 同内容（hash 相同）跳过；同 id 异内容覆盖（incoming 赢，对齐
 * review_metrics「修订后最新赢」语义）；新 id 追加。merged 按库序排稳。
 */
export function mergeAssetLibrary(
  existing: ReadonlyArray<LibraryAsset>,
  incoming: ReadonlyArray<LibraryAsset>,
): AssetMergeResult {
  const byId = new Map(existing.map((asset) => [`${asset.kind}:${asset.id}`, asset]));
  let skipped = 0;
  let overwritten = 0;
  let added = 0;
  for (const asset of incoming) {
    const key = `${asset.kind}:${asset.id}`;
    const current = byId.get(key);
    if (current) {
      if (assetContentHash(current) === assetContentHash(asset)) {
        skipped += 1;
      } else {
        byId.set(key, asset);
        overwritten += 1;
      }
    } else {
      byId.set(key, asset);
      added += 1;
    }
  }
  return { merged: sortAssets([...byId.values()]), skipped, overwritten, added };
}

/**
 * 内置精选题材基底种子（首批 3 个，随包分发；355 号空库冷启动对策）。
 * id 为 snake_case slug；samples 为写法示例片段而非本书世界设定。
 */
export const GENRE_BASE_SEEDS: ReadonlyArray<LibraryAsset> = [
  {
    id: "urban_supernatural",
    kind: "genre-base",
    name: "都市异能",
    body:
      "现代都市背景下的超凡力量体系。核心张力=日常身份与异能秘密的双线挤压：异能升级必须付出可感知代价（精力/人脉/隐匿风险），力量增长与生活崩坏同步推进。金手指要克制——前期异能只能解决「被欺负」而不是「翻身」，翻身要靠主角用异能做出现代人的聪明决策。",
    expectations: [
      "每 3–5 章一次异能用法的新花样（不是数值变强而是用法变巧）",
      "都市秩序（工作/家庭/朋友）持续被异能秘密挤压并付出代价",
      "反派同样受现实规则约束，斗智成分不低于斗力",
    ],
    taboos: [
      "异能万能化：解决一切问题的按键式金手指",
      "现代人行为逻辑消失：主角获得力量后变成古代皇帝思维",
      "反派降智：为了衬托主角而让对手做出违背利益的决策",
    ],
    samples: [
      "他把手按在闸机上，电流顺着指尖爬进系统——余额清零的提示音响起时，保安的目光刚好扫过来。三秒。他只有三秒装作什么都没发生。",
    ],
  },
  {
    id: "xuanhuan_cultivation",
    kind: "genre-base",
    name: "玄幻修真",
    body:
      "境界驱动的东方玄幻。核心循环=资源争夺→境界突破→新地图新秩序：每次突破都要改写主角在势力格局中的位置，而不是只改数字。修炼体系须自洽（境界间有质变标志），突破节点放小高潮，突破代价与心魔埋进人物弧线。",
    expectations: [
      "境界突破有可感知的质变标志（新能力/新视野/新敌人）",
      "资源线清晰：灵石/功法/丹药争夺驱动至少一半章节冲突",
      "每卷一个跨越势力层级的新格局（宗门→州→域→界）",
    ],
    taboos: [
      "闭关流水账：跳过冲突的纯数值突破",
      "境界碾压万能：低阶永远不可能凭智谋/底牌赢高阶",
      "配角工具化：所有NPC只为供应主角资源而存在",
    ],
    samples: [
      "丹成那一刻，雷云没有散——它们在等他的下一口气。林动笑了：夺舍他的老东西，恐怕没想到这一炉丹里掺了自己的血。",
    ],
  },
  {
    id: "mystery_investigation",
    kind: "genre-base",
    name: "悬疑刑侦",
    body:
      "信息差驱动的侦探叙事。核心引擎=读者与侦探的信息差管理：每章释放一条可回溯的线索，同时制造一个新的误导方向。案件结构=表面动机→隐藏动机→结构性真凶，破案靠证据链而非巧合；主角的执念/伤痕是贯穿案件外的第二主线。",
    expectations: [
      "每章至少一条可回溯的真实线索（读者理论上可推出真相）",
      "每案件三层反转：表象→动机→结构（最大意外落在结构层）",
      "破案前主角付出真实代价（受伤/失去/立场崩塌）",
    ],
    taboos: [
      "巧合破案：关键证据靠运气送上门",
      "侦探全知：主角知道读者不可能知道的私密信息",
      "真凶零铺垫：结局突然出现前文未存在的角色",
    ],
    samples: [
      "档案袋里只有一张超市小票。老周看了三遍——收银员编号是 police 分局的内勤代码。买酸奶的时间，正是三年前那场大火的凌晨。",
    ],
  },
];
