/**
 * 964 号：风格指纹（统计面，无 LLM）——写法档案池 `POST /style-profiles {name, text}`
 * 的服务端计算内核。
 *
 * 逐字恢复自上游 2.0 迁移（7aed0d82）前的 `packages/core/src/agents/style-analyzer.ts`
 * `analyzeStyle`：2.0 把文风分析换成 LLM 指南（compileStyleGuide）并删除了统计分析器，
 * 但 340 号写法档案池（StyleProfile 全文 → 特征池 → 绑定注入）仍以该指纹为数据源。
 * Rust 对偶 `engine-rs/src/agents/style_analyzer.rs::analyze_style`；双端断言同一
 * golden 向量表 `__tests__/golden/style-fingerprint-vectors.json`。
 *
 * 与原版三处工程差异：`analyzedAt` 可注入（golden 确定性）；段落长度最值改循环（原版
 * `Math.min(...arr)` 展开在超大样本上栈溢出）；词汇多样性引号剥离取 ASCII 与弯引号
 * 并集（见下方注释）。
 */

export interface StyleFingerprint {
  readonly avgSentenceLength: number;
  readonly sentenceLengthStdDev: number;
  readonly avgParagraphLength: number;
  readonly paragraphLengthRange: { readonly min: number; readonly max: number };
  readonly vocabularyDiversity: number;
  readonly topPatterns: ReadonlyArray<string>;
  readonly rhetoricalFeatures: ReadonlyArray<string>;
  readonly sourceName?: string;
  readonly analyzedAt?: string;
}

// Common rhetorical patterns in Chinese fiction
const RHETORICAL_PATTERNS: ReadonlyArray<{ readonly name: string; readonly regex: RegExp }> = [
  { name: "比喻(像/如/仿佛)", regex: /[像如仿佛似](?:是|同|一般|一样)/g },
  { name: "排比", regex: /[，。；]([^，。；]{2,6})[，。；]\1/g },
  { name: "反问", regex: /难道|怎么可能|岂不是|何尝不/g },
  { name: "夸张", regex: /天崩地裂|惊天动地|翻天覆地|震耳欲聋/g },
  { name: "拟人", regex: /[风雨雪月花树草石](?:在|像|仿佛).*?(?:笑|哭|叹|呻|吟|怒|舞)/g },
  { name: "短句节奏", regex: /[。！？][^。！？]{1,8}[。！？]/g },
];

// Common rhetorical patterns in English fiction
const EN_RHETORICAL_PATTERNS: ReadonlyArray<{ readonly name: string; readonly regex: RegExp }> = [
  { name: "simile (like/as if)", regex: /\b(?:like a|like an|as if|as though)\b/gi },
  { name: "rhetorical question", regex: /\b(?:how could|why would|what if|wasn't it|isn't it|could it be)\b[^.!?]*\?/gi },
  { name: "tricolon", regex: /\b\w+,\s+\w+,\s+and\s+\w+\b/gi },
  { name: "short punchy rhythm", regex: /[.!?]\s+[A-Z][^.!?]{1,24}[.!?]/g },
];

export function analyzeStyleFingerprint(
  text: string,
  sourceName?: string,
  language: "zh" | "en" = "zh",
  analyzedAt: string = new Date().toISOString(),
): StyleFingerprint {
  const isEn = language === "en";

  const sentences = text
    .split(isEn ? /[.!?\n]+/ : /[。！？\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);

  const paragraphs = text
    .split(/\n\s*\n/)
    .map((p) => p.trim())
    .filter((p) => p.length > 0);

  // Measure length in the language's native unit: words for English, characters for Chinese.
  const measure = (s: string): number =>
    isEn ? (s.match(/[A-Za-z0-9]+(?:'[A-Za-z0-9]+)?/g)?.length ?? 0) : s.replace(/\s+/g, "").length;

  const sentenceLengths = sentences.map(measure);
  const avgSentenceLength = sentenceLengths.length > 0
    ? sentenceLengths.reduce((a, b) => a + b, 0) / sentenceLengths.length
    : 0;
  const sentenceLengthStdDev = sentenceLengths.length > 1
    ? Math.sqrt(
        sentenceLengths.reduce((sum, l) => sum + (l - avgSentenceLength) ** 2, 0) /
          sentenceLengths.length,
      )
    : 0;

  const paragraphLengths = paragraphs.map(measure);
  const avgParagraphLength = paragraphLengths.length > 0
    ? paragraphLengths.reduce((a, b) => a + b, 0) / paragraphLengths.length
    : 0;
  let minParagraph = 0;
  let maxParagraph = 0;
  for (let index = 0; index < paragraphLengths.length; index += 1) {
    const length = paragraphLengths[index]!;
    if (index === 0 || length < minParagraph) minParagraph = length;
    if (index === 0 || length > maxParagraph) maxParagraph = length;
  }

  // Vocabulary diversity (TTR — Type-Token Ratio): word-level for English, character-level for Chinese.
  let vocabularyDiversity: number;
  if (isEn) {
    const words = text.toLowerCase().match(/[a-z0-9]+(?:'[a-z0-9]+)?/g) ?? [];
    vocabularyDiversity = words.length > 0 ? new Set(words).size / words.length : 0;
  } else {
    // 964 号：引号剥离取 ASCII 与弯引号并集——原版字面仅 ASCII `"'`（Rust 移植取
    // 弯引号 “”‘’，双端早已分叉）；中文对白惯用弯引号，并集才符合「剥离标点」原意。
    const chars = text.replace(/[\s\n\r，。！？、：；"'“”‘’（）【】《》\d]/g, "");
    vocabularyDiversity = chars.length > 0 ? new Set(chars).size / chars.length : 0;
  }

  // Top sentence opening patterns: first word for English, first 2 chars for Chinese.
  const openingCounts: Record<string, number> = {};
  for (const s of sentences) {
    const key = isEn
      ? (s.match(/[A-Za-z']+/)?.[0]?.toLowerCase() ?? "")
      : (s.length >= 2 ? s.slice(0, 2) : "");
    if (key) openingCounts[key] = (openingCounts[key] ?? 0) + 1;
  }
  const topPatterns = Object.entries(openingCounts)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 5)
    .filter(([, count]) => count >= 3)
    .map(([pattern, count]) => (isEn ? `${pattern}… (${count})` : `${pattern}...(${count}次)`));

  const rhetoricalPatterns = isEn ? EN_RHETORICAL_PATTERNS : RHETORICAL_PATTERNS;
  const rhetoricalFeatures: string[] = [];
  for (const { name, regex } of rhetoricalPatterns) {
    const matches = text.match(regex);
    if (matches && matches.length >= 2) {
      rhetoricalFeatures.push(isEn ? `${name} (${matches.length})` : `${name}(${matches.length}处)`);
    }
  }

  return {
    avgSentenceLength: Math.round(avgSentenceLength * 10) / 10,
    sentenceLengthStdDev: Math.round(sentenceLengthStdDev * 10) / 10,
    avgParagraphLength: Math.round(avgParagraphLength),
    paragraphLengthRange: { min: minParagraph, max: maxParagraph },
    vocabularyDiversity: Math.round(vocabularyDiversity * 1000) / 1000,
    topPatterns,
    rhetoricalFeatures,
    sourceName,
    analyzedAt,
  };
}
