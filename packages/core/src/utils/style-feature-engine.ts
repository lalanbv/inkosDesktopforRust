import type { StyleProfile } from "../models/style-profile.js";

/**
 * 写法引擎资产化契约（G4/339 号，Phase B 批次二首项；ANWA A5 采纳）。
 *
 * StyleProfile 是一份统计指纹——**资产化**意味着把它拆成作者可见、可逐项
 * 启停、可跨书绑定的"特征池"，并给写作链提供三件套：
 *
 * 1. **特征池推导** `deriveFeaturePool`：从 StyleProfile 确定性推导特征项
 *    （句长/段长/词汇多样性 + topPatterns + rhetoricalFeatures），每项带
 *    写作模型可执行的 guidance 文本；
 * 2. **启停与组合** `applyFeatureSelection` + `resolveStyleBinding`：显式白名单
 *    优先于黑名单；每书绑定（StyleBinding）把特征池绑定到具体书；
 * 3. **试写入口** `buildTrialWritePrompt`：绑定生效特征 + 场景前提 → 试写提示词。
 *
 * 另附 **专名泄露检测** `detectProperNounLeak`（ANWA A9）：仿写/拆书产物的
 * 正文若出现参考作品专名（人名/地名/门派/功法/书名）即计数告警。
 *
 * 双端：`engine-rs/src/utils/style_feature_engine.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/style-feature-engine-vectors.json`。
 */

export interface StyleFeature {
  readonly id: string;
  readonly kind: "metric" | "pattern" | "rhetoric";
  readonly label: string;
  /** 注入写作模型的执行指令（zh/en 双语渲染见 composeFeatureGuidance）。 */
  readonly guidanceZh: string;
  readonly guidanceEn: string;
}

const round = (value: number): number => Math.round(value * 10) / 10;

/** 特征池推导：确定性；空列表维度跳过。 */
export function deriveFeaturePool(profile: StyleProfile): StyleFeature[] {
  const features: StyleFeature[] = [];
  const sentenceLength = round(profile.avgSentenceLength);
  features.push({
    id: "metric:sentence-length",
    kind: "metric",
    label: "sentence-length",
    guidanceZh: `单句平均长度控制在约 ${sentenceLength} 字（允许小幅波动）。`,
    guidanceEn: `Keep the average sentence length near ${sentenceLength} (small variance allowed).`,
  });
  const paragraphLow = Math.round(profile.paragraphLengthRange.min);
  const paragraphHigh = Math.round(profile.paragraphLengthRange.max);
  features.push({
    id: "metric:paragraph-length",
    kind: "metric",
    label: "paragraph-length",
    guidanceZh: `单段长度大致落在 ${paragraphLow}–${paragraphHigh} 字区间。`,
    guidanceEn: `Keep paragraph lengths roughly within ${paragraphLow}–${paragraphHigh} characters.`,
  });
  const diversityPct = Math.round(profile.vocabularyDiversity * 100);
  features.push({
    id: "metric:vocabulary-diversity",
    kind: "metric",
    label: "vocabulary-diversity",
    guidanceZh: `保持词汇多样性约 ${diversityPct}%（TTR），避免高频重复用词。`,
    guidanceEn: `Keep vocabulary diversity near ${diversityPct}% (TTR); avoid repeated word choices.`,
  });
  for (const pattern of profile.topPatterns ?? []) {
    features.push({
      id: `pattern:${pattern}`,
      kind: "pattern",
      label: pattern,
      guidanceZh: `写作时体现句式特征「${pattern}」。`,
      guidanceEn: `Reflect the sentence pattern "${pattern}" in the prose.`,
    });
  }
  for (const feature of profile.rhetoricalFeatures ?? []) {
    features.push({
      id: `rhetoric:${feature}`,
      kind: "rhetoric",
      label: feature,
      guidanceZh: `适度运用修辞手法「${feature}」，不要堆砌。`,
      guidanceEn: `Use the rhetorical device "${feature}" with restraint.`,
    });
  }
  return features;
}

/**
 * 启停与组合：显式白名单（enabledIds）优先——给出时只保留白名单；
 * 否则取全集再剔除黑名单（disabledIds）。未知 id 一律忽略（不报错）。
 */
export function applyFeatureSelection(
  pool: ReadonlyArray<StyleFeature>,
  enabledIds?: ReadonlyArray<string>,
  disabledIds?: ReadonlyArray<string>,
): StyleFeature[] {
  if (enabledIds && enabledIds.length > 0) {
    const whitelist = new Set(enabledIds);
    return pool.filter((feature) => whitelist.has(feature.id));
  }
  if (disabledIds && disabledIds.length > 0) {
    const blacklist = new Set(disabledIds);
    return pool.filter((feature) => !blacklist.has(feature.id));
  }
  return [...pool];
}

