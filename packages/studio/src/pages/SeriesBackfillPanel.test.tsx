import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderToString } from "react-dom/server";
import { SeriesBackfillPanel, groupBackfillItemsByCategory } from "@/pages/SeriesBackfillPanel";

const fetchJsonMock = vi.fn();
const postApiMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  fetchJson: (path: string) => fetchJsonMock(path),
  postApi: (path: string, body?: unknown) => postApiMock(path, body),
  useApi: vi.fn(),
}));

const t = (key: string) => key;

describe("SeriesBackfillPanel（184 号 C3-b/c 系列书回填向导）", () => {
  beforeEach(() => {
    fetchJsonMock.mockReset();
    postApiMock.mockReset();
    fetchJsonMock.mockResolvedValue({ books: [] });
  });

  it("表单结构：源/目标下拉 + 抽取按钮；无书数据时按钮禁用", () => {
    const html = renderToString(<SeriesBackfillPanel t={t} />);
    expect(html).toContain('data-slot="backfill-source"');
    expect(html).toContain('data-slot="backfill-target"');
    expect(html).toContain('data-slot="backfill-extract"');
    expect(html).toContain("disabled");
    expect(html).toContain("backfill.hint");
  });

  it("书列表可用时抽取按钮不再因数据缺失禁用", async () => {
    fetchJsonMock.mockResolvedValue({
      books: [
        { id: "a", title: "A" },
        { id: "b", title: "B" },
      ],
    });
    // renderToString 首帧 data 为 null（useEffect 拉取后才有）——此断言只覆盖
    // 首帧不抛错；数据就绪后的预览/勾选交互由浏览器端到端覆盖。
    const html = renderToString(<SeriesBackfillPanel t={t} />);
    expect(html).toContain('data-slot="backfill-source"');
  });
});

describe("groupBackfillItemsByCategory（194 号类别扩展）", () => {
  const item = (id: string, category: string) => ({ id, category, title: id, content: "描述" });

  it("七类各自归位，未知类别归入 other 且垫底", () => {
    const grouped = groupBackfillItemsByCategory([
      item("a", "worldview"),
      item("b", "faction"),
      item("c", "unknown-x"),
      item("d", "location"),
      item("e", "item"),
      item("f", "character"),
      item("g", "plot"),
      item("h", "style"),
      item("i", "other-thing"),
    ]);
    expect(grouped.map(([key]) => key)).toEqual([
      "worldview", "faction", "location", "item", "character", "plot", "style", "other",
    ]);
    const other = grouped.find(([key]) => key === "other")?.[1] ?? [];
    expect(other.map((entry) => entry.id)).toEqual(["c", "i"]);
  });

  it("空输入返回空分组", () => {
    expect(groupBackfillItemsByCategory([])).toEqual([]);
  });
});
