/**
 * R20 系列正典共享（389 号契约批，四轮 P0；v4 §3 R20）。
 *
 * Sudowrite Series Folders 对标：跨书共享的正典层——同系列多书消费
 * 同一套角色/设定条目，作者一次编写、逐书命中注入。核心契约：
 *
 * 1. `parseSeriesCanonFile`：`.inkos/series/{seriesId}.json` 解析
 *    （version=1 整包校验；条目逐条校验非法跳过；产物按 name 码元序）；
 * 2. `mergeCodexLayers`：双层合并——book 同名条目**覆盖** series 条目
 *    （覆盖即省略：幸存 series 条目单独注入，book 块零改动）；
 * 3. `renderSeriesCodexBlock`：幸存条目命中 → 「## 系列实体卡」块（双语）。
 *
 * 条目与 EntityCodexCard 同构（kind 沿用名册五枚举，事件类条目落
 * "other"）；seriesId 为 snake_case slug（禁空格/连字符，≤64 码元）。
 *
 * 双端：`engine-rs/src/utils/series_canon.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/series-canon-vectors.json`（TS 断言
 * `golden-series-canon.test.ts`；Rust 差分 `tests/golden_series_canon_diff.rs`）。
 */

import { CODEX_LIMITS, type CodexMatch, type EntityCodexCard } from "./entity-codex.js";

export const SERIES_CANON_VERSION = 1 as const;

export interface SeriesCanonFile {
  readonly version: typeof SERIES_CANON_VERSION;
  readonly seriesId: string;
  readonly entries: ReadonlyArray<EntityCodexCard>;
}

const SERIES_ID_PATTERN = /^[a-z0-9_]{1,64}$/;

/** seriesId 规范：snake_case slug（禁空格/连字符/路径段，≤64 码元）。 */
export function isValidSeriesId(seriesId: string): boolean {
  return SERIES_ID_PATTERN.test(seriesId);
}

const CODEX_KINDS = new Set(["person", "place", "faction", "item", "other"]);
const SERIES_LIMITS = { name: 80, aliases: 10, alias: 60 } as const;

const clampText = (value: string, maxChars: number): string =>
  value.length > maxChars ? value.slice(0, maxChars) : value;

const byName = (a: EntityCodexCard, b: EntityCodexCard): number =>
  a.name < b.name ? -1 : a.name > b.name ? 1 : 0;

/** 条目校验（非法跳过/字段截断）；kind 非法兜底 other（对齐名册解析）。 */
function sanitizeEntry(raw: unknown): EntityCodexCard | undefined {
  if (typeof raw !== "object" || raw === null) return undefined;
  const record = raw as Record<string, unknown>;
  const name = typeof record.name === "string" ? record.name.trim() : "";
  if (!name) return undefined;
  const aliases = Array.isArray(record.aliases)
    ? record.aliases
        .map((alias) => (typeof alias === "string" ? alias.trim() : ""))
        .filter(Boolean)
        .slice(0, SERIES_LIMITS.aliases)
        .map((alias) => clampText(alias, SERIES_LIMITS.alias))
    : [];
  const kindRaw = typeof record.kind === "string" ? record.kind.trim() : "";
  const kind = (CODEX_KINDS.has(kindRaw) ? kindRaw : "other") as EntityCodexCard["kind"];
  const summary = typeof record.summary === "string" ? clampText(record.summary, CODEX_LIMITS.summary) : "";
  const facts = Array.isArray(record.facts)
    ? record.facts
        .map((fact) => (typeof fact === "string" ? fact.trim() : ""))
        .filter(Boolean)
        .slice(0, CODEX_LIMITS.facts)
        .map((fact) => clampText(fact, CODEX_LIMITS.fact))
    : [];
  const relationships = Array.isArray(record.relationships)
    ? record.relationships
        .map((relation) => {
          if (typeof relation !== "object" || relation === null) return undefined;
          const rel = relation as Record<string, unknown>;
          const target = typeof rel.target === "string" ? rel.target.trim() : "";
          if (!target) return undefined;
          const note = typeof rel.note === "string" ? clampText(rel.note, CODEX_LIMITS.relationshipNote) : "";
          return { target, note };
        })
        .filter((relation): relation is { target: string; note: string } => relation !== undefined)
        .slice(0, CODEX_LIMITS.relationships)
    : [];
  const firstChapter =
    typeof record.firstChapter === "number" &&
    Number.isInteger(record.firstChapter) &&
    record.firstChapter >= 0
      ? record.firstChapter
      : undefined;
  return {
    name: clampText(name, SERIES_LIMITS.name),
    aliases,
    kind,
    summary,
    facts,
    relationships,
    ...(firstChapter !== undefined ? { firstChapter } : {}),
  };
}