/** 每书绑定：特征池名 + 启停覆盖 + 注入预算。 */
export interface StyleBinding {
  readonly profileName: string;
  readonly enabledIds?: ReadonlyArray<string>;
  readonly disabledIds?: ReadonlyArray<string>;
  /** guidance 段最大字符数（超出截断）。 */
  readonly maxGuidanceChars?: number;
}

export interface BindingResolution {
  readonly profileName: string;
  readonly poolSize: number;
  readonly enabled: StyleFeature[];
  readonly guidanceZh: string;
  readonly guidanceEn: string;
}

const GUIDANCE_ITEM_LIMIT = 12;

export function composeStyleGuidance(
  enabled: ReadonlyArray<StyleFeature>,
  language: "zh" | "en",
  maxChars?: number,
): string {
  if (enabled.length === 0) return "";
  const lines = enabled.slice(0, GUIDANCE_ITEM_LIMIT).map((feature) =>
    `- ${language === "en" ? feature.guidanceEn : feature.guidanceZh}`,
  );
  const header = language === "en"
    ? "## Style features (bound profile — follow while writing)"
    : "## 写法特征（绑定档案——写作时遵循）";
  const text = [header, ...lines].join("\n");
  if (maxChars !== undefined && text.length > maxChars) {
    return `${text.slice(0, Math.max(0, maxChars - 1))}…`;
  }
  return text;
}

/** 绑定解析：档案不存在返回 null；产出双语文案段（试写/写作链共用）。 */
export function resolveStyleBinding(
  profiles: ReadonlyArray<Record<string, unknown>>,
  binding: StyleBinding,
): BindingResolution | null {
  const profile = profiles.find((entry) => entry["sourceName"] === binding.profileName) as
    | StyleProfile
    | undefined;
  if (!profile) return null;
  const pool = deriveFeaturePool(profile);
  const enabled = applyFeatureSelection(pool, binding.enabledIds, binding.disabledIds);
  return {
    profileName: binding.profileName,
    poolSize: pool.length,
    enabled,
    guidanceZh: composeStyleGuidance(enabled, "zh", binding.maxGuidanceChars),
    guidanceEn: composeStyleGuidance(enabled, "en", binding.maxGuidanceChars),
  };
}

/** 试写入口：绑定特征 + 场景前提 → 试写提示词（单块输出，禁止越题）。 */
export function buildTrialWritePrompt(params: {
  readonly guidance: string;
  readonly premise: string;
  readonly sceneBrief: string;
  readonly targetChars: number;
  readonly language: "zh" | "en";
}): string {
  if (params.language === "en") {
    return `Trial-write a passage under the bound style profile.

${params.guidance || "(no style features bound)"}

## Premise
${params.premise}

## Scene brief
${params.sceneBrief}

Requirements:
- Around ${params.targetChars} characters of prose
- Output a single TRIAL_CONTENT block and nothing else
- Do not step outside the scene brief; do not invent named characters beyond it`;
  }
  return `按绑定的写法档案试写一段正文。

${params.guidance || "（未绑定任何写法特征）"}

## 前提
${params.premise}

## 场景简报
${params.sceneBrief}

要求：
- 正文约 ${params.targetChars} 字
- 只输出一个 TRIAL_CONTENT 区块，不要输出其他内容
- 不要超出场景简报的范围；不要自行新增具名角色`;
}

// ── 专名泄露检测（ANWA A9）──

export interface ProperNounLeakHit {
  readonly name: string;
  readonly occurrences: number;
}

/** 泄露检测：protectedNames（参考作品专名）在正文中的出现计数（大小写不敏感、长名优先防重叠）。 */
export function detectProperNounLeak(params: {
  readonly content: string;
  readonly protectedNames: ReadonlyArray<string>;
  readonly minOccurrences?: number;
}): ProperNounLeakHit[] {
  const minOccurrences = params.minOccurrences ?? 1;
  const content = params.content ?? "";
  if (!content || params.protectedNames.length === 0) return [];

  // 长名优先：把已匹配的长名替换为占位符，防止"萧家堡"同时计出"萧家"。
  const names = [...new Set(params.protectedNames.map((name) => name.trim()).filter(Boolean))]
    .sort((a, b) => b.length - a.length);
  let masked = content;
  const hits: ProperNounLeakHit[] = [];
  for (const name of names) {
    const lowerName = name.toLowerCase();
    let occurrences = 0;
    let index = masked.toLowerCase().indexOf(lowerName);
    while (index >= 0) {
      occurrences += 1;
      masked = masked.slice(0, index) + "\u0000".repeat(name.length) + masked.slice(index + name.length);
      index = masked.toLowerCase().indexOf(lowerName);
    }
    if (occurrences >= minOccurrences) {
      hits.push({ name, occurrences });
    }
  }
  return hits.sort((a, b) => b.occurrences - a.occurrences || a.name.localeCompare(b.name));
}
