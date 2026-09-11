/**
 * 实体名册 + 新专名确认卡（G7b/342 号，Phase B 批次二末项；WNW W6 的采纳）。
 *
 * 防止"同人异名 / 同名异人"：settle 链的章摘要 `characters` 列（LLM 从正文
 * 提取的出场人物）与本报名册逐一比对，未登记项生成确认卡三选——
 * **新实体 / 已有名子的别名 / 疑似笔误**。作者确认前不入册（先例：rename_entity
 * 只有名册内操作；新实体必须过确认卡）。

 * truth 类目 `story/entity_roster.md`（作者可手改，每条一节）：
 *
 *   ## entity-1
 *   - name: 林动
 *   - aliases: 林小哥, 少年林动
 *   - kind: person
 *   - registeredAt: 1
 *
 * 双端：`engine-rs/src/utils/entity_roster.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/entity-roster-vectors.json`。
 */

export interface RosterEntity {
  readonly id: string;
  /** 规范名。 */
  readonly name: string;
  readonly aliases: ReadonlyArray<string>;
  readonly kind: "person" | "place" | "faction" | "item" | "other";
  readonly registeredAt: number;
}

const ENTITY_HEADING = /^##\s+(entity-[\w-]+)\s*$/;
const FIELD_LINE = /^-\s*(\w+)\s*:\s*(.*)$/;
const VALID_KINDS = new Set(["person", "place", "faction", "item", "other"]);
const CHARACTER_SPLIT = /[,，、;；/]/;
const CHARACTER_STOPWORDS = /^(无|没有|暂无|none|n\/a|unknown|未知|others?|其他)$/i;

/** 解析实体名册（手改容错：缺 kind 归 other，坏节跳过）。 */
export function parseEntityRoster(markdown: string): RosterEntity[] {
  const entries: RosterEntity[] = [];
  let current: { id: string; fields: Map<string, string> } | undefined;
  const flush = () => {
    if (!current) return;
    const name = current.fields.get("name")?.trim() ?? "";
    if (!name) {
      current = undefined;
      return;
    }
    const aliases = splitList(current.fields.get("aliases") ?? "");
    const kindRaw = (current.fields.get("kind") ?? "other").trim();
    const kind = (VALID_KINDS.has(kindRaw) ? kindRaw : "other") as RosterEntity["kind"];
    const registeredAt = Number.parseInt((current.fields.get("registeredAt") ?? "").trim(), 10);
    entries.push({
      id: current.id,
      name,
      aliases,
      kind,
      registeredAt: Number.isFinite(registeredAt) ? registeredAt : 0,
    });
    current = undefined;
  };

  for (const rawLine of markdown.split(/\r?\n/)) {
    const line = rawLine.trim();
    const heading = ENTITY_HEADING.exec(line);
    if (heading) {
      flush();
      current = { id: heading[1]!, fields: new Map() };
      continue;
    }
    if (!current) continue;
    const field = FIELD_LINE.exec(line);
    if (field) current.fields.set(field[1]!, field[2] ?? "");
  }
  flush();
  return entries;
}

