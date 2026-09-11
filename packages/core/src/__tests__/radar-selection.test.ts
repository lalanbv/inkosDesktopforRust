//! G14a/335 号：雷达"选后再析"单测。
//!
//! 免费扫榜（fetchRankings 不经 LLM）与勾选范围过滤
//! （filterRankingsBySelection 三维度独立，空 = 不过滤）；信号卡字段
//! （crowding/differentiation）由 LLM JSON 解析整体透传，解析侧差分在
//! engine-rs tests（parse_passes_signal_card_fields_through）镜像。
import { describe, expect, it } from "vitest";
import {
  fetchRankings,
  filterRankingsBySelection,
  type RadarSelection,
} from "../agents/radar.js";
import { TextRadarSource } from "../agents/radar-source.js";

const RANKINGS = [
  {
    platform: "番茄小说",
    entries: [
      { title: "书A", author: "", category: "都市", extra: "" },
      { title: "书B", author: "", category: "仙侠", extra: "" },
    ],
  },
  {
    platform: "起点中文网",
    entries: [{ title: "书C", author: "", category: "都市", extra: "" }],
  },
];

describe("radar select-then-analyze (G14a)", () => {
  it("fetches rankings from sources without any LLM call", async () => {
    const source = new TextRadarSource("# 榜单\n- 书A (作者X) [都市]");
    const rankings = await fetchRankings([source]);
    expect(rankings).toHaveLength(1);
    expect(rankings[0]!.platform).toBe("external");
    expect(rankings[0]!.entries.length).toBeGreaterThan(0);
  });

  it("passes everything through when selection is empty", () => {
    expect(filterRankingsBySelection(RANKINGS, undefined)).toEqual(RANKINGS);
    expect(filterRankingsBySelection(RANKINGS, {})).toEqual(RANKINGS);
    expect(filterRankingsBySelection(RANKINGS, {} as RadarSelection)).toEqual(RANKINGS);
  });

  it("filters by selected platforms", () => {
    const got = filterRankingsBySelection(RANKINGS, { platforms: ["起点中文网"] });
    expect(got).toHaveLength(1);
    expect(got[0]!.platform).toBe("起点中文网");
  });

  it("filters by selected categories across platforms", () => {
    const got = filterRankingsBySelection(RANKINGS, { categories: ["都市"] });
    expect(got).toHaveLength(2);
    expect(got[0]!.entries).toHaveLength(1);
    expect(got[0]!.entries[0]!.title).toBe("书A");
    expect(got[1]!.entries[0]!.title).toBe("书C");
  });

  it("drops platforms whose entries no longer match", () => {
    const got = filterRankingsBySelection(RANKINGS, { categories: ["科幻"] });
    expect(got).toEqual([]);
  });

  it("matches titles as a fallback dimension", () => {
    const got = filterRankingsBySelection(RANKINGS, { titles: ["书B"] });
    expect(got).toHaveLength(1);
    expect(got[0]!.entries[0]!.title).toBe("书B");
  });
});
