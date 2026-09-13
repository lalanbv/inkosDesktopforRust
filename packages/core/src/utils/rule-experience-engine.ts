import { z } from "zod";

/**
 * R5 反AI规则资产化 + G13 项目经验记忆（365 号契约层，二轮 P1；355 号 §4 R5）。
 *
 * 1. **AntiAiRule**：逐书反AI规则资产（type/severity/enabled）——
 *    - 写作预防：`composeAntiAiGuidance` 注入写法 guidance 同管道（340 号
 *      StyleBinding→composer 链）；
 *    - detect 消费：`scanAntiAiRules` 扫描章节文本产出命中清单；
 *    - fix 消费：`renderAntiAiFixGuidance` 按命中生成修复提示块。
 * 2. **ExperienceEntry**（G13 /learn 语义）：章审查后沉淀「本卷验证有效的
 *    手法」，`renderExperienceGuidance` 注入同管道 guidance。
 *
 * 双端：`engine-rs/src/utils/rule_experience_engine.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/rule-experience-vectors.json`（TS 断言
 * `golden-rule-experience.test.ts`；Rust 差分 `tests/golden_rule_experience_diff.rs`）。
 */

export const AntiAiRuleSchema = z.object({
  /** slug 标识（snake_case，库内唯一）。 */
  id: z
    .string()
    .min(1)
    .max(64)
    .regex(/^[a-z0-9_]+$/, "id must be snake_case slug"),
  /** 规则类型：phrase=措辞 / structure=结构 / rhythm=节奏 / cliche=陈词套语。 */
  type: z.enum(["phrase", "structure", "rhythm", "cliche"]),
  /** 匹配模式（字面文本；isRegex=true 时为正则表达式）。 */
  pattern: z.string().min(1).max(200),
  isRegex: z.boolean().default(false),
  severity: z.enum(["critical", "warning", "info"]).default("warning"),
  /** 告警/修复文案（≤200）。 */
  message: z.string().min(1).max(200),
  /** 修复建议（推荐替换写法，可选）。 */
  replacement: z.string().max(200).optional(),
  enabled: z.boolean().default(true),
});

export type AntiAiRule = z.infer<typeof AntiAiRuleSchema>;
export type AntiAiRuleType = z.infer<typeof AntiAiRuleSchema>["type"];
export type AntiAiSeverity = z.infer<typeof AntiAiRuleSchema>["severity"];

export interface RuleValidateResult {
  readonly rule?: AntiAiRule;
  readonly errors: ReadonlyArray<string>;
}

/** 单条规则校验：schema + isRegex 时正则可编译性检查（双端一致）。 */
export function validateAntiAiRule(raw: unknown): RuleValidateResult {
  const parsed = AntiAiRuleSchema.safeParse(raw);
  if (!parsed.success) {
    return {
      errors: parsed.error.issues.map(
        (issue) => `${issue.path.join(".") || "(root)"}: ${issue.message}`,
      ),
    };
  }
  const rule = parsed.data;
  if (rule.isRegex) {
    try {
      // eslint-disable-next-line no-new
      new RegExp(rule.pattern);
    } catch (error) {
      return { errors: [`pattern: invalid regex — ${String(error)}`] };
    }
  }
  return { rule, errors: [] };
}

export interface AntiAiHit {
  readonly ruleId: string;
  readonly severity: AntiAiRule["severity"];
  readonly message: string;
  /** 首个命中窗口（命中 ±20 字），供界面定位。 */
  readonly excerpt: string;
  /** 命中次数。 */
  readonly count: number;
  /** 修复建议（规则携带时透出）。 */
  readonly replacement?: string;
}

const EXCERPT_WINDOW = 20;

