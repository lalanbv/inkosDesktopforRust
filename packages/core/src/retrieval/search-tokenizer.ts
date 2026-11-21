/**
 * 搜索分词纯函数单源（R47/1640 号从 local-search.ts 原样抽出）。
 *
 * 抽出动机：retrieval/global-search.ts 的前端消费面（⌘P 命中档与章阅读器
 * 焦点，vite 生产 bundle）不能拖入 local-search 的 node:sqlite 依赖——本模块
 * 仅用浏览器安全面（Intl.Segmenter / String.normalize / 正则）；local-search
 * （node:sqlite FTS5 索引内核）反向 import 并 re-export 保持 API 单源。
 *
 * Rust 镜像 = engine-rs/src/utils/local_search.rs（tokenize_search_text /
 * build_match_query / split_markdown_for_search 三函数，golden
 * search-tokenizer-vectors 锁双端 token 序列）。
 */

export interface SearchDocument {
  readonly id: string;
  readonly scope: string;
  readonly kind: string;
  readonly source: string;
  readonly title: string;
  readonly body: string;
  readonly metadata?: Readonly<Record<string, unknown>>;
}

export interface SearchHit extends SearchDocument {
  readonly score: number;
}

export interface MarkdownSearchSegment {
  readonly heading: string;
  readonly body: string;
  readonly charStart: number;
  readonly charEnd: number;
}

export function splitMarkdownForSearch(markdown: string): MarkdownSearchSegment[] {
  const segments: MarkdownSearchSegment[] = [];
  let heading = "";
  const blockPattern = /(?:^|\n)([^\n](?:[\s\S]*?))(?=\n\s*\n|$)/g;
  for (const match of markdown.matchAll(blockPattern)) {
    const raw = match[1] ?? "";
    const leadingOffset = (match[0]?.length ?? raw.length) - raw.length;
    const charStart = (match.index ?? 0) + leadingOffset;
    const body = raw.trim();
    if (!body) continue;
    const headingMatch = body.match(/^#{1,6}\s+(.+)$/);
    if (headingMatch) {
      heading = headingMatch[1]?.trim() ?? heading;
      continue;
    }
    segments.push({
      heading,
      body,
      charStart,
      charEnd: charStart + raw.length,
    });
  }
  return segments;
}

export function tokenizeSearchText(text: string): string[] {
  const normalized = text.normalize("NFKC").toLocaleLowerCase();
  const segmenter = new Intl.Segmenter(undefined, { granularity: "word" });
  const tokens: string[] = [];
  for (const part of segmenter.segment(normalized)) {
    const token = part.segment.trim();
    if (!part.isWordLike || !token) continue;
    if (/^[\p{Script=Latin}\p{N}_-]+$/u.test(token) && token.length < 2) continue;
    tokens.push(token);
  }
  // ICU can segment short Chinese compounds into individual Han characters.
  // Adjacent Han bigrams preserve those compounds for lexical retrieval; this
  // is tokenizer mechanics, not a semantic intent rule.
  const segmentedTokenCount = tokens.length;
  for (let index = 0; index < segmentedTokenCount - 1; index += 1) {
    const left = tokens[index] ?? "";
    const right = tokens[index + 1] ?? "";
    if (/^\p{Script=Han}$/u.test(left) && /^\p{Script=Han}$/u.test(right)) {
      tokens.push(`${left}${right}`);
    }
  }
  for (const match of normalized.matchAll(/[\p{L}\p{N}]+(?:[-_][\p{L}\p{N}]+)+/gu)) {
    tokens.push(match[0]);
  }
  return tokens;
}

export function buildMatchQuery(query: string): string | null {
  const tokens = [...new Set(tokenizeSearchText(query))];
  if (tokens.length === 0) return null;
  return tokens.map((token) => `"${token.replaceAll('"', '""')}"`).join(" OR ");
}
