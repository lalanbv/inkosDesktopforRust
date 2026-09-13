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

/** R15/384 号：市场包元数据（可选；分享时标注作者/描述/来源）。 */
export interface AssetPackMeta {
  readonly author?: string;
  readonly description?: string;
  readonly source?: string;
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

/**
 * R15/384 号：导入预览（零落盘）——解析包并对照现有库给出
 * 新增/覆盖/跳过预估，供「预览 → 确认」两段式导入 UI。
 */
export function previewAssetLibraryImport(
  existing: ReadonlyArray<LibraryAsset>,
  json: string,
): { added: number; overwritten: number; skipped: number; errors: ReadonlyArray<string>; samples: ReadonlyArray<{ id: string; name: string; kind: AssetKind }> } {
  const parsed = parseAssetLibraryImport(json);
  const merged = mergeAssetLibrary(existing, parsed.assets);
  return {
    added: merged.added,
    overwritten: merged.overwritten,
    skipped: merged.skipped,
    errors: parsed.errors,
    samples: parsed.assets.map((asset) => ({ id: asset.id, name: asset.name, kind: asset.kind })),
  };
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

/**
 * 内置推进模式种子（363 号，双端逐字镜像；空库冷启动随包分发）。
 * 推进模式 = 情节推进范式：写作时与题材基底可叠加挂载（接线批 364 号）。
 */
export const PROGRESSION_MODE_SEEDS: ReadonlyArray<LibraryAsset> = [
  {
    id: "hook_cycle",
    kind: "progression-mode",
    name: "钩子循环推进",
    body:
      "章级推进范式：章末强钩 → 下章开头真实承接 → 中段推进该钩并付出一次代价 → 章末再埋新钩。钩子类型轮换（悬念/危机/情感交替），循环以「代价被记账」为闭合标志——读者的期待是被承诺出来的账，每轮必须还一笔再欠一笔。",
    expectations: [
      "章末钩与下章开头必须真实承接，不重置场景也不拖延兑现",
      "钩子类型相邻循环不重复（悬念后接危机或情感）",
      "每个循环内至少一次可感知代价（资源/关系/信息）",
    ],
    taboos: [
      "同型钩子连用三章以上（读者疲劳点）",
      "章末空抛：钩子内容与正文推进无关",
      "只欠不还：连续多轮循环无任何旧钩兑付",
    ],
    samples: [
      "她终于打开了那个盒子——里面的东西不是钱，是一张她自己签名的认罪书。日期，是明天。",
    ],
  },
  {
    id: "escalating_loop",
    kind: "progression-mode",
    name: "升级循环推进",
    body:
      "卷级推进范式：每个循环（约 5–10 章）赌注明确升一级，且代价前置——先付出再收获。升级来自主角的选择与牺牲（用已知信息做更冒险的决定），而非外力送来的新外挂。上一轮的代价在下一轮持续生效成为本轮障碍，形成「债滚债」的推进压力。",
    expectations: [
      "每循环开局一句话能说清本轮赌注比上轮大在哪",
      "上轮代价在本轮至少一次实际阻碍主角",
      "升级决策由主角主动做出并承担可见风险",
    ],
    taboos: [
      "数值膨胀代替局势升级（敌人只是数字变大了）",
      "危机重复同一形态（换个名字的同一件事）",
      "外力救场：新外挂/新帮手凭空出现解决本轮危机",
    ],
    samples: [
      "上次他赌上的是右手经脉。这一次，对面坐着全城最不该得罪的人，而他手里的筹码只有半张烧残的地图——和右手的旧伤。",
    ],
  },
  {
    id: "three_act",
    kind: "progression-mode",
    name: "三幕卷结构",
    body:
      "卷级结构范式：建置（约 25%）→ 对抗（约 50%）→ 解决（约 25%），幕间各放一个转折点。第一转折打破主角的既有策略，中点用假胜利或假失败翻转局势，高潮同时解决主冲突并埋下卷间钩。三幕比例是节奏底线而非装饰——建置超四成必拖。",
    expectations: [
      "第一转折落在卷内 20%–30% 处，且由主角自己的决定触发",
      "中点有一次局势翻转（假胜利或假失败）",
      "高潮解决本卷主冲突，同时开启下一卷的核心悬念",
    ],
    taboos: [
      "建置超卷长四成（迟迟不进对抗幕）",
      "转折无因果铺垫（纯意外事件砸脸）",
      "解决幕拖尾：高潮后灌水超过卷长一成",
    ],
    samples: [
      "所有人都以为庆功宴是这一卷的结束——直到主宾的椅子空了，桌上的信封里装着第三具尸体的照片。第一幕，才刚刚收尾。",
    ],
  },
];

// ── R4/364 号：库资产 guidance 渲染（导演方向候选/建书挂载共用）──

const GUIDANCE_BODY_MAX_CHARS = 600;
const GUIDANCE_LIST_MAX_ITEMS = 5;

/**
 * 资产 guidance 块（注入导演方向候选 prompt / 建书上下文）：
 * 每资产一节（name + body 截断 600 码元 + expectations/taboos 各取前 5 条）。
 * assets 为空返回 undefined（不产空节）。
 */
export function renderAssetGuidanceBlock(
  assets: ReadonlyArray<LibraryAsset>,
  language: "zh" | "en" = "zh",
): string | undefined {
  if (assets.length === 0) return undefined;
  const isEn = language === "en";
  const clip = (text: string): string =>
    text.length > GUIDANCE_BODY_MAX_CHARS
      ? `${text.slice(0, GUIDANCE_BODY_MAX_CHARS - 1)}…`
      : text;
  const sections = assets.map((asset) => {
    const lines = [
      `### ${asset.name} (${asset.kind}/${asset.id})`,
      clip(asset.body),
    ];
    if (asset.expectations.length > 0) {
      lines.push(
        isEn ? "Expectations:" : "读者期待：",
        ...asset.expectations
          .slice(0, GUIDANCE_LIST_MAX_ITEMS)
          .map((item) => `- ${item}`),
      );
    }
    if (asset.taboos.length > 0) {
      lines.push(
        isEn ? "Taboos:" : "禁忌：",
        ...asset.taboos
          .slice(0, GUIDANCE_LIST_MAX_ITEMS)
          .map((item) => `- ${item}`),
      );
    }
    return lines.join("\n");
  });
  return [
    isEn ? "## Library asset references" : "## 库资产参考",
    isEn
      ? "The following reference assets set expectations and taboos for the directions below."
      : "以下参考资产规定了方向的读者期待与禁忌。",
    "",
    ...sections,
  ].join("\n");
}

/**
 * 两段式采用材料头注（364 号）：世界样本等库资产「采用到本书」时写入材料池
 * 的 markdown 前缀——通用样本≠本书世界，采用时须按本书设定改写。
 */
export function renderAdoptionHeader(asset: LibraryAsset, bookId: string, language: "zh" | "en" = "zh"): string {
  const isEn = language === "en";
  return isEn
    ? `# Adopted library asset: ${asset.name}\n\n> Generic sample ≠ this book's world. Rewrite per this book's settings before use.\n> Source: ${asset.kind}/${asset.id} · adopted by book \`${bookId}\`\n`
    : `# 采用的库资产：${asset.name}\n\n> 通用样本≠本书世界：采用时须按本书设定改写。\n> 来源：${asset.kind}/${asset.id} · 采用书：\`${bookId}\`\n`;
}