/** 解析系列正典文件（version/seriesId 整包校验；坏条目跳过；name 码元序）。 */
export function parseSeriesCanonFile(raw: unknown): SeriesCanonFile | undefined {
  if (typeof raw !== "object" || raw === null) return undefined;
  const record = raw as Record<string, unknown>;
  if (record.version !== SERIES_CANON_VERSION) return undefined;
  const seriesId = typeof record.seriesId === "string" ? record.seriesId : "";
  if (!isValidSeriesId(seriesId)) return undefined;
  if (!Array.isArray(record.entries)) return undefined;
  const entries = record.entries
    .map(sanitizeEntry)
    .filter((entry): entry is EntityCodexCard => entry !== undefined)
    .sort(byName);
  return { version: SERIES_CANON_VERSION, seriesId, entries };
}

export interface CodexLayerMerge {
  /** 书内卡片（原样返回，顺序不变）。 */
  readonly book: ReadonlyArray<EntityCodexCard>;
  /** 幸存 series 条目（同名被 book 覆盖剔除后，name 码元序）。 */
  readonly series: ReadonlyArray<EntityCodexCard>;
}

/** 双层合并：book 同名条目覆盖 series 条目（覆盖即省略，非交织）。 */
export function mergeCodexLayers(
  bookCards: ReadonlyArray<EntityCodexCard>,
  seriesEntries: ReadonlyArray<EntityCodexCard>,
): CodexLayerMerge {
  const bookNames = new Set(bookCards.map((card) => card.name));
  const series = seriesEntries.filter((entry) => !bookNames.has(entry.name)).sort(byName);
  return { book: bookCards, series };
}

const SERIES_EXCERPT_MAX = 400;

/** 注入渲染：幸存条目命中 → 「## 系列实体卡」块（无命中 undefined）。 */
export function renderSeriesCodexBlock(
  matches: ReadonlyArray<CodexMatch<EntityCodexCard>>,
  language: "zh" | "en" = "zh",
): string | undefined {
  if (matches.length === 0) return undefined;
  const isEn = language === "en";
  const sections = matches.map(({ card }) => {
    const aliasPart = card.aliases.length > 0 ? `（${card.aliases.join("、")}）` : "";
    const lines = [`### ${card.name}${aliasPart}`];
    if (card.summary) {
      lines.push(
        card.summary.length > SERIES_EXCERPT_MAX
          ? `${card.summary.slice(0, SERIES_EXCERPT_MAX)}…`
          : card.summary,
      );
    }
    if (card.facts.length > 0) {
      lines.push(isEn ? "Canon facts:" : "正典事实：");
      for (const fact of card.facts) lines.push(`- ${fact}`);
    }
    if (card.relationships.length > 0) {
      lines.push(isEn ? "Relationships:" : "关系：");
      for (const relation of card.relationships) {
        lines.push(`- ${relation.target}: ${relation.note}`);
      }
    }
    return lines.join("\n");
  });
  return [
    isEn
      ? "## Series entity cards (cross-book canon — follow canon facts)"
      : "## 系列实体卡（跨书正典——遵循正典事实）",
    ...sections,
  ].join("\n");
}
