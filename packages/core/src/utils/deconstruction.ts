/**
 * 拆书工作台契约（G5/350 号，Phase C 第二大件首批；ANWA A6 拆书的确定性核心采纳）。
 *
 * 统一拆书面 = 把 chapter-analyzer 的逐章产物聚合为**可发布的拆书资产**：
 * 1. **四档人物档案** `deriveCharacterDossier`：brief（出场章）→ standard（+首末章/
 *    事件行）→ deep（+出场率/最长缺席）→ full（+全部证据原文）；
 * 2. **章节证据回溯** `buildEvidenceIndex`：角色 → [{章, 证据}] 索引
 *    （"这个角色在第几章做过什么"一键回溯）；
 * 3. **节奏/卖点统计** `analyzePacingStats`：章型序列占比 + 最长连续同型 + 强钩密度；
 * 4. **产物导出** `renderDeconstructionExport`：渲染为参考资料兼容 markdown
 *    （`## Extracted content` 标记，被 `reference-context.extractMaterialContent`
 *    消费——拆书产物可直接发布到参考资料供 G1 召回）。

 * 双端：`engine-rs/src/utils/deconstruction.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/deconstruction-vectors.json`。
 */

export const DECONSTRUCTION_DEPTHS = ["brief", "standard", "deep", "full"] as const;
export type DeconstructionDepth = (typeof DECONSTRUCTION_DEPTHS)[number];

/** 单章拆书证据（来自 chapter-analyzer 产物或素材分章）。 */
export interface DeconChapter {
  readonly chapter: number;
  /** 出场人物（已拆分）。 */
  readonly characters: ReadonlyArray<string>;
  /** 本章关键事件/梗概。 */
  readonly events: string;
  /** 章型（推进/铺垫/高潮…；可空）。 */
  readonly chapterType?: string;
  /** 伏笔动静（G10 强度口径可复用；可空）。 */
  readonly hookActivity?: string;
}

/** 证据回溯索引：角色 → 逐章证据。 */
export interface EvidenceEntry {
  readonly chapter: number;
  readonly evidence: string;
}

export interface EvidenceIndex {
  /** 角色 → 逐章证据（章号升序）。 */
  readonly byCharacter: Readonly<Record<string, EvidenceEntry[]>>;
  /** 全局章型序列（升序，仅含有章型的章）。 */
  readonly chapterTypes: ReadonlyArray<{ readonly chapter: number; readonly chapterType: string }>;
}

/** 证据索引构建（章号升序稳定；events 空白行不入索引）。 */
export function buildEvidenceIndex(chapters: ReadonlyArray<DeconChapter>): EvidenceIndex {
  const byCharacter: Record<string, EvidenceEntry[]> = {};
  const chapterTypes: Array<{ chapter: number; chapterType: string }> = [];
  for (const chapter of [...chapters].sort((a, b) => a.chapter - b.chapter)) {
    const evidence = chapter.events.trim();
    if (evidence) {
      for (const character of chapter.characters) {
        const name = character.trim();
        if (!name) continue;
        (byCharacter[name] ??= []).push({ chapter: chapter.chapter, evidence });
      }
    }
    if (chapter.chapterType?.trim()) {
      chapterTypes.push({ chapter: chapter.chapter, chapterType: chapter.chapterType.trim() });
    }
  }
  return { byCharacter, chapterTypes };
}

export interface CharacterDossier {
  readonly name: string;
  readonly appearances: ReadonlyArray<number>;
  readonly firstChapter: number;
  readonly lastChapter: number;
  /** 出场章数 / 书籍章数跨度（0–1，一位小数）。deep/full 档填充。 */
  readonly frequency?: number;
  /** 最长连续缺席章数。deep/full 档填充。 */
  readonly longestAbsence?: number;
  readonly evidence: ReadonlyArray<EvidenceEntry>;
}

/** 四档档案推导：depth 决定 detail 字段填充（brief 最薄，full 全量）。 */
export function deriveCharacterDossier(
  index: EvidenceIndex,
  name: string,
  depth: DeconstructionDepth,
  totalChapters?: number,
): CharacterDossier {
  const evidence = index.byCharacter[name] ?? [];
  const appearances = evidence.map((entry) => entry.chapter);
  const firstChapter = appearances[0] ?? 0;
  const lastChapter = appearances[appearances.length - 1] ?? 0;

  let longestAbsence = 0;
  if (appearances.length >= 2) {
    for (let i = 1; i < appearances.length; i++) {
      longestAbsence = Math.max(longestAbsence, appearances[i]! - appearances[i - 1]! - 1);
    }
  }
  const span = totalChapters ?? (lastChapter || 1);
  const frequency = span > 0 && appearances.length > 0
    ? Math.round((appearances.length / span) * 10) / 10
    : 0;

  return {
    name,
    appearances,
    firstChapter,
    lastChapter,
    ...(depth === "deep" || depth === "full" ? { frequency } : {}),
    ...(depth === "deep" || depth === "full" ? { longestAbsence } : {}),
    evidence:
      depth === "full"
        ? evidence
        : evidence.slice(0, depth === "brief" ? 0 : depth === "standard" ? 3 : 10),
  };
}

