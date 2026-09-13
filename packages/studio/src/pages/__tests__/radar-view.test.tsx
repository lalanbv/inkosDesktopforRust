import { describe, expect, it, vi } from "vitest";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { RadarView } from "../RadarView";

// R27/402 号冒烟：RadarView 接受可选 onCreateBook（雷达→一键开书），
// 空态挂载不炸、不渲染推荐卡按钮（result 为内部态，交互路径由纯函数
// buildBookCreatePrefill 测试 + App 接线 tsc 锁定）。
describe("RadarView create-book wiring (R27/402)", () => {
  it("mounts the empty state with the create-book callback provided", () => {
    const onCreateBook = vi.fn();
    const html = renderToStaticMarkup(
      <RadarView
        nav={{ toDashboard: vi.fn() }}
        theme="light"
        t={(key) => (key === "nav.connected" ? "已连接" : key)}
        onCreateBook={onCreateBook}
      />,
    );
    expect(typeof html).toBe("string");
    expect(html.length).toBeGreaterThan(0);
    // 服务端渲染不触发 effect → 无数据 → 回调未被调用。
    expect(onCreateBook).not.toHaveBeenCalled();
  });

  it("mounts without the callback (back-compat with previous call sites)", () => {
    const html = renderToStaticMarkup(
      <RadarView nav={{ toDashboard: vi.fn() }} theme="dark" t={(key) => key} />,
    );
    expect(typeof html).toBe("string");
    expect(html.length).toBeGreaterThan(0);
  });
});
