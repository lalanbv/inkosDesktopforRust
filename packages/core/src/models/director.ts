import { z } from "zod";

/**
 * 自动导演产品化契约（G6/352 号，Phase C 末件首批；ANWA A7 采纳）。
 *
 * 既有建书 = 单方案问答式 draft（BookCreationDraft）。G6 补齐**导演层**：
 * 1. **灵感卡** `InspirationCard`：作者唯一必选创作点（一句话灵感 + 可选维度）；
 * 2. **方向候选批量生成** `buildDirectionCandidatesPrompt` / `parseDirectionCandidates`：
 *    一次产 2~3 套并列方向（标题/钩子/简介/差异化），支持标题组重做（excludeTitles）；
 * 3. **运行模式** `resolveDirectorRunPlan`：ready-stop（就绪即停）/ range（范围）/
 *    full-book（全书）→ 停点与章区间计划，与 scheduler 循环边界对接；
 * 4. **驾驶舱阶段推进** `nextDirectorStage`：灵感→方向→大纲→写作→暂停/完成的状态
 *    决策表（阶段×停点，对齐 331 恢复手册）。

 * 双端：`engine-rs/src/models/director.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/director-vectors.json`。
 */

// ── 灵感卡 ──

export const InspirationCardSchema = z.object({
  /** 一句话灵感（唯一必选创作点）。 */
  premise: z.string().min(1),
  genre: z.string().min(1).optional(),
  platform: z.string().min(1).optional(),
  tone: z.string().min(1).optional(),
  keywords: z.array(z.string().min(1)).default([]),
});

export type InspirationCard = z.infer<typeof InspirationCardSchema>;

// ── 方向候选 ──

export interface DirectionCandidate {
  readonly id: string;
  readonly title: string;
  /** 一句话钩子。 */
  readonly hook: string;
  readonly genre: string;
  readonly synopsis: string;
  /** 差异化（如何避开同质化）。 */
  readonly differentiator: string;
  readonly confidence: number;
}

/**
 * 方向候选批量生成 prompt（count 默认 3；excludeTitles = 标题组重做时剔除的历史标题）。
 */
export function buildDirectionCandidatesPrompt(params: {
  readonly inspiration: InspirationCard;
  readonly count?: number;
  readonly excludeTitles?: ReadonlyArray<string>;
  readonly language?: "zh" | "en";
  /** R4/364 号：库资产 guidance 块（renderAssetGuidanceBlock 产物，可选挂载）。 */
  readonly assetGuidance?: string;
}): string {
  const count = params.count ?? 3;
  const isEn = params.language === "en";
  const i = params.inspiration;
  const excludeBlock =
    params.excludeTitles && params.excludeTitles.length > 0
      ? isEn
        ? `\nExcluded titles (do not reuse): ${params.excludeTitles.join(", ")}\n`
        : `\n已排除标题（不得复用）：${params.excludeTitles.join("、")}\n`
      : "";
  return isEn
    ? `You are the story director. Generate ${count} alternative book directions from the same inspiration.

## Inspiration
- Premise: ${i.premise}${i.genre ? `\n- Genre: ${i.genre}` : ""}${i.platform ? `\n- Platform: ${i.platform}` : ""}${i.tone ? `\n- Tone: ${i.tone}` : ""}${i.keywords.length > 0 ? `\n- Keywords: ${i.keywords.join(", ")}` : ""}${excludeBlock}
Output JSON: {"directions":[{"id":"d1","title":"...","hook":"one-line hook","genre":"...","synopsis":"2-3 sentences","differentiator":"how it avoids sameness","confidence":0.0-1.0}]}${params.assetGuidance ? `\n\n${params.assetGuidance}` : ""}`
    : `你是故事导演。基于同一份灵感，生成 ${count} 套并列的开书方向。

## 灵感卡
- 灵感：${i.premise}${i.genre ? `\n- 题材：${i.genre}` : ""}${i.platform ? `\n- 平台：${i.platform}` : ""}${i.tone ? `\n- 基调：${i.tone}` : ""}${i.keywords.length > 0 ? `\n- 关键词：${i.keywords.join("、")}` : ""}${excludeBlock}
输出 JSON：{"directions":[{"id":"d1","title":"书名","hook":"一句话钩子","genre":"题材","synopsis":"两三句简介","differentiator":"差异化（如何避开同质化）","confidence":0.0-1.0}]}${params.assetGuidance ? `\n\n${params.assetGuidance}` : ""}`
}

/**
 * 解析方向候选（容错：围栏剥离/首个 JSON 块），剔除与 excludeTitles 重叠的标题
 * （标题组重做），按 confidence 降序；directions 缺失/空 → 空数组不抛错。
 */