export interface PacingStats {
  /** 章型计数（含无章型章计入 "untyped"）。 */
  readonly counts: Readonly<Record<string, number>>;
  /** 最长连续同型章数与章型。 */
  readonly longestRun: { readonly chapterType: string; readonly length: number };
  /** 强钩密度：hookActivity 非空的章占比（0–1，一位小数）。 */
  readonly strongHookDensity: number;
}

/** 节奏/卖点统计：章型分布 + 最长连续同型 + 强钩密度（确定性口径）。 */
export function analyzePacingStats(chapters: ReadonlyArray<DeconChapter>): PacingStats {
  const counts: Record<string, number> = {};
  let longestType = "";
  let longestLength = 0;
  let runType = "";
  let runLength = 0;
  let strongChapters = 0;

  for (const chapter of chapters) {
    const type = chapter.chapterType?.trim() || "untyped";
    counts[type] = (counts[type] ?? 0) + 1;
    if (type === runType) {
      runLength += 1;
    } else {
      runType = type;
      runLength = 1;
    }
    if (runLength > longestLength) {
      longestLength = runLength;
      longestType = type;
    }
    if (chapter.hookActivity?.trim()) strongChapters += 1;
  }
  const total = chapters.length || 1;
  return {
    counts,
    longestRun: { chapterType: longestType, length: longestLength },
    strongHookDensity: Math.round((strongChapters / total) * 10) / 10,
  };
}

/** 导出渲染：参考资料兼容 markdown（`## Extracted content` 后为拆书正文）。 */
export function renderDeconstructionExport(
  dossiers: ReadonlyArray<CharacterDossier>,
  pacing: PacingStats,
  language: "zh" | "en" = "zh",
): string {
  const isEn = language === "en";
  const header = isEn
    ? "# Deconstruction Export"
    : "# 拆书产物（人物档案 / 节奏 / 卖点）";
  const dossierBlocks = dossiers.map((dossier) => {
    const lines = [
      `### ${dossier.name}`,
      isEn
        ? `- Chapters: ${dossier.appearances.join(", ") || "-"} | span ${dossier.firstChapter}–${dossier.lastChapter} | frequency ${dossier.frequency}`
        : `- 出场章：${dossier.appearances.join(", ") || "-"}｜跨度 ${dossier.firstChapter}–${dossier.lastChapter}｜出场率 ${dossier.frequency}`,
    ];
    for (const entry of dossier.evidence) {
      lines.push(`  - ch${entry.chapter}: ${entry.evidence}`);
    }
    return lines.join("\n");
  });
  const body = [
    header,
    "",
    "## Extracted content",
    "",
    isEn ? "### Character dossiers" : "### 人物档案",
    ...dossierBlocks,
    "",
    isEn
      ? `### Pacing: longest same-type run = ${pacing.longestRun.length} (${pacing.longestRun.chapterType}); strong-hook density ${pacing.strongHookDensity}`
      : `### 节奏：最长连续同型 ${pacing.longestRun.length} 章（${pacing.longestRun.chapterType}）；强钩密度 ${pacing.strongHookDensity}`,
  ];
  return body.join("\n");
}

/** 聚合产物（一次性产齐：索引 + 档案 + 节奏 + 可发布 markdown）。 */
export interface DeconstructionResult {
  readonly index: EvidenceIndex;
  readonly dossiers: ReadonlyArray<CharacterDossier>;
  readonly pacing: PacingStats;
  readonly markdown: string;
}

/**
 * 一括聚合（统一拆书面端点的核心调用）：按出场证据数取前 topCharacters
 * （默认 5）个角色的 full 档，加上节奏统计渲染为可发布 markdown。
 */
export function buildDeconstructionExport(params: {
  readonly chapters: ReadonlyArray<DeconChapter>;
  readonly depth?: DeconstructionDepth;
  readonly language?: "zh" | "en";
  readonly topCharacters?: number;
}): DeconstructionResult {
  const index = buildEvidenceIndex(params.chapters);
  const pacing = analyzePacingStats(params.chapters);
  const topCharacters = params.topCharacters ?? 5;
  const dossiers = Object.entries(index.byCharacter)
    .sort((a, b) => b[1].length - a[1].length || comparePlainDecon(a[0], b[0]))
    .slice(0, Math.max(1, topCharacters))
    .map(([name]) => deriveCharacterDossier(index, name, "full", undefined));
  const markdown = renderDeconstructionExport(dossiers, pacing, params.language ?? "zh");
  return { index, dossiers, pacing, markdown };
}

function comparePlainDecon(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}
