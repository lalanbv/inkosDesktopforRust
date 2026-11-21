/**
 * 全局搜索纯函数核（R47，1640 号）。
 *
 * Rust 镜像 = engine-rs/src/utils/global_search.rs：查询分词复用
 * tokenizeSearchText（分词单源，golden search-tokenizer-vectors 已锁双端），
 * 命中定位/高亮裁剪/加权字段扫描由 golden search-highlight-vectors 锁双端，
 * 排序候选由 search-ranking-vectors 锁双端。
 *
 * 偏移口径备案：匹配在 toLowerCase() 投影串上定位，再按码点下标映射回原串
 * 取窗（与 Rust chars().count() 同口径，UTF-16 代理对对/emoji 不错位）；
 * 极小语种（如 U+0130）小写化变长时 best-effort，不影响命中本身。
 */

import { tokenizeSearchText } from "./search-tokenizer.js";

/** 高亮片段前后窗（码点数）。对齐 Rust SEARCH_CONTEXT_BEFORE/AFTER。 */
export const SEARCH_CONTEXT_BEFORE = 48;
export const SEARCH_CONTEXT_AFTER = 96;
/** 省略号标记（片段被裁剪时加在首/尾）。 */
export const SNIPPET_ELLIPSIS = "…";

export interface HighlightSnippet {
  readonly text: string;
  readonly matchStart: number;
  readonly matchLength: number;
}

export interface WeightedFieldMatch {
  readonly fieldIndex: number;
  readonly weight: number;
  readonly charPos: number;
  readonly charLength: number;
}

const codePointLength = (value: string): number => Array.from(value).length;

/**
 * 查询匹配 token 集：分词单源 + 去重保序 + 原始/紧缩短语候选。
 *
 * 分词单源只产 bigram（「碎镜之主」→ 碎镜/镜之/之主），整短语高亮依赖
 * 紧缩候选（trim+lowercase、去内部空白）。排序 = 码点长度降序、同长按
 * 首现序（与 Rust 同一比较器；sort 稳定，候选 raw 先于 condensed）。
 */
export function queryTokens(query: string): string[] {
  const seen = new Set<string>();
  const tokens: Array<{ token: string; index: number }> = [];
  tokenizeSearchText(query).forEach((token, index) => {
    if (!seen.has(token)) {
      seen.add(token);
      tokens.push({ token, index });
    }
  });
  const raw = query.trim().toLowerCase();
  const condensed = raw.replace(/[\t\n\r ]/g, "");
  for (const candidate of [raw, condensed]) {
    if (candidate && !seen.has(candidate)) {
      seen.add(candidate);
      tokens.push({ token: candidate, index: Number.MAX_SAFE_INTEGER });
    }
  }
  return tokens
    .slice()
    .sort((a, b) => codePointLength(b.token) - codePointLength(a.token) || a.index - b.index)
    .map((entry) => entry.token);
}

/** 最早命中定位（同位取最长候选）：返回原串上的 (码点下标, 码点长度)。 */
export function findQueryMatch(
  content: string,
  tokens: ReadonlyArray<string>,
): [number, number] | null {
  if (tokens.length === 0 || content.length === 0) return null;
  const haystack = content.toLowerCase();
  let best: [number, number] | null = null;
  for (const token of tokens) {
    const index = haystack.indexOf(token);
    if (index < 0) continue;
    const charPos = Array.from(haystack.slice(0, index)).length;
    const charLength = codePointLength(token);
    if (!best || charPos < best[0] || (charPos === best[0] && charLength > best[1])) {
      best = [charPos, charLength];
    }
  }
  return best;
}

/**
 * 高亮片段：最早命中居中裁剪（前 48 后 96 码点），被裁侧加省略号；
 * matchStart/Length 为片段内码点坐标（含省略号前缀偏移）。无命中 → null。
 */
export function buildHighlightSnippet(content: string, query: string): HighlightSnippet | null {
  const match = findQueryMatch(content, queryTokens(query));
  if (!match) return null;
  const [charPos, charLength] = match;
  const chars = Array.from(content);
  const total = chars.length;
  const start = Math.max(0, charPos - SEARCH_CONTEXT_BEFORE);
  const end = Math.min(charPos + charLength + SEARCH_CONTEXT_AFTER, total);
  const prefix = start > 0 ? SNIPPET_ELLIPSIS : "";
  const suffix = end < total ? SNIPPET_ELLIPSIS : "";
  const text = `${prefix}${chars.slice(start, end).join("")}${suffix}`;
  return {
    text,
    matchStart: Array.from(prefix).length + (charPos - start),
    matchLength: charLength,
  };
}

/** 加权字段扫描（名册/账本轴）：按给定顺序取首个命中字段。 */
export function findWeightedFieldMatch(
  fields: ReadonlyArray<readonly [string, number]>,
  query: string,
): WeightedFieldMatch | null {
  const tokens = queryTokens(query);
  if (tokens.length === 0) return null;
  for (let index = 0; index < fields.length; index += 1) {
    const [text, weight] = fields[index]!;
    const match = findQueryMatch(text, tokens);
    if (match) {
      return { fieldIndex: index, weight, charPos: match[0], charLength: match[1] };
    }
  }
  return null;
}
