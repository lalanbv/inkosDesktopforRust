// 464 号：ServiceQuickLinks 三库快捷链接（纯静态面冒烟）。
import { describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import { getServiceQuickLinks, ServiceQuickLinks } from "./ServiceQuickLinks";

describe("ServiceQuickLinks（464 号）", () => {
  it("已知服务商返回链接清单（含官网/文档/价格）", () => {
    const links = getServiceQuickLinks("openrouter");
    expect(links).toHaveLength(3);
    expect(links.map((l) => l.href)).toContain("https://openrouter.ai/keys");
    expect(links.every((l) => l.label.length > 0)).toBe(true);
  });

  it("未知服务商返回空数组", () => {
    expect(getServiceQuickLinks("custom:Mock")).toEqual([]);
  });

  it("detail 变体渲染链接锚点（新窗口+noreferrer）；未知服务商渲染 null", () => {
    const html = renderToString(<ServiceQuickLinks serviceId="openrouter" />);
    expect(html).toContain('target="_blank"');
    expect(html).toContain('rel="noreferrer"');
    expect(html).toContain("https://openrouter.ai/keys");

    const empty = renderToString(<ServiceQuickLinks serviceId="custom:Mock" />);
    expect(empty).not.toContain("https://");
  });
});
