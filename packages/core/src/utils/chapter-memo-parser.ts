import { ChapterMemoSchema, ReaderExperienceSchema, type ChapterMemo, type ReaderExperience } from "../models/input-governance.js";

export class PlannerParseError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PlannerParseError";
  }
}

// Phase hotfix 4: each required section is a (zh, en) heading pair.
// The English headings come from PLANNER_MEMO_SYSTEM_PROMPT_EN — we accept
// EITHER language at parse time so the same parser works for both.
//
// Phase hotfix 7: minContentChars enforces non-emptiness per section so
// "all required headings + blank payload" no longer slips through. The "do not"
// section uses a relaxed threshold because "无 / N/A / none." is legitimate
// for chapters with no extra prohibitions.
//
// Threshold rationale:
// - 20 chars: long enough to catch obvious empty sections (whitespace,
//   "(略)", "TODO") but short enough to accept genuinely sparse memos for
//   breath/transition chapters (Phase 6 sparse-memo principle).
// - 1 char for "## 不要做" / "## Do not" because "无" / "N/A" / "none" /
//   "—" are all legitimate for a chapter with no extra prohibitions; we
//   only need to ensure the section is not whitespace-only.
interface RequiredSection {
  readonly zh: string;
  readonly en: string;
  readonly minContentChars: number;
}

const REQUIRED_SECTIONS: ReadonlyArray<RequiredSection> = [
  { zh: "## 场景与篇幅预算", en: "## Scene and length budget", minContentChars: 20 },
  { zh: "## 当前任务", en: "## Current task", minContentChars: 20 },
  { zh: "## 读者此刻在等什么", en: "## What the reader is waiting for right now", minContentChars: 20 },
  { zh: "## 该兑现的 / 暂不掀的", en: "## To pay off / to keep buried", minContentChars: 20 },
  { zh: "## 日常/过渡承担什么任务", en: "## What the slow / transitional beats carry", minContentChars: 20 },
  { zh: "## 关键抉择过三连问", en: "## Three-question check on the key choice", minContentChars: 20 },
  { zh: "## 章尾必须发生的改变", en: "## Required end-of-chapter change", minContentChars: 20 },
  { zh: "## 本章 hook 账", en: "## Hook ledger for this chapter", minContentChars: 20 },
  { zh: "## 不要做", en: "## Do not", minContentChars: 1 },
];

const GOAL_HEADINGS = ["## 本章目标", "## Chapter goal"] as const;
const THREAD_HEADINGS = ["## 关联线索", "## Thread refs", "## Related threads"] as const;

// ---------------------------------------------------------------------------
// R1 读者体验合同（357 号）——「读者体验合同」节的可选结构化提取。
//
// 兼容性策略：该节**不进** REQUIRED_SECTIONS。persisted-governed-plan 会重放
// 解析存量 intent markdown，旧书 memo 没有这一节，必填化会把历史计划全部判死。
// 因此节存在且七个叙事字段齐全才产出 readerExperience，否则为 undefined，
// 审稿维度 40（追读承接）随之跳过。字段值截断到 200 UTF-16 码元后入 schema。
// ---------------------------------------------------------------------------

const CONTRACT_HEADINGS = ["## 读者体验合同", "## Reader experience contract"] as const;

const CONTRACT_MAX_FIELD_UNITS = 200;
const CONTRACT_MAX_TITLE_UNITS = 60;

interface ContractFieldSpec {
  readonly key: keyof Omit<ReaderExperience, "titleCandidates">;
  readonly zh: string;
  readonly en: string;
}

const CONTRACT_FIELDS: ReadonlyArray<ContractFieldSpec> = [
  { key: "previousHandoff", zh: "开头承接", en: "previousHandoff" },
  { key: "readerQuestion", zh: "读者问题", en: "readerQuestion" },
  { key: "promisePayoff", zh: "承诺兑现", en: "promisePayoff" },
  { key: "protagonistWant", zh: "主角欲求", en: "protagonistWant" },
  { key: "protagonistObstacle", zh: "主角障碍", en: "protagonistObstacle" },
  { key: "sceneTurn", zh: "场景转折", en: "sceneTurn" },
  { key: "endingNetChange", zh: "章末净变化", en: "endingNetChange" },
];

const CONTRACT_TITLE_LABELS = ["章名候选", "titleCandidates"] as const;

