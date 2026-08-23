/**
 * 侧栏节点树过滤(P3-2):标题 includes 过滤,命中节点的祖先链保留
 * (父命中则子树整棵保留;子命中则父链保留但兄弟剪除)。纯函数。
 */

export interface TreeFilterNode {
  readonly id: string;
  readonly title: string;
  readonly children?: ReadonlyArray<TreeFilterNode>;
}

/** 生成过滤用的视图树(书 → 会话)。 */
export function buildBookTree(books: ReadonlyArray<{ id: string; title: string }>, sessionLabelsByBook: Readonly<Record<string, ReadonlyArray<{ id: string; title: string }>>>): TreeFilterNode[] {
  return books.map((book) => ({
    id: book.id,
    title: book.title,
    children: sessionLabelsByBook[book.id] ?? [],
  }));
}

export function filterTree(nodes: ReadonlyArray<TreeFilterNode>, query: string): TreeFilterNode[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...nodes];
  const walk = (list: ReadonlyArray<TreeFilterNode>): TreeFilterNode[] =>
    list.flatMap((node) => {
      const selfHit = node.title.toLowerCase().includes(q);
      const children = node.children ? walk(node.children) : undefined;
      if (selfHit) return [{ ...node, ...(node.children ? { children: node.children } : {}) }];
      if (children && children.length > 0) return [{ ...node, children }];
      return [];
    });
  return walk(nodes);
}

/** 过滤态下的展开判定:命中会话的书应自动展开(用户不必手点)。 */
export function deriveExpandedBookIds(nodes: ReadonlyArray<TreeFilterNode>, query: string): Set<string> {
  const q = query.trim().toLowerCase();
  if (!q) return new Set();
  const expanded = new Set<string>();
  const walk = (list: ReadonlyArray<TreeFilterNode>) => {
    for (const node of list) {
      if (!node.children) continue;
      const childHit = node.children.some((child) => child.title.toLowerCase().includes(q));
      const selfHit = node.title.toLowerCase().includes(q);
      if (selfHit || childHit) expanded.add(node.id);
      walk(node.children);
    }
  };
  walk(nodes);
  return expanded;
}
