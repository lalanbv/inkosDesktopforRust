/**
 * R17 LLM-as-judge 评分维度（385 号，三轮 P2；v3 §3 R17）。
 *
 * 独立评审模型对生成正文的四维打分（0–10，一位小数），供评测集离线
 * 回放（mock judge）与真实评审（配置 LLM 后）共用同一契约。
 * 解析容错：围栏剥离 + 首个 JSON 对象；越界 clamp 0–10；缺失维度 undefined。
 */

export interface JudgeVerdict {
  readonly coherence: number;
  readonly tension: number;
  readonly style: number;
  readonly consistency: number;
  /** 一句话总评（≤200 码元）。 */
  readonly verdict: string;
}

export const JUDGE_DIMENSIONS = [
  "coherence",
  "tension",
  "style",
  "consistency",
] as const;

export type JudgeDimension = (typeof JUDGE_DIMENSIONS)[number];

const clamp10 = (value: number): number => Math.min(10, Math.max(0, Math.round(value * 10) / 10));

/** 解析评审产出：越界 clamp、缺失维度 undefined 字段剔除；无有效维度 → undefined。 */
export function parseJudgeVerdict(content: string): JudgeVerdict | undefined {
  const fenced = content.match(/```(?:json)?\s*([\s\S]*?)\s*```/i);
  const raw = (fenced?.[1] ?? content).trim();
  const match = /\{[\s\S]*\}/.exec(raw);
  if (!match) return undefined;
  let parsed: Record<string, unknown>;
  try {
    parsed = JSON.parse(match[0]) as Record<string, unknown>;
  } catch {
    return undefined;
  }
  const verdictText = typeof parsed.verdict === "string" ? parsed.verdict : "";
  const dimensions: Partial<Record<JudgeDimension, number>> = {};
  let any = false;
  for (const dimension of JUDGE_DIMENSIONS) {
    const value = parsed[dimension];
    if (typeof value === "number" && Number.isFinite(value)) {
      dimensions[dimension] = clamp10(value);
      any = true;
    }
  }
  if (!any && !verdictText) return undefined;
  return {
    ...(dimensions.coherence !== undefined ? { coherence: dimensions.coherence } : {}),
    ...(dimensions.tension !== undefined ? { tension: dimensions.tension } : {}),
    ...(dimensions.style !== undefined ? { style: dimensions.style } : {}),
    ...(dimensions.consistency !== undefined ? { consistency: dimensions.consistency } : {}),
    verdict: clamp(verdictText, 200),
  } as JudgeVerdict;
}

function clamp(value: string, max: number): string {
  return value.length > max ? value.slice(0, max) : value;
}

/** 评审均值（一位小数；无维度 undefined）。 */
export function judgeAverage(verdict: JudgeVerdict): number | undefined {
  const values = JUDGE_DIMENSIONS.map((dimension) => verdict[dimension]).filter(
    (value): value is number => typeof value === "number",
  );
  if (values.length === 0) return undefined;
  return Math.round((values.reduce((sum, value) => sum + value, 0) / values.length) * 10) / 10;
}

/** 评审提示词（zh/en 双语，评测/真实评审共用）。 */
export function buildJudgePrompt(
  chapterContent: string,
  language: "zh" | "en" = "zh",
): string {
  const isEn = language === "en";
  if (isEn) {
    return `You are an independent fiction reviewer. Score the chapter on four dimensions (0-10, one decimal): coherence, tension, style, consistency. Output JSON: {"coherence":7.5,"tension":6,"style":8,"consistency":9,"verdict":"one-sentence review"}\n\n---\n${chapterContent}`;
  }
  return `你是独立的小说评审。对章节按四个维度打分（0–10，一位小数）：coherence（连贯）、tension（张力）、style（文笔）、consistency（一致性）。输出 JSON：{"coherence":7.5,"tension":6,"style":8,"consistency":9,"verdict":"一句话总评"}\n\n---\n${chapterContent}`;
}
