import { describe, expect, it } from "vitest";
import { groupBooksBySeries } from "./dashboard-book-groups";

interface Row {
  readonly id: string;
  readonly series?: { readonly name: string; readonly order: number };
}

describe("groupBooksBySeries（180 号 C3-a Dashboard 系列分组）", () => {
  it("有系列的聚组按卷序升序，散书归末尾散书组", () => {
    const rows: Row[] = [
      { id: "solo", series: undefined },
      { id: "s3", series: { name: "斗气大陆", order: 3 } },
      { id: "s1", series: { name: "斗气大陆", order: 1 } },
      { id: "o2", series: { name: "另一部", order: 2 } },
      { id: "s2", series: { name: "斗气大陆", order: 2 } },
    ];
    const groups = groupBooksBySeries<Row>(rows);
    expect(groups.map((g) => g.name)).toEqual(["斗气大陆", "另一部", ""]);
    const douQi = groups[0];
    expect(douQi.key).toBe("series:斗气大陆");
    expect(douQi.books.map((b) => b.id)).toEqual(["s1", "s2", "s3"]);
    expect(groups[2].key).toBe("standalone");
    expect(groups[2].books.map((b) => b.id)).toEqual(["solo"]);
  });

  it("无系列数据时全部进散书组且不产生组头名", () => {
    const groups = groupBooksBySeries<Row>([{ id: "a" }, { id: "b" }]);
    expect(groups).toHaveLength(1);
    expect(groups[0].name).toBe("");
    expect(groups[0].books).toHaveLength(2);
  });

  it("空列表返回空组", () => {
    expect(groupBooksBySeries([])).toEqual([]);
  });
});
