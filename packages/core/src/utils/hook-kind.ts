import type { HookKind } from "../models/runtime-state.js";

/**
 * R23 伏笔类型标注（393 号，四轮 P1；对标蛙趣 8 类伏笔生命周期的取精版）。
 *
 * 规范分类 ≤7 类不做数量战：承诺/悬念/危机/物品/信息/情感/世界观。
 * 职责：把 settler/architect/作者输入的自由文本归一化为规范 kind
 * （别名表精确匹配，未知 → undefined 绝不臆测）；双语展示标签。
 * 双端契约：`engine-rs/src/utils/hook_kind.rs` 1:1 镜像；共享向量
 * `golden/hook-kind-vectors.json`（TS：`golden-hook-kind.test.ts`；
 * Rust 差分：`tests/golden_hook_kind_diff.rs`）。
 */

export const HOOK_KIND_IDS = [
  "promise",
  "suspense",
  "crisis",
  "artifact",
  "information",
  "emotion",
  "worldview",
] as const;

export type HookKindId = (typeof HOOK_KIND_IDS)[number];

/** 别名表：小写化后精确匹配（zh 别名原样；en 大小写不敏感）。 */
const KIND_ALIASES: Readonly<Record<string, HookKind>> = {
  // promise 承诺
  "promise": "promise",
  "承诺": "promise",
  "约定": "promise",
  "誓言": "promise",
  // suspense 悬念
  "suspense": "suspense",
  "mystery": "suspense",
  "悬念": "suspense",
  "谜团": "suspense",
  // crisis 危机
  "crisis": "crisis",
  "危机": "crisis",
  "险局": "crisis",
  "威胁": "crisis",
  // artifact 物品
  "artifact": "artifact",
  "relic": "artifact",
  "物品": "artifact",
  "信物": "artifact",
  "道具": "artifact",
  // information 信息
  "information": "information",
  "info": "information",
  "secret": "information",
  "信息": "information",
  "情报": "information",
  "秘密": "information",
  // emotion 情感
  "emotion": "emotion",
  "relationship": "emotion",
  "romance": "emotion",
  "情感": "emotion",
  "感情": "emotion",
  "关系": "emotion",
  // worldview 世界观
  "worldview": "worldview",
  "lore": "worldview",
  "世界观": "worldview",
  "设定": "worldview",
};

/** 自由文本 → 规范 kind；未知返回 undefined（绝不臆测归类）。 */
export function normalizeHookKind(raw: string): HookKind | undefined {
  const key = raw.trim().toLowerCase();
  return KIND_ALIASES[key];
}

/** 双语展示标签（UI/timeline/回收提示）。 */
export function hookKindLabel(kind: HookKind, language: "zh" | "en" = "zh"): string {
  const zh: Record<HookKind, string> = {
    promise: "承诺",
    suspense: "悬念",
    crisis: "危机",
    artifact: "物品",
    information: "信息",
    emotion: "情感",
    worldview: "世界观",
  };
  const en: Record<HookKind, string> = {
    promise: "Promise",
    suspense: "Suspense",
    crisis: "Crisis",
    artifact: "Artifact",
    information: "Information",
    emotion: "Emotion",
    worldview: "Worldview",
  };
  return language === "en" ? en[kind] : zh[kind];
}
