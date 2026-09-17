/**
 * R10 实体卡 Codex 化（375 号契约批，三轮 P0；v3 §3 R10）。
 *
 * 实体卡 = 名册条目的**视图扩展**（同源派生，非第二真相源）：名册
 * （G7b entity_roster.md）仍是唯一身份真相，卡片在视图层补充摘要/
 * 硬事实/关系供写作注入。核心契约：
 *
 * 1. `deriveCodexCards`：名册 → 卡片视图（同源派生，作者编辑只发生在
 *    卡片扩展字段上，name/aliases/kind 永远以名册为准）；
 * 2. `matchCodexCards`：场景文本（正文/备忘）按 name+aliases 命中卡片，
 *    命中数降序稳定排序（禁 localeCompare）；
 * 3. `renderCodexBlock`：命中卡片 → 「## 实体卡」注入块（双语）。
 *
 * 双端：`engine-rs/src/utils/entity_codex.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/entity-codex-vectors.json`（TS 断言
 * `golden-entity-codex.test.ts`；Rust 差分 `tests/golden_entity_codex_diff.rs`）。
 */

import type { RosterEntity } from "./entity-roster.js";

export interface CodexRelationship {
  readonly target: string;
  readonly note: string;
}

export interface EntityCodexCard {
  /** 与名册 name 一致（同源键）。 */
  readonly name: string;
  readonly aliases: ReadonlyArray<string>;
  readonly kind: RosterEntity["kind"];
  /** 卡片摘要（谁/什么，≤300 码元；名册派生时为空，作者可编辑）。 */
  readonly summary: string;
  /** 硬事实（正典约束，≤10 条 × ≤200 码元）。 */
  readonly facts: ReadonlyArray<string>;
  /** 关系（≤10 条）。 */
  readonly relationships: ReadonlyArray<CodexRelationship>;
  /** 首次出场章（名册 registeredAt 不可用时缺省）。 */
  readonly firstChapter?: number;
}

export const CODEX_LIMITS = {
  summary: 300,
  facts: 10,
  fact: 200,
  relationships: 10,
  relationshipNote: 200,
} as const;

/** 名册 → 卡片视图（同源派生）：summary/facts 置空，身份字段以名册为准。 */
export function deriveCodexCards(roster: ReadonlyArray<RosterEntity>): EntityCodexCard[] {
  return roster
    .map((entity) => ({
      name: entity.name,
      aliases: [...entity.aliases],
      kind: entity.kind,
      summary: "",
      facts: [] as string[],
      relationships: [] as CodexRelationship[],
    }))
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

const clampList = (items: ReadonlyArray<string>, maxItems: number, maxChars: number): string[] =>
  items
    .map((item) => item.trim())
    .filter(Boolean)
    .slice(0, maxItems)
    .map((item) => (item.length > maxChars ? item.slice(0, maxChars) : item));

/**
 * 518 号：**消费面**卡归一化（Rust 侧 EntityCodexCard 全字段 `#[serde(default)]`
 * 的 TS 对应物；读写端点双端均 Value 原样透传，容差只发生在进管线前）。
 * 缺 aliases 的卡此前在 matchCodexCards 展开 `...card.aliases` 即崩
 * （差分器实证：PUT codex 200 → resync 500）。未知字段（如 Rust 回显的 id）
 * spread 保留；kind 仅补缺省不校验取值（Rust 侧为 String 无枚举约束）。
 */
export function normalizeCodexCards(cards: ReadonlyArray<unknown>): EntityCodexCard[] {
  const out: EntityCodexCard[] = [];
  for (const card of cards) {
    if (!card || typeof card !== "object") continue;
    const raw = card as Record<string, unknown>;
    if (typeof raw.name !== "string" || raw.name.length === 0) continue;
    out.push({
      ...raw,
      name: raw.name,
      aliases: Array.isArray(raw.aliases)
        ? raw.aliases.filter((alias): alias is string => typeof alias === "string")
        : [],
      kind: (typeof raw.kind === "string" ? raw.kind : "other") as EntityCodexCard["kind"],
      summary: typeof raw.summary === "string" ? raw.summary : "",
      facts: Array.isArray(raw.facts)
        ? raw.facts.filter((fact): fact is string => typeof fact === "string")
        : [],
      relationships: Array.isArray(raw.relationships)
        ? raw.relationships.filter(
            (relation): relation is CodexRelationship =>
              !!relation && typeof relation === "object"
                && typeof (relation as CodexRelationship).target === "string"
                && typeof (relation as CodexRelationship).note === "string",
          )
        : [],
    } as EntityCodexCard);
  }
  return out;
}

/** 卡片修订：以名册同源卡为底座合并作者扩展（summary/facts/relationships）。 */
export function reviseCodexCard(
  base: EntityCodexCard,
  revision: Partial<Pick<EntityCodexCard, "summary" | "facts" | "relationships">>,
): EntityCodexCard {
  return {
    ...base,
    summary: (revision.summary ?? base.summary).slice(0, CODEX_LIMITS.summary),
    facts: clampList(revision.facts ?? base.facts, CODEX_LIMITS.facts, CODEX_LIMITS.fact),
    relationships: (revision.relationships ?? base.relationships)
      .slice(0, CODEX_LIMITS.relationships)
      .map((relation) => ({
        target: relation.target,
        note: relation.note.slice(0, CODEX_LIMITS.relationshipNote),
      })),
  };
}

export interface CodexMatch<T> {
  readonly card: T;
  /** 命中次数（name 与 alias 合并计）。 */
  readonly hits: number;
}

/** 场景命中：text 含 name 或任一 alias 的卡片（0 命中不返回）。 */
export function matchCodexCards<T extends EntityCodexCard>(
  text: string,
  cards: ReadonlyArray<T>,
): Array<CodexMatch<T>> {
  const matches: Array<CodexMatch<T>> = [];
  for (const card of cards) {
    let hits = 0;
    for (const term of [card.name, ...card.aliases]) {
      if (!term) continue;
      let from = 0;
      while (from <= text.length) {
        const index = text.indexOf(term, from);
        if (index < 0) break;
        hits += 1;
        from = index + Math.max(1, term.length);
      }
    }
    if (hits > 0) matches.push({ card, hits });
  }
  return matches.sort(
    (a, b) =>
      b.hits - a.hits ||
      (a.card.name < b.card.name ? -1 : a.card.name > b.card.name ? 1 : 0),
  );
}

const CODEX_EXCERPT_MAX = 400;

/** 注入渲染：命中卡片 → 「## 实体卡」块（无命中返回 undefined）。 */
export function renderCodexBlock(
  matches: ReadonlyArray<CodexMatch<EntityCodexCard>>,
  language: "zh" | "en" = "zh",
): string | undefined {
  if (matches.length === 0) return undefined;
  const isEn = language === "en";
  const sections = matches.map(({ card }) => {
    const aliasPart = card.aliases.length > 0 ? `（${card.aliases.join("、")}）` : "";
    const lines = [`### ${card.name}${aliasPart}`];
    if (card.summary) {
      lines.push(card.summary.length > CODEX_EXCERPT_MAX ? `${card.summary.slice(0, CODEX_EXCERPT_MAX)}…` : card.summary);
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
    isEn ? "## Entity cards (scene cast — follow canon facts)" : "## 实体卡（本场出场——遵循正典事实）",
    ...sections,
  ].join("\n");
}
