import { describe, expect, it } from "vitest";
import {
  buildTimelineAfterCellEdit,
  buildTimelineAfterAddPlotline,
  initializeTimelineFromChapters,
  type TimelineDoc,
} from "./timeline-edit";

function seed(): TimelineDoc {
  return {
    version: 1,
    bookId: "b1",
    updatedAt: "2026-09-07T00:00:00.000Z",
    plotlines: [
      { id: "main", name: "主线", cells: [{ chapter: 1, title: "风起", note: "入场" }] },
      { id: "side", name: "支线", cells: [] },
    ],
  };
}

describe("buildTimelineAfterCellEdit（182 号 C4-c 单元格编辑）", () => {
  it("更新已有 cell：非空取 trim 值，updatedAt 刷新", () => {
    const next = buildTimelineAfterCellEdit(seed(), "main", 1, { title: "  风起（改）  ", note: "新备注" });
    const cell = next.plotlines[0].cells[0];
    expect(cell.title).toBe("风起（改）");
    expect(cell.note).toBe("新备注");
    expect(next.updatedAt).not.toBe("2026-09-07T00:00:00.000Z");
    // 原 doc 不被变异（不可变更新）。
    expect(seed().plotlines[0].cells[0].title).toBe("风起");
  });

  it("新建 cell：cells 按章序归位", () => {
    const next = buildTimelineAfterCellEdit(seed(), "main", 3, { title: "危机", note: "大比" });
    expect(next.plotlines[0].cells.map((c) => c.chapter)).toEqual([1, 3]);
    expect(next.plotlines[0].cells[1].title).toBe("危机");
  });

  it("留空即清除该字段；仅 chapter 的空 cell 合法保留", () => {
    const cleared = buildTimelineAfterCellEdit(seed(), "main", 1, { title: "", note: "" });
    expect(cleared.plotlines[0].cells[0].title).toBeUndefined();
    expect(cleared.plotlines[0].cells[0].note).toBeUndefined();
    expect(cleared.plotlines[0].cells[0].chapter).toBe(1);

    const newEmpty = buildTimelineAfterCellEdit(seed(), "side", 2, { title: "", note: "" });
    expect(newEmpty.plotlines[1].cells).toEqual([{ chapter: 2 }]);
  });

  it("其他情节线不受影响", () => {
    const next = buildTimelineAfterCellEdit(seed(), "main", 1, { title: "改", note: "" });
    expect(next.plotlines[1].cells).toEqual([]);
  });
});

describe("initializeTimelineFromChapters（兜底单线 → 可编辑 timeline）", () => {
  it("每章一个 cell（章序），单条主线", () => {
    const doc = initializeTimelineFromChapters(
      "b1",
      [{ number: 2, title: "第二章" }, { number: 1, title: "第一章" }],
      "主线",
    );
    expect(doc.version).toBe(1);
    expect(doc.bookId).toBe("b1");
    expect(doc.plotlines).toHaveLength(1);
    expect(doc.plotlines[0].name).toBe("主线");
    expect(doc.plotlines[0].cells.map((c) => c.chapter)).toEqual([1, 2]);
    expect(doc.plotlines[0].cells[0].title).toBe("第一章");
  });
});

describe("buildTimelineAfterAddPlotline（新增情节线）", () => {
  it("追加空 cells 的新线，原名保留", () => {
    const next = buildTimelineAfterAddPlotline(seed(), "  感情线  ");
    expect(next.plotlines).toHaveLength(3);
    expect(next.plotlines[2].name).toBe("感情线");
    expect(next.plotlines[2].cells).toEqual([]);
  });
});