function utf16Truncate(value: string, maxUnits: number): string {
  let units = 0;
  let out = "";
  for (const char of value) {
    const len = char.length;
    if (units + len > maxUnits) break;
    out += char;
    units += len;
  }
  return out.trim();
}

/** 取合同节的原始行（不折叠空白，与 extractSectionContent 的折叠版不同）。 */
function extractContractSectionLines(body: string): string[] {
  for (const heading of CONTRACT_HEADINGS) {
    const start = body.indexOf(heading);
    if (start < 0) continue;
    const after = body.slice(start + heading.length);
    const next = after.match(/\n##\s/);
    const raw = next ? after.slice(0, next.index) : after;
    return raw.split("\n").map((line) => line.trim()).filter(Boolean);
  }
  return [];
}

function matchContractField(line: string, labels: ReadonlyArray<string>): string | undefined {
  const match = line.match(/^(?:-\s*)?(.+?)\s*[：:]\s*(.*)$/);
  if (!match) return undefined;
  const label = match[1]!.trim();
  if (!labels.some((candidate) => label.toLowerCase() === candidate.toLowerCase())) return undefined;
  return match[2]!.trim();
}

/**
 * 从 memo body 提取读者体验合同。七个叙事字段全部命中才产出完整合同，
 * 缺任一字段返回 undefined（整节视为未携带）。titleCandidates 可选，
 * 按 ｜ / ｜、/ 分隔，最多取 3 个。
 */
function extractReaderExperience(body: string): ReaderExperience | undefined {
  const lines = extractContractSectionLines(body);
  if (lines.length === 0) return undefined;

  const values = new Map<string, string>();
  for (const line of lines) {
    for (const field of CONTRACT_FIELDS) {
      if (values.has(field.key)) continue;
      const value = matchContractField(line, [field.zh, field.en]);
      if (value !== undefined && value.length > 0) {
        values.set(field.key, value);
      }
    }
  }
  if (values.size < CONTRACT_FIELDS.length) return undefined;

  const titleCandidates: string[] = [];
  for (const line of lines) {
    const value = matchContractField(line, CONTRACT_TITLE_LABELS);
    if (value === undefined) continue;
    for (const part of value.split(/[｜|、／]/)) {
      const cleaned = part.trim();
      if (cleaned.length === 0) continue;
      if (titleCandidates.includes(cleaned)) continue;
      titleCandidates.push(utf16Truncate(cleaned, CONTRACT_MAX_TITLE_UNITS));
      if (titleCandidates.length >= 3) break;
    }
    break;
  }

  const record: Record<string, string> = {};
  for (const field of CONTRACT_FIELDS) {
    record[field.key] = utf16Truncate(values.get(field.key)!, CONTRACT_MAX_FIELD_UNITS);
  }
  return ReaderExperienceSchema.parse({ ...record, titleCandidates });
}

/**
 * Extract the content between `heading` and the next `## ...` heading (or
 * end-of-body). Strips whitespace and returns "" if the section payload is
 * absent. The heading itself is NOT included.
 */
function extractSectionContent(body: string, heading: string): string {
  const startIndex = body.indexOf(heading);
  if (startIndex < 0) return "";
  const after = body.slice(startIndex + heading.length);
  // Find the next H2 heading on its own line. The leading newline + ## guards
  // against false matches inside the current section's prose.
  const nextHeadingMatch = after.match(/\n##\s/);
  const sectionRaw = nextHeadingMatch
    ? after.slice(0, nextHeadingMatch.index)
    : after;
  return sectionRaw.replace(/\s+/g, " ").trim();
}

function stripWrappingFence(raw: string): string {
  const trimmed = raw.trim();
  const fenced = trimmed.match(/^```(?:md|markdown)?\s*\n([\s\S]*?)\n```\s*$/i);
  return fenced?.[1]?.trim() ?? trimmed;
}

function dropLeadingProse(raw: string): string {
  const markers = [
    "# 第 ",
    "# Chapter ",
    ...GOAL_HEADINGS,
    ...THREAD_HEADINGS,
    ...REQUIRED_SECTIONS.flatMap((section) => [section.zh, section.en]),
  ];
  let first = -1;
  for (const marker of markers) {
    const index = raw.indexOf(marker);
    if (index >= 0 && (first < 0 || index < first)) {
      first = index;
    }
  }
  return first >= 0 ? raw.slice(first).trim() : raw.trim();
}

function extractAnyHeading(body: string, headings: ReadonlyArray<string>): string {
  for (const heading of headings) {
    const content = extractSectionContent(body, heading);
    if (content) return content;
  }
  return "";
}

function extractGoal(body: string): string {
  const explicitGoal = extractAnyHeading(body, GOAL_HEADINGS);
  if (explicitGoal) {
    return explicitGoal.split(/\n|。|\. /)[0]?.trim() ?? "";
  }
  return "";
}

function extractThreadRefs(body: string): string[] {
  const block = extractAnyHeading(body, THREAD_HEADINGS);
  if (!block || /^(无|none|n\/a|na|—|-|\(none\))$/i.test(block.trim())) {
    return [];
  }
  const matches = block.match(/\b[A-Za-z][A-Za-z0-9_-]*\d+[A-Za-z0-9_-]*\b/g) ?? [];
  return [...new Set(matches)];
}

function extractMemoBody(markdown: string): string {
  const starts = REQUIRED_SECTIONS
    .flatMap((section) => [section.zh, section.en])
    .map((heading) => markdown.indexOf(heading))
    .filter((index) => index >= 0);
  if (starts.length === 0) return markdown.trim();
  return markdown.slice(Math.min(...starts)).trim();
}

function makeDisplayGoal(goal: string): string {
  if (goal.length <= 50) return goal;
  return `${goal.slice(0, 47).trimEnd()}...`;
}

function prependFullGoalIfNeeded(markdown: string, body: string, fullGoal: string, displayGoal: string): string {
  if (fullGoal === displayGoal) return body;
  const heading = markdown.includes("## Chapter goal") ? "## Chapter goal" : "## 本章目标";
  return `${heading}\n${fullGoal}\n\n${body}`;
}

/**
 * Parse a planner memo produced by the LLM.
 *
 * Format: plain Markdown containing a `## 本章目标` / `## Chapter goal`
 * section, an optional thread-ref section, and the required memo section
 * headings.
 *
 * Strict on the LLM-owned memo sections. Caller-owned fields (chapter /
 * golden-opening) come from the host, not from the model. A long chapter goal
 * is kept in the memo body and reduced only to a short display label for the
 * schema field, so parser robustness does not silently delete planning intent.
 *
 * The parser strips a wrapping Markdown code fence and any leading assistant
 * prose ("好的，下面是...") before the first memo heading. It does not accept
 * YAML frontmatter as a required model protocol anymore.
 */
export function parseMemo(
  raw: string,
  expectedChapter: number,
  isGoldenOpening: boolean,
): ChapterMemo {
  const markdown = dropLeadingProse(stripWrappingFence(raw));
  const goal = extractGoal(markdown);
  const body = extractMemoBody(markdown);
  const threadRefs = extractThreadRefs(markdown);

  if (goal.length === 0) {
    throw new PlannerParseError("goal must be a non-empty string");
  }
  const displayGoal = makeDisplayGoal(goal);

  const missing = REQUIRED_SECTIONS.filter(
    (section) => !body.includes(section.zh) && !body.includes(section.en),
  );
  if (missing.length > 0) {
    // Report by zh heading (canonical) so the LLM-feedback loop stays stable.
    throw new PlannerParseError(
      `missing sections: ${missing.map((s) => s.zh).join(", ")}`,
    );
  }

  // Phase hotfix 7: each section's payload must be non-empty (≥ minContentChars).
  // Headings present + blank payload was previously accepted, allowing useless
  // "shell" memos to flow downstream. Threshold differs per section: most need
  // 20 chars (one short sentence) while "## 不要做" / "## Do not" allows 1
  // (e.g. "无", "N/A") since "no extra prohibitions" is a legitimate state.
  const empty = REQUIRED_SECTIONS.filter((section) => {
    const heading = body.includes(section.zh) ? section.zh : section.en;
    const content = extractSectionContent(body, heading);
    return content.length < section.minContentChars;
  });
  if (empty.length > 0) {
    const detail = empty
      .map((s) => `${s.zh} (need ≥ ${s.minContentChars} chars)`)
      .join(", ");
    throw new PlannerParseError(`empty sections: ${detail}`);
  }

  const readerExperience = extractReaderExperience(body);
  return ChapterMemoSchema.parse({
    chapter: expectedChapter,
    goal: displayGoal,
    isGoldenOpening,
    body: prependFullGoalIfNeeded(markdown, body, goal, displayGoal),
    threadRefs,
    ...(readerExperience ? { readerExperience } : {}),
  });
}
