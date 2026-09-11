/**
 * 信息差账本（G7a/341 号，Phase B 批次二；WNW W5 信息差登记 + 泄密机检的采纳）。
 *
 * truth 类目 `story/info_gaps.md`（作者可手改，每条一节）：
 *
 *   ## gap-1
 *   - secret: 主角身世是前朝皇室
 *   - knows: 林动, 王胖
 *   - readerKnows: false
 *   - registeredAt: 3
 *   - keywords: 皇室, 前朝, 血脉

 * 机检两枚（确定性、无 LLM，供 continuity 审计维度 33/34 的本地预扫描）：
 * - **泄密机检** `detectSecretLeaks`：未到读者已知时点的秘密关键词出现在正文中，
 *   命中窗口含对白引号则升级（对白/内心戏泄密风险最高）；
 * - **废笔机检** `detectReaderRedundancy`：读者已知的秘密关键词再次复述——向读者
 *   复述已知信息属废笔。
 *
 * 双端：`engine-rs/src/utils/info_gap_ledger.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/info-gap-vectors.json`。
 */

export interface InfoGapEntry {
  readonly id: string;
  readonly secret: string;
  /** 知情角色（逗号/顿号分隔存储）。 */
  readonly knows: ReadonlyArray<string>;
  /** 读者是否已知情（true 后再复述即废笔）。 */
  readonly readerKnows: boolean;
  /** 登记章（秘密埋设的章节号）。 */
  readonly registeredAt: number;
  readonly keywords: ReadonlyArray<string>;
}

const GAP_HEADING = /^##\s+(gap-[\w-]+)\s*$/;
const FIELD_LINE = /^-\s*(\w+)\s*:\s*(.*)$/;