function splitList(value: string): string[] {
  return value
    .split(/[,，、;；]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

/** 渲染名册（防呆方言：每条一节、平铺 key: value 行）。 */
export function renderEntityRoster(entries: ReadonlyArray<RosterEntity>): string {
  const sections = entries.map((entry) =>
    [
      `## ${entry.id}`,
      `- name: ${entry.name}`,
      `- aliases: ${entry.aliases.join(", ")}`,
      `- kind: ${entry.kind}`,
      `- registeredAt: ${entry.registeredAt}`,
    ].join("\n"),
  );
  return ["# 实体名册（Entity Roster）", ...sections].join("\n\n");
}

/** 从章摘要 characters 列提取人名候选（去重保序、剔除停用词）。 */
export function extractCharacterCandidates(
  summaries: ReadonlyArray<{ readonly chapter: number; readonly characters: string }>,
): string[] {
  const seen = new Set<string>();
  const ordered: string[] = [];
  for (const summary of [...summaries].sort((a, b) => a.chapter - b.chapter)) {
    for (const raw of summary.characters.split(CHARACTER_SPLIT)) {
      const name = raw.trim();
      if (!name || CHARACTER_STOPWORDS.test(name)) continue;
      const key = name.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      ordered.push(name);
    }
  }
  return ordered;
}

/** 经典 Levenshtein 距离（双端一致的纯 DP 实现）。 */
export function editDistance(a: string, b: string): number {
  const aChars = [...a];
  const bChars = [...b];
  if (aChars.length === 0) return bChars.length;
  if (bChars.length === 0) return aChars.length;
  let previous = Array.from({ length: bChars.length + 1 }, (_, i) => i);
  for (let i = 1; i <= aChars.length; i++) {
    const currentRow = [i];
    for (let j = 1; j <= bChars.length; j++) {
      const substitution = previous[j - 1]! + (aChars[i - 1] === bChars[j - 1] ? 0 : 1);
      currentRow.push(Math.min(previous[j]! + 1, currentRow[j - 1]! + 1, substitution));
    }
    previous = currentRow;
  }
  return previous[bChars.length]!;
}

/** 确认卡三选动作。 */
export type RosterCandidateAction = "new-entity" | "alias" | "typo";

export interface RosterCandidateCard {
  readonly candidate: string;
  readonly action: RosterCandidateAction;
  /** alias/typo 指向的规范名。 */
  readonly targetName?: string;
  readonly confidence: "high" | "medium" | "low";
}

const CONFIDENCE_RANK: Record<RosterCandidateCard["confidence"], number> = {
  high: 0,
  medium: 1,
  low: 2,
};
const ACTION_RANK: Record<RosterCandidateAction, number> = {
  typo: 0,
  alias: 1,
  "new-entity": 2,
};

/**
 * 新专名比对（settle 时调用）：候选与名册逐一比对，未登记项生成确认卡。
 * 判定顺序：
 * 1. 精确命中（name/aliases，大小写不敏感）→ 已登记，不出卡；
 * 2. 与 name 或 alias 编辑距离 ≤1（且候选长度 ≥2）→ typo（指向规范名，high）；
 * 3. 包含关系（候选含 name 或被 name 包含，较短者 ≥2 字）→ alias（指向规范名，medium）；
 * 4. 其余 → new-entity（low）。
 * 排序：action(typo→alias→new) → confidence → candidate 码元序。
 */
export function resolveRosterCandidates(
  candidates: ReadonlyArray<string>,
  roster: ReadonlyArray<RosterEntity>,
): RosterCandidateCard[] {
  const cards: RosterCandidateCard[] = [];
  for (const candidate of candidates) {
    const key = candidate.toLowerCase();
    let matched = false;
    for (const entity of roster) {
      if (entity.name.toLowerCase() === key || entity.aliases.some((a) => a.toLowerCase() === key)) {
        matched = true;
        break;
      }
    }
    if (matched) continue;

    // G7b 判定顺序：包含关系（儿/阿/小缀昵称等）→ 别名；编辑距离 ≤1 → 笔误；
    // 两者都不中 → 新实体。（先昵称后笔误：中文昵称多为后缀增删，会被距离 1 误判）。
    let aliasTarget: string | undefined;
    let typoTarget: string | undefined;
    for (const entity of roster) {
      const nameChars = [...entity.name].length;
      const contained =
        nameChars >= 2 && (candidate.includes(entity.name) || entity.name.includes(candidate));
      if (contained) {
        aliasTarget = entity.name;
        break;
      }
      if (nameChars >= 2 && editDistance(candidate, entity.name) <= 1) {
        typoTarget = entity.name;
        break;
      }
      for (const alias of entity.aliases) {
        if ([...alias].length >= 2 && editDistance(candidate, alias) <= 1) {
          aliasTarget = entity.name;
          break;
        }
      }
      if (aliasTarget) break;
    }
    if (aliasTarget) {
      cards.push({ candidate, action: "alias", targetName: aliasTarget, confidence: "medium" });
      continue;
    }
    if (typoTarget) {
      cards.push({ candidate, action: "typo", targetName: typoTarget, confidence: "high" });
      continue;
    }
    cards.push({ candidate, action: "new-entity", confidence: "low" });
  }
  return cards.sort(
    (a, b) =>
      ACTION_RANK[a.action] - ACTION_RANK[b.action]
      || CONFIDENCE_RANK[a.confidence] - CONFIDENCE_RANK[b.confidence]
      || comparePlain(a.candidate, b.candidate),
  );
}

/** 纯码元比较（禁 localeCompare——排序跨端确定）。 */
function comparePlain(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** 确认动作输入（确认卡三选的落库口径）。 */
export interface RosterConfirmation {
  readonly candidate: string;
  readonly action: RosterCandidateAction;
  readonly targetName?: string;
  /** 登记章（new-entity 用；缺省 0）。 */
  readonly chapter?: number;
}

export interface RosterConfirmationResult {
  readonly roster: ReadonlyArray<RosterEntity>;
  readonly applied:
    | { readonly kind: "new-entity"; readonly id: string }
    | { readonly kind: "alias"; readonly targetName: string }
    | { readonly kind: "typo-ignored" }
    | { readonly kind: "duplicate-ignored" };
}

/**
 * 确认写回（纯函数）：
 * - new-entity → 追加 entity-N+1（registeredAt = chapter ?? 0）；
 * - alias → candidate 并入 targetName 实体的 aliases（去重）；
 * - typo → 不入册（笔误修正属正文修订范畴），kind=typo-ignored；
 * - 重复确认（实体已存在/别名已存在）→ duplicate-ignored。
 */
export function applyRosterConfirmation(
  roster: ReadonlyArray<RosterEntity>,
  confirmation: RosterConfirmation,
): RosterConfirmationResult {
  const key = confirmation.candidate.toLowerCase();
  const alreadyKnown = roster.some(
    (entity) =>
      entity.name.toLowerCase() === key
      || entity.aliases.some((alias) => alias.toLowerCase() === key),
  );
  if (alreadyKnown) {
    return { roster, applied: { kind: "duplicate-ignored" } };
  }

  if (confirmation.action === "new-entity") {
    let maxIndex = 0;
    for (const entity of roster) {
      const match = /^entity-(\d+)$/.exec(entity.id);
      if (match) maxIndex = Math.max(maxIndex, Number.parseInt(match[1]!, 10));
    }
    const entry: RosterEntity = {
      id: `entity-${maxIndex + 1}`,
      name: confirmation.candidate,
      aliases: [],
      kind: "person",
      registeredAt: confirmation.chapter ?? 0,
    };
    return { roster: [...roster, entry], applied: { kind: "new-entity", id: entry.id } };
  }

  if (confirmation.action === "alias" && confirmation.targetName) {
    const target = roster.find((entity) => entity.name === confirmation.targetName);
    if (!target) return { roster, applied: { kind: "duplicate-ignored" } };
    const updated = roster.map((entity) =>
      entity.id === target.id && !entity.aliases.includes(confirmation.candidate)
        ? { ...entity, aliases: [...entity.aliases, confirmation.candidate] }
        : entity,
    );
    return { roster: updated, applied: { kind: "alias", targetName: target.name } };
  }

  return { roster, applied: { kind: "typo-ignored" } };
}

/** 确认卡人话渲染（聊天确认卡/审计注入共用）。 */
export function renderRosterConfirmationCard(
  cards: ReadonlyArray<RosterCandidateCard>,
  language: "zh" | "en" = "zh",
): string | undefined {
  if (cards.length === 0) return undefined;
  const isEn = language === "en";
  const lines = cards.map((card) => {
    if (card.action === "typo") {
      return isEn
        ? `- "${card.candidate}" → possible typo of "${card.targetName}"?`
        : `- "${card.candidate}" → 疑似 "${card.targetName}" 的笔误？`;
    }
    if (card.action === "alias") {
      return isEn
        ? `- "${card.candidate}" → possible alias of "${card.targetName}"?`
        : `- "${card.candidate}" → 可能是 "${card.targetName}" 的别名？`;
    }
    return isEn
      ? `- "${card.candidate}" → new entity?`
      : `- "${card.candidate}" → 新实体？`;
  });
  return [
    isEn ? "Unregistered proper nouns found in this chapter (new entity / alias / typo):" : "本章发现未登记专名（新实体 / 别名 / 笔误）：",
    ...lines,
  ].join("\n");
}