/** detect 消费：扫描文本（仅 enabled 规则；字面计数 / regex 全匹配）。 */
export function scanAntiAiRules(
  text: string,
  rules: ReadonlyArray<AntiAiRule>,
): AntiAiHit[] {
  const hits: AntiAiHit[] = [];
  for (const rule of rules) {
    if (!rule.enabled) continue;
    if (rule.isRegex) {
      let regex: RegExp;
      try {
        regex = new RegExp(rule.pattern, "g");
      } catch {
        continue; // 编译失败静默跳过（校验期已报）。
      }
      const matches = [...text.matchAll(regex)];
      if (matches.length === 0) continue;
      const first = matches[0]!;
      const start = Math.max(0, (first.index ?? 0) - EXCERPT_WINDOW);
      const end = Math.min(text.length, (first.index ?? 0) + first[0].length + EXCERPT_WINDOW);
      hits.push({
        ruleId: rule.id,
        severity: rule.severity,
        message: rule.message,
        excerpt: text.slice(start, end),
        count: matches.length,
        ...(rule.replacement ? { replacement: rule.replacement } : {}),
      });
    } else {
      const pattern = rule.pattern;
      let count = 0;
      let firstIndex = -1;
      let from = 0;
      while (from <= text.length) {
        const index = text.indexOf(pattern, from);
        if (index < 0) break;
        count += 1;
        if (firstIndex < 0) firstIndex = index;
        from = index + Math.max(1, pattern.length);
      }
      if (count === 0) continue;
      const start = Math.max(0, firstIndex - EXCERPT_WINDOW);
      const end = Math.min(text.length, firstIndex + pattern.length + EXCERPT_WINDOW);
      hits.push({
        ruleId: rule.id,
        severity: rule.severity,
        message: rule.message,
        excerpt: text.slice(start, end),
        count,
        ...(rule.replacement ? { replacement: rule.replacement } : {}),
      });
    }
  }
  const severityRank: Record<AntiAiRule["severity"], number> = { critical: 0, warning: 1, info: 2 };
  return hits.sort(
    (a, b) =>
      severityRank[a.severity] - severityRank[b.severity]
      || b.count - a.count
      || (a.ruleId < b.ruleId ? -1 : a.ruleId > b.ruleId ? 1 : 0),
  );
}

/** fix 消费：命中 → 修复提示块（critical 前，含 replacement 建议；无命中 undefined）。 */
export function renderAntiAiFixGuidance(
  hits: ReadonlyArray<AntiAiHit>,
  language: "zh" | "en" = "zh",
): string | undefined {
  if (hits.length === 0) return undefined;
  const isEn = language === "en";
  const lines = hits.map(
    (hit) => `- [${hit.severity}] ${hit.message}（×${hit.count}）`,
  );
  const replacementLines = hits
    .filter((hit) => hit.replacement)
    .map((hit) => `- ${hit.ruleId} → ${hit.replacement}`);
  const header = isEn
    ? "## Anti-AI violations (fix before delivery)"
    : "## 反AI规则命中（交付前修复）";
  const parts = [header, ...lines];
  if (replacementLines.length > 0) {
    parts.push(isEn ? "Suggested rewrites:" : "建议改法：", ...replacementLines);
  }
  return parts.join("\n");
}

/** 写作预防：enabled 规则 → 写法 guidance 同管道禁则块（无 enabled 规则 undefined）。 */
export function composeAntiAiGuidance(
  rules: ReadonlyArray<AntiAiRule>,
  language: "zh" | "en" = "zh",
  maxChars?: number,
): string | undefined {
  const enabled = rules.filter((rule) => rule.enabled);
  if (enabled.length === 0) return undefined;
  const isEn = language === "en";
  const header = isEn
    ? "## Anti-AI rules (bound — never violate while writing)"
    : "## 反AI规则（绑定——写作时严禁违反）";
  const lines = enabled.map((rule) => {
    const replacement = rule.replacement
      ? (isEn ? ` (prefer: ${rule.replacement})` : `（改为：${rule.replacement}）`)
      : "";
    return `- [${rule.severity}] ${rule.message}${replacement}`;
  });
  const text = [header, ...lines].join("\n");
  if (maxChars !== undefined && text.length > maxChars) {
    return `${text.slice(0, Math.max(0, maxChars - 1))}…`;
  }
  return text;
}

