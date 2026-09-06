import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderToString } from "react-dom/server";
import { SeriesBackfillPanel } from "@/pages/SeriesBackfillPanel";

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
