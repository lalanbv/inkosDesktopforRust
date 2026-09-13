import { describe, it, expect } from "vitest";
import { parseHash, routeToHash } from "../hooks/use-hash-route";

// 405 号走查修复：#/radar 直达/刷新此前回落 Dashboard（parseHash 缺分支 +
// routeToHash 落 default 空串）——市场雷达深链回归锁定。
describe("radar route", () => {
  it("parses #/radar", () => { expect(parseHash("#/radar")).toEqual({ page: "radar" }); });
  it("round-trips", () => { expect(routeToHash({ page: "radar" })).toBe("#/radar"); });
  it("still falls back to dashboard for unknown paths", () => {
    expect(parseHash("#/no-such-page")).toEqual({ page: "dashboard" });
  });
});
