import type { HashRoute } from "@/hooks/use-hash-route";

/**
 * 命令面板「最近访问」的一条记录：路由的扁平化快照 + 展示文案。
 * `page` 取 `HashRoute["page"]`；参数字段按需携带，用于重建路由。
 */
export interface RecentEntry {
  page: HashRoute["page"];
  /** 已本地化的展示文案（面包屑末段，如书名 /「第 3 章」）。 */
  label: string;
  bookId?: string;
  chapterNumber?: number;
  serviceId?: string;
  projectId?: string;
  tab?: string;
}

export interface RecentsStore {
  recents: ReadonlyArray<RecentEntry>;
  pushRecent: (entry: RecentEntry) => void;
  clearRecents: () => void;
}
