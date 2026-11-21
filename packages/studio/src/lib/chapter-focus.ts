/**
 * R47/1640 号：章阅读器一次性焦点传递（⌘P 正文命中 → 跳章并滚动到命中行）。
 *
 * 备案：HashRoute 扩展（focusQuery 进序列化面）成本高（全消费点）且深链
 * 场景不存在（⌘P 为会话内交互），故走模块级一次性 nonce 传递；刷新后焦点
 * 丢失是有意行为（无深链语义）。
 */

interface PendingChapterFocus {
  readonly bookId: string;
  readonly chapterNumber: number;
  readonly query: string;
  readonly nonce: number;
}

let pendingFocus: PendingChapterFocus | null = null;
let nonceCounter = 0;

export function setChapterFocus(bookId: string, chapterNumber: number, query: string): void {
  nonceCounter += 1;
  pendingFocus = { bookId, chapterNumber, query, nonce: nonceCounter };
}

/** 消费一次性焦点：仅当书+章匹配时返回查询词（取走即清，防重放）。 */
export function takeChapterFocus(bookId: string, chapterNumber: number): string | null {
  const focus = pendingFocus;
  pendingFocus = null;
  if (focus && focus.bookId === bookId && focus.chapterNumber === chapterNumber) {
    return focus.query;
  }
  return null;
}