export function parseDirectionCandidates(
  content: string,
  excludeTitles: ReadonlyArray<string> = [],
): DirectionCandidate[] {
  const match = /\{[\s\S]*\}/.exec(content);
  if (!match) return [];
  let parsed: { directions?: Array<Record<string, unknown>> };
  try {
    parsed = JSON.parse(match[0]) as typeof parsed;
  } catch {
    return [];
  }
  const rows = Array.isArray(parsed.directions) ? parsed.directions : [];
  const excluded = new Set(excludeTitles.map((t) => t.trim().toLowerCase()).filter(Boolean));
  const out: DirectionCandidate[] = [];
  for (const [index, row] of rows.entries()) {
    const title = typeof row.title === "string" ? row.title.trim() : "";
    if (!title) continue;
    if (excluded.has(title.trim().toLowerCase())) continue;
    out.push({
      id: typeof row.id === "string" && row.id ? row.id : `d${index + 1}`,
      title,
      hook: typeof row.hook === "string" ? row.hook : "",
      genre: typeof row.genre === "string" ? row.genre : "",
      synopsis: typeof row.synopsis === "string" ? row.synopsis : "",
      differentiator: typeof row.differentiator === "string" ? row.differentiator : "",
      confidence: typeof row.confidence === "number" ? row.confidence : 0.5,
    });
  }
    return out.sort(
    (a, b) =>
      b.confidence - a.confidence || (a.title < b.title ? -1 : a.title > b.title ? 1 : 0),
  );
}

// ── 运行模式与阶段编排 ──

export const DIRECTOR_RUN_MODES = ["ready-stop", "range", "full-book"] as const;
export type DirectorRunMode = (typeof DIRECTOR_RUN_MODES)[number];

export const DIRECTOR_STAGES = [
  "inspiration",
  "directions",
  "outline",
  "writing",
  "paused",
  "done",
] as const;
export type DirectorStage = (typeof DIRECTOR_STAGES)[number];

export interface DirectorRunPlan {
  readonly mode: DirectorRunMode;
  readonly fromChapter: number;
  /** 范围/全书模式的终章（含）；ready-stop 恒 null。 */
  readonly toChapter: number | null;
  /** 该模式的停点序列：方向确认后是否自动继续。 */
  readonly stopAfterDirections: boolean;
}

/**
 * 运行模式 → 执行计划：
 * - ready-stop：生成方向即停（toChapter=null, stopAfterDirections=true）；
 * - range：跑 [from, to]，to 缺省 = targetChapters；
 * - full-book：跑 [from, targetChapters]。
 */
export function resolveDirectorRunPlan(params: {
  readonly mode: DirectorRunMode;
  readonly fromChapter?: number;
  readonly toChapter?: number;
  readonly targetChapters: number;
}): DirectorRunPlan {
  const fromChapter = Math.max(1, params.fromChapter ?? 1);
  if (params.mode === "ready-stop") {
    return { mode: params.mode, fromChapter, toChapter: null, stopAfterDirections: true };
  }
  if (params.mode === "range") {
    const to = params.toChapter ?? params.targetChapters;
    return {
      mode: params.mode,
      fromChapter,
      toChapter: Math.max(fromChapter, Math.min(to, params.targetChapters)),
      stopAfterDirections: false,
    };
  }
  return {
    mode: params.mode,
    fromChapter,
    toChapter: Math.max(fromChapter, params.targetChapters),
    stopAfterDirections: false,
  };
}

/**
 * 驾驶舱阶段推进：当前阶段 + 运行模式 → 下一阶段。
 * directions 之后：ready-stop → paused（等确认）；其余 → outline；
 * writing 之后：range 且到达 toChapter → paused；full-book 到达 → done；
 * 其余 → 继续 writing。
 */
export function nextDirectorStage(params: {
  readonly current: DirectorStage;
  readonly mode: DirectorRunMode;
  readonly writtenChapters: number;
  readonly toChapter: number;
}): DirectorStage {
  const { current, mode, writtenChapters, toChapter } = params;
  switch (current) {
    case "inspiration":
      return "directions";
    case "directions":
      return mode === "ready-stop" ? "paused" : "outline";
    case "outline":
      return "writing";
    case "writing":
      if (writtenChapters >= toChapter) {
        return mode === "full-book" ? "done" : "paused";
      }
      return "writing";
    default:
      return current;
  }
}

/** 机器可读契约（双端 golden 锁形状）。 */
export interface DirectorContract {
  runModes: DirectorRunMode[];
  stages: DirectorStage[];
  readyStopStopsAfterDirections: boolean;
  manualEditNeverAutoOverwritten: boolean;
}

export const DIRECTOR_CONTRACT: DirectorContract = {
  runModes: [...DIRECTOR_RUN_MODES],
  stages: [...DIRECTOR_STAGES],
  readyStopStopsAfterDirections: true,
  manualEditNeverAutoOverwritten: true,
};
