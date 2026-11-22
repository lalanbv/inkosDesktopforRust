/**
 * R52/1656 号：编辑器实体命中集扫描纯函数（Rust 镜像 =
 * engine-rs/src/utils/entity_highlight.rs，golden entity-highlight-vectors.json
 * 锁定；码点下标口径=golden_search_highlight 家族同款）。
 *
 * Codex tracking 语义（v8 深化三 R52 定义）：**大小写敏感**（indexOf 无
 * flag；中文无大小写恒命中）+ **排除短语条目级**（exclude 名单精确匹配
 * 术语则该术语不扫）+ 重叠**最长优先**（别名「林动大哥」压过名「林动」；
 * 同长同位先注册卡先得）。minLength 缺省 2（单字术语噪声过滤）。
 */

export interface EntityHighlightHit {
  /** 命中术语（实体名或别名原文）。 */
  readonly entity: string;
  /** 悬停卡索引（cards 下标）。 */
  readonly cardIndex: number;
  /** 码点下标（exclusive end）。 */
  readonly start: number;
  readonly end: number;
}

export interface EntityCardLike {
  readonly name: string;
  readonly aliases?: ReadonlyArray<string>;
}

/** 命中集扫描：cards 的 name+aliases 在 text 中全部出现位置（去重叠后按
 * start 升序）。O(术语数 × 文本长)——调用端配合防抖（编辑态重扫）与
 * 大章禁用双闸（组件层职责）。 */
export function scanEntityHighlights(
  text: string,
  cards: ReadonlyArray<EntityCardLike>,
  options?: {
    /** 术语最小码元长度（缺省 2）。 */
    readonly minLength?: number;
    /** 排除短语（条目级——命中术语精确匹配则不扫）。 */
    readonly exclude?: ReadonlyArray<string>;
  },
): ReadonlyArray<EntityHighlightHit> {
  const minLength = options?.minLength ?? 2;
  const exclude = new Set(options?.exclude ?? []);
  const codepoints = Array.from(text);
  // 术语表（去重：同术语首卡注册；长度/排除过滤）。
  const terms: Array<{ term: ReadonlyArray<string>; cardIndex: number }> = [];
  const seen = new Set<string>();
  cards.forEach((card, cardIndex) => {
    for (const name of [card.name, ...(card.aliases ?? [])]) {
      if (seen.has(name)) continue;
      if (name.length < minLength) continue;
      if (exclude.has(name)) continue;
      seen.add(name);
      terms.push({ term: Array.from(name), cardIndex });
    }
  });
  // 全部出现位置（朴素扫描——first 码元预判快路径）。
  const spans: Array<{ start: number; end: number; cardIndex: number }> = [];
  for (const { term, cardIndex } of terms) {
    const first = term[0];
    if (first === undefined) continue;
    for (let i = 0; i + term.length <= codepoints.length; i += 1) {
      if (codepoints[i] !== first) continue;
      let matched = true;
      for (let j = 1; j < term.length; j += 1) {
        if (codepoints[i + j] !== term[j]) {
          matched = false;
          break;
        }
      }
      if (matched) spans.push({ start: i, end: i + term.length, cardIndex });
    }
  }
  // 贪心去重叠：start 升序、长度降序、卡序稳定。
  spans.sort(
    (a, b) =>
      a.start - b.start ||
      (b.end - b.start) - (a.end - a.start) ||
      a.cardIndex - b.cardIndex,
  );
  const hits: EntityHighlightHit[] = [];
  let cursor = 0;
  for (const span of spans) {
    if (span.start >= cursor) {
      hits.push({
        entity: codepoints.slice(span.start, span.end).join(""),
        cardIndex: span.cardIndex,
        start: span.start,
        end: span.end,
      });
      cursor = span.end;
    }
  }
  return hits;
}
