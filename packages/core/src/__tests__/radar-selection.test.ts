import { describe, expect, it } from "vitest";
import { filterRankingsBySelection, type RadarSelection } from "../agents/radar.js";
import type { PlatformRankings } from "../agents/radar-source.js";

/**
 * 707 号：选后再析（G14a/335）恢复——filterRankingsBySelection 纯函数面
 * （三维度独立过滤；空 = 不过滤该维度；全空 = 全量）。fork 原实现逐字，
 * engine-rs filter_rankings_by_selection 同水位对偶。
 */
const rankings: ReadonlyArray<PlatformRankings> = [
  {
    platform: "番茄小说",
    entries: [
      { title: "镜花缘", author: "甲", category: "玄幻", extra: "[热门榜]" },
      { title: "都市之镜", author: "乙", category: "都市", extra: "[黑马榜]" },
    ],
  },
  {
    platform: "起点",
    entries: [{ title: "剑来", author: "丙", category: "仙侠", extra: "[热门榜]" }],
  },
];

describe("filterRankingsBySelection (707: G14a/335 selection restore)", () => {
  it("无 selection 原样返回", () => {
    expect(filterRankingsBySelection(rankings)).toBe(rankings);
    expect(filterRankingsBySelection(rankings, undefined)).toBe(rankings);
  });

  it("平台维度：大小写不敏感精确匹配", () => {
    const out = filterRankingsBySelection(rankings, { platforms: ["番茄小说"] } as RadarSelection);
    expect(out).toHaveLength(1);
    expect(out[0]!.platform).toBe("番茄小说");
    expect(out[0]!.entries).toHaveLength(2);
  });

  it("分类维度：子串包含匹配，命中条目所在平台保留", () => {
    const out = filterRankingsBySelection(rankings, { categories: ["玄"] } as RadarSelection);
    expect(out).toHaveLength(1);
    expect(out[0]!.platform).toBe("番茄小说");
    expect(out[0]!.entries.map((e) => e.title)).toEqual(["镜花缘"]);
  });

  it("标题维度：子串包含匹配", () => {
    const out = filterRankingsBySelection(rankings, { titles: ["剑来"] } as RadarSelection);
    expect(out.map((r) => r.platform)).toEqual(["起点"]);
  });

  it("三维度独立：分类×标题 OR 合并命中", () => {
    const out = filterRankingsBySelection(rankings, { categories: ["都市"], titles: ["剑来"] } as RadarSelection);
    expect(out.map((r) => r.platform)).toEqual(["番茄小说", "起点"]);
    expect(out[0]!.entries.map((e) => e.title)).toEqual(["都市之镜"]);
  });

  it("全空 selection = 不过滤任何维度（与旧行为一致）", () => {
    const out = filterRankingsBySelection(rankings, { platforms: [], categories: [], titles: [] } as RadarSelection);
    expect(out).toEqual(rankings);
  });

  it("过滤后空平台被剔除", () => {
    const out = filterRankingsBySelection(rankings, { titles: ["不存在的书"] } as RadarSelection);
    expect(out).toHaveLength(0);
  });
});
