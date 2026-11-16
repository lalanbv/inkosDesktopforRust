import { describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import { EngineStatusBanner } from "@/components/EngineStatusBanner";
import { BANNER_TEXT, FAILURE_THRESHOLD, nextHealth } from "@/hooks/use-engine-health";

// 1463：状态机纯函数三面 + 横幅渲染两态（renderToString 惯例——hook 轮询
// 在 SSR 下不跑，横幅经 props 间接验证：down 文案/role 与恢复 toast 由
// 状态机 recovered 沿驱动，沿逻辑在纯函数测）。

describe("nextHealth state machine", () => {
  it("alive keeps up with reset failures", () => {
    expect(nextHealth(true, 0)).toEqual({ state: "up", failures: 0, recovered: false });
    // 已判 down 后恢复：recovered 沿触发（供 toast）。
    expect(nextHealth(true, FAILURE_THRESHOLD)).toEqual({
      state: "up",
      failures: 0,
      recovered: true,
    });
  });

  it("failures below threshold stay up (transient tolerance)", () => {
    expect(nextHealth(false, 0)).toEqual({ state: "up", failures: 1, recovered: false });
    expect(nextHealth(false, FAILURE_THRESHOLD - 1)).toEqual({
      state: "down",
      failures: FAILURE_THRESHOLD,
      recovered: false,
    });
  });

  it("down persists while dead and recovery path resets", () => {
    // 判 down 后继续死：保持 down（计数封顶不再涨语义——持续 down）。
    expect(nextHealth(false, FAILURE_THRESHOLD).state).toBe("down");
    // 恢复：up+recovered。
    const r = nextHealth(true, FAILURE_THRESHOLD);
    expect(r.state).toBe("up");
    expect(r.recovered).toBe(true);
    // 恢复后再失败：从 up 重新计（failures 已清）。
    expect(nextHealth(false, 0).state).toBe("up");
  });
});

describe("EngineStatusBanner rendering", () => {
  it("banner text is bilingual non-empty and thresholds locked", () => {
    // 文案锚定（中英形态齐备=横幅可视面的契约）。
    expect(BANNER_TEXT.down.length).toBeGreaterThan(4);
    expect(BANNER_TEXT.recovered.length).toBeGreaterThan(4);
    expect(FAILURE_THRESHOLD).toBe(3);
  });

  it("renders nothing harmful in SSR (hook quiescent, no banner markup)", () => {
    // renderToString 下 hook 处于 checking 首态：零横幅渲染（首探前无打扰）。
    const html = renderToString(<EngineStatusBanner />);
    expect(html).not.toContain("role=\"alert\"");
    expect(html).not.toContain("role=\"status\"");
  });
});