// ── G13：项目经验记忆（/learn 语义）──

export const ExperienceEntrySchema = z.object({
  id: z
    .string()
    .min(1)
    .max(64)
    .regex(/^[a-z0-9_]+$/, "id must be snake_case slug"),
  /** 沉淀来源章（0=书级通用经验）。 */
  chapter: z.number().int().min(0),
  kind: z.enum(["technique", "hook", "pacing", "dialogue"]),
  /** 经验文本（「本卷验证有效的手法」，≤200）。 */
  text: z.string().min(1).max(200),
  enabled: z.boolean().default(true),
  createdAt: z.string().min(1),
});

export type ExperienceEntry = z.infer<typeof ExperienceEntrySchema>;
export type ExperienceKind = z.infer<typeof ExperienceEntrySchema>["kind"];

/** 经验合并：同 text（trim 后）去重保序——已有条目保留，新条目追加。 */
export function mergeExperienceEntries(
  existing: ReadonlyArray<ExperienceEntry>,
  incoming: ReadonlyArray<ExperienceEntry>,
): ReadonlyArray<ExperienceEntry> {
  const seen = new Set(existing.map((entry) => entry.text.trim()));
  const merged = [...existing];
  for (const entry of incoming) {
    const key = entry.text.trim();
    if (seen.has(key)) continue;
    seen.add(key);
    merged.push(entry);
  }
  return merged;
}

