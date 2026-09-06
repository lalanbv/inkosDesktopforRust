export interface WithSeries {
  readonly series?: { readonly name: string; readonly order: number };
}

export interface BookSeriesGroup<T extends WithSeries = WithSeries> {
  /** 稳定 key：系列名（组间唯一）；散书组固定 "standalone"。 */
  readonly key: string;
  /** 系列名；散书组为空串（不渲染组头）。 */
  readonly name: string;
  /** 系列组按卷序升序；散书保持原始顺序。 */
  readonly books: ReadonlyArray<T>;
}

/**
 * Dashboard 书卡按系列分组（180 号 C3-a）。
 * - 有 series.name 的聚为一组，组内按 series.order 升序；
 * - 无 series 的归入末尾散书组（不渲染组头）；
 * - 系列组之间按组内最小 order 排序（同名异常重名按名字符合并）。
 */
export function groupBooksBySeries<T extends WithSeries>(
  books: ReadonlyArray<T>,
): ReadonlyArray<BookSeriesGroup<T>> {
  const seriesMap = new Map<string, T[]>();
  const standalone: T[] = [];
  for (const book of books) {
    if (book.series?.name) {
      const bucket = seriesMap.get(book.series.name) ?? [];
      bucket.push(book);
      seriesMap.set(book.series.name, bucket);
    } else {
      standalone.push(book);
    }
  }
  const groups = [...seriesMap.entries()]
    .map(([name, groupBooks]) => ({
      key: `series:${name}`,
      name,
      books: [...groupBooks].sort((a, b) => (a.series?.order ?? 0) - (b.series?.order ?? 0)),
    }))
    .sort((a, b) => {
      const ao = Math.min(...a.books.map((x) => x.series?.order ?? Number.MAX_SAFE_INTEGER));
      const bo = Math.min(...b.books.map((x) => x.series?.order ?? Number.MAX_SAFE_INTEGER));
      return ao - bo;
    });
  if (standalone.length) groups.push({ key: "standalone", name: "", books: standalone });
  return groups;
}