/** 解析信息差账本 markdown（作者手改容错：缺字段用空值，坏节跳过）。 */
export function parseInfoGapsMarkdown(markdown: string): InfoGapEntry[] {
  const entries: InfoGapEntry[] = [];
  let current: { id: string; fields: Map<string, string> } | undefined;
  const flush = () => {
    if (!current) return;
    const secret = current.fields.get("secret")?.trim() ?? "";
    const knows = splitList(current.fields.get("knows") ?? "");
    const readerKnows = /^(true|是|yes|1)$/i.test((current.fields.get("readerKnows") ?? "").trim());
    const registeredAt = Number.parseInt((current.fields.get("registeredAt") ?? "").trim(), 10);
    const keywords = splitList(current.fields.get("keywords") ?? "");
    if (secret || keywords.length > 0) {
      entries.push({
        id: current.id,
        secret,
        knows,
        readerKnows,
        registeredAt: Number.isFinite(registeredAt) ? registeredAt : 0,
        keywords,
      });
    }
    current = undefined;
  };

  for (const rawLine of markdown.split(/\r?\n/)) {
    const line = rawLine.trim();
    const heading = GAP_HEADING.exec(line);
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

/** 渲染信息差账本（防呆方言：每条一节、平铺 key: value 行）。 */
export function renderInfoGapsMarkdown(entries: ReadonlyArray<InfoGapEntry>): string {
  const sections = entries.map((entry) =>
    [
      `## ${entry.id}`,
      `- secret: ${entry.secret}`,
      `- knows: ${entry.knows.join(", ")}`,
      `- readerKnows: ${entry.readerKnows}`,
      `- registeredAt: ${entry.registeredAt}`,
      `- keywords: ${entry.keywords.join(", ")}`,
    ].join("\n"),
  );
  return ["# 信息差账本（Info Gaps）", ...sections].join("\n\n");
}

// ── 机检 ──

const DIALOGUE_MARKS = /[「」『』“”‘’"']/;

export interface SecretLeakHit {
  readonly gapId: string;
  readonly keyword: string;
  readonly occurrences: number;
  /** 命中窗口含对白引号（对白/内心泄密风险最高）。 */
  readonly inDialogue: boolean;
  readonly knows: ReadonlyArray<string>;
}

/**
 * 泄密机检：readerKnows=false 的秘密关键词出现在正文中即候选；
 * 命中位置前后 24 字窗口含对白引号 → inDialogue=true（升级为必须审查）。
 * 同 gap 多关键词按命中数聚合，取最高危窗口判定 inDialogue。
 */
export function detectSecretLeaks(params: {
  readonly content: string;
  readonly gaps: ReadonlyArray<InfoGapEntry>;
  readonly currentChapter: number;
}): SecretLeakHit[] {
  const hits: SecretLeakHit[] = [];
  for (const gap of params.gaps) {
    if (gap.readerKnows) continue;
    if (gap.registeredAt > params.currentChapter) continue; // 未登记（未来秘密）不扫
    for (const keyword of gap.keywords) {
      if (!keyword) continue;
      const occurrences = countOccurrences(params.content, keyword);
      if (occurrences === 0) continue;
      const inDialogue = windowHasDialogue(params.content, keyword);
      hits.push({ gapId: gap.id, keyword, occurrences, inDialogue, knows: [...gap.knows] });
    }
  }
  return hits.sort(
    (a, b) =>
      Number(b.inDialogue) - Number(a.inDialogue)
      || b.occurrences - a.occurrences
      || a.gapId.localeCompare(b.gapId)
      || a.keyword.localeCompare(b.keyword),
  );
}

export interface ReaderRedundancyHit {
  readonly gapId: string;
  readonly keyword: string;
  readonly occurrences: number;
}

/** 废笔机检：读者已知的秘密关键词再次出现——向读者复述已知信息。 */
export function detectReaderRedundancy(params: {
  readonly content: string;
  readonly gaps: ReadonlyArray<InfoGapEntry>;
}): ReaderRedundancyHit[] {
  const hits: ReaderRedundancyHit[] = [];
  for (const gap of params.gaps) {
    if (!gap.readerKnows) continue;
    for (const keyword of gap.keywords) {
      if (!keyword) continue;
      const occurrences = countOccurrences(params.content, keyword);
      if (occurrences > 0) {
        hits.push({ gapId: gap.id, keyword, occurrences });
      }
    }
  }
  return hits.sort(
    (a, b) =>
      b.occurrences - a.occurrences
      || comparePlain(a.gapId, b.gapId)
      || comparePlain(a.keyword, b.keyword),
  );
}

/** 纯码元比较（禁 localeCompare——排序必须跨端/跨环境确定）。 */
function comparePlain(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function countOccurrences(content: string, keyword: string): number {
  if (!keyword) return 0;
  const lower = content.toLowerCase();
  const needle = keyword.toLowerCase();
  let count = 0;
  let index = lower.indexOf(needle);
  while (index >= 0) {
    count += 1;
    index = lower.indexOf(needle, index + needle.length);
  }
  return count;
}

function windowHasDialogue(content: string, keyword: string, window = 24): boolean {
  const lower = content.toLowerCase();
  const needle = keyword.toLowerCase();
  let index = lower.indexOf(needle);
  while (index >= 0) {
    const from = Math.max(0, index - window);
    const to = Math.min(content.length, index + needle.length + window);
    if (DIALOGUE_MARKS.test(content.slice(from, to))) return true;
    index = lower.indexOf(needle, index + needle.length);
  }
  return false;
}

/** 审计注入文本：continuity 维度 33/34 的本地机检摘要（无命中返回 undefined）。 */
export function renderInfoGapAuditNotes(params: {
  readonly leaks: ReadonlyArray<SecretLeakHit>;
  readonly redundancies: ReadonlyArray<ReaderRedundancyHit>;
  readonly language?: "zh" | "en";
}): string | undefined {
  const language = params.language ?? "zh";
  if (params.leaks.length === 0 && params.redundancies.length === 0) return undefined;
  const lines: string[] = [];
  if (params.leaks.length > 0) {
    lines.push(
      language === "en"
        ? "Secret-leak pre-scan flagged (verify whether the speaker actually knows the secret):"
        : "泄密机检命中（请核实说话人是否知情）：",
    );
    for (const hit of params.leaks) {
      lines.push(
        `- [${hit.gapId}] "${hit.keyword}" ×${hit.occurrences}${hit.inDialogue ? (language === "en" ? " (in dialogue)" : "（对白内）") : ""}${language === "en" ? "" : `；知情人：${hit.knows.join("、") || "无"}`}`,
      );
    }
  }
  if (params.redundancies.length > 0) {
    lines.push(
      language === "en"
        ? "Reader-redundancy pre-scan flagged (reader already knows these):"
        : "废笔机检命中（读者已知，勿复述）：",
    );
    for (const hit of params.redundancies) {
      lines.push(`- [${hit.gapId}] "${hit.keyword}" ×${hit.occurrences}`);
    }
    void language;
  }
  return lines.join("\n");
}