/** G13 注入：enabled 经验 → guidance 块（≤10 条，章号升序稳定；无 enabled undefined）。 */
export function renderExperienceGuidance(
  entries: ReadonlyArray<ExperienceEntry>,
  language: "zh" | "en" = "zh",
  maxChars?: number,
): string | undefined {
  const enabled = entries
    .filter((entry) => entry.enabled)
    .sort((a, b) => a.chapter - b.chapter || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
    .slice(0, 10);
  if (enabled.length === 0) return undefined;
  const isEn = language === "en";
  const header = isEn
    ? "## Proven techniques (learned from this book — reuse)"
    : "## 本书验证有效的手法（经验记忆——复用）";
  const lines = enabled.map((entry) => `- ${entry.text}`);
  const text = [header, ...lines].join("\n");
  if (maxChars !== undefined && text.length > maxChars) {
    return `${text.slice(0, Math.max(0, maxChars - 1))}…`;
  }
  return text;
}

// ── R22/392 号：反AI规则内容指纹 + 内置种子包（四轮 P1；363 号种子兜底先例）──

/** 规则内容指纹的 canonical 形式（双端逐字一致；与 362 assetCanonicalForm 同编码约定）。 */
export function antiAiRuleCanonicalForm(rule: AntiAiRule): string {
  return [
    rule.id,
    rule.type,
    rule.pattern,
    rule.isRegex ? "1" : "0",
    rule.severity,
    rule.message,
    rule.replacement ?? "",
    rule.enabled ? "1" : "0",
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

/** 规则内容指纹（同内容必同指纹——种子幂等合并依据，对齐 362/360 hash 语义）。 */
export function antiAiRuleContentHash(rule: AntiAiRule): string {
  return fnv1aHex16(antiAiRuleCanonicalForm(rule));
}

/** 规则组指纹：逐条 canonical 以 \n 连接后的 FNV 指纹（种子包完整性锚点）。 */
export function antiAiRulePackHash(rules: ReadonlyArray<AntiAiRule>): string {
  return fnv1aHex16(rules.map((rule) => antiAiRuleCanonicalForm(rule)).join("\n"));
}

/**
 * 内置反AI规则种子包（392 号，双端逐字镜像；随包分发的冷启动底线防线）。
 * 取精弃糟：只收 9 条最高频、可机检、有明确改法的 AI 腔（对标「模板数量
 * 战」的糟粕刻意做小）；覆盖全部四类型（phrase/structure/rhythm/cliche）；
 * 全部字面匹配（isRegex=false）——正则种子留用户自建，误伤面不可控。
 */
export const ANTI_AI_RULE_SEEDS: ReadonlyArray<AntiAiRule> = [
  {
    id: "eye_glint_stock",
    type: "phrase",
    pattern: "眼中闪过一丝",
    isRegex: false,
    severity: "critical",
    message: "「眼中闪过一丝X」是最高频的 AI 神态套语——抽象神态报告没有画面。",
    replacement: "改写成可看见的动作：捏紧的指节、挪开的视线、喉结滚动。",
    enabled: true,
  },
  {
    id: "essay_connective",
    type: "structure",
    pattern: "值得注意的是",
    isRegex: false,
    severity: "critical",
    message: "论文式连接词，AI 论述腔直接漏进叙事——删除，让事实自己说话。",
    enabled: true,
  },
  {
    id: "mouth_smirk_stock",
    type: "phrase",
    pattern: "嘴角勾起一抹",
    isRegex: false,
    severity: "warning",
    message: "「嘴角勾起一抹X」是 AI 高频表情模板——肌肉报告不等于情绪。",
    replacement: "写对面的人看见了什么、感到了什么。",
    enabled: true,
  },
  {
    id: "air_freeze_cliche",
    type: "cliche",
    pattern: "空气仿佛凝固",
    isRegex: false,
    severity: "warning",
    message: "张力陈词——气氛凝固是报告不是体验。",
    replacement: "让在场者的某个小动作失灵：端着的杯子停在半空。",
    enabled: true,
  },
  {
    id: "deep_breath_buffer",
    type: "rhythm",
    pattern: "深吸一口气",
    isRegex: false,
    severity: "warning",
    message: "高频缓冲动作、节奏拖延——每章至多一次，情绪转折处用更具体的身体反应。",
    enabled: true,
  },
  {
    id: "discourse_hedge",
    type: "structure",
    pattern: "从某种意义上",
    isRegex: false,
    severity: "warning",
    message: "对冲式论断削弱叙事权威——角色可以有立场，叙事者不骑墙。",
    replacement: "改成明确判断，或删掉整个从句。",
    enabled: true,
  },
  {
    id: "pupil_contraction",
    type: "phrase",
    pattern: "瞳孔骤缩",
    isRegex: false,
    severity: "warning",
    message: "高频惊吓神态模板——惊惧写失控的后果：后退半步、手里的东西掉了。",
    enabled: true,
  },
  {
    id: "vague_time_transition",
    type: "rhythm",
    pattern: "不知过了多久",
    isRegex: false,
    severity: "info",
    message: "时间过渡含糊——用环境或动作标记流逝：灯换了颜色、茶凉透了。",
    enabled: true,
  },
  {
    id: "as_if_whisper_cliche",
    type: "cliche",
    pattern: "仿佛在诉说",
    isRegex: false,
    severity: "info",
    message: "「仿佛在诉说X」把情绪结论塞给读者——删掉，写让人产生此感的画面。",
    enabled: true,
  },
];

export interface AntiAiRulesWithSeeds {
  readonly rules: ReadonlyArray<AntiAiRule>;
  /** true = 规则来自内置种子（文件缺失/损坏的内存态兜底，不自动落盘）。 */
  readonly seeded: boolean;
}

/**
 * 种子兜底解析（363 号先例）：文件缺失/损坏（undefined）→ 内置种子
 * （seeded=true，内存态不落盘）；**文件存在但规则为空 = 用户显式清空的
 * 选择，不回填种子**（否则用户永远无法关掉防线）。
 */
export function resolveAntiAiRulesWithSeeds(
  parsed: ReadonlyArray<AntiAiRule> | undefined,
): AntiAiRulesWithSeeds {
  if (parsed === undefined) {
    return { rules: ANTI_AI_RULE_SEEDS, seeded: true };
  }
  return { rules: parsed, seeded: false };
}
