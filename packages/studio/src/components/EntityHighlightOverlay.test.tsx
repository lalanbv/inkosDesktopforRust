// @vitest-environment jsdom
//! R52/1656 号：实体高亮 overlay 组件测——命中渲染/悬停实体卡/大章双闸。
//! 纯函数语义由 core golden 锁定；像素级对齐由 IAB 走查（变更记录备案）
//! 与 EDITOR_TYPOGRAPHY_CLASS 单源机制保障。
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { EntityHighlightOverlay, ENTITY_OVERLAY_MAX_CHARS } from "./EntityHighlightOverlay";

const cards = [
  { name: "林动", aliases: ["林动大哥"], kind: "person", summary: "主角，天才少年。", facts: ["武祖血脉"] },
  { name: "Lin Dong", aliases: [], kind: "person", summary: "English entity." },
];

function hostRender(text: string) {
  const areaRef = { current: document.createElement("textarea") };
  const { container } = render(
    <div className="relative">
      <EntityHighlightOverlay text={text} cards={cards} textareaRef={areaRef} />
    </div>,
  );
  return container;
}

describe("EntityHighlightOverlay（R52/1656 号）", () => {
  // 真实时钟（fake timers 与 waitFor/useDeferredValue 调度死锁——1653 教训）。
  afterEach(() => {
    cleanup();
  });

  it("命中段渲染 mark 背景（最长别名优先 + 大小写敏感）", async () => {
    const container = hostRender("林动大哥与 Lin Dong 相遇，lin dong 不亮。");
    await waitFor(() => {
      const hit = container.querySelector('[data-entity-hit="林动大哥"]');
      expect(hit?.textContent).toBe("林动大哥");
    });
    expect(container.querySelector('[data-entity-hit="Lin Dong"]')?.textContent).toBe("Lin Dong");
    // 小写不命中。
    expect(container.querySelector('[data-entity-hit="lin dong"]')).toBeNull();
    // 普通段透明文本与命中段拼接=原文。
    const mirror = container.querySelector('[aria-hidden="true"]');
    expect(mirror?.textContent).toBe("林动大哥与 Lin Dong 相遇，lin dong 不亮。");
    // 镜像层透明（文本由 textarea 渲染——防双影机制）。
    expect(mirror?.className).toContain("text-transparent");
  });

  it("悬停命中段出实体卡（summary/facts/别名）", async () => {
    const container = hostRender("林动出场。");
    await waitFor(() => {
      expect(container.querySelector('[data-entity-hit="林动"]')).toBeTruthy();
    });
    fireEvent.mouseEnter(container.querySelector('[data-entity-hit="林动"]')!);
    const tooltip = await waitFor(() => {
      const card = container.querySelector('[data-entity-card="hover"]');
      expect(card).toBeTruthy();
      return card!;
    });
    expect(tooltip.getAttribute("role")).toBe("tooltip");
    expect(tooltip.textContent).toContain("主角，天才少年。");
    expect(tooltip.textContent).toContain("武祖血脉");
    expect(tooltip.textContent).toContain("林动大哥");
    fireEvent.mouseLeave(container.querySelector('[data-entity-hit="林动"]')!);
    await waitFor(() => {
      expect(container.querySelector('[data-entity-card="hover"]')).toBeNull();
    });
  });

  it("大章双闸：>2 万码元禁用（overlay off + 零命中 DOM）", async () => {
    const large = "林动".repeat(11_000);
    expect(large.length).toBeGreaterThan(ENTITY_OVERLAY_MAX_CHARS);
    const container = hostRender(large);
    await waitFor(() => {
      expect(container.querySelector('[data-entity-overlay="off"]')).toBeTruthy();
    });
    expect(container.querySelector('[data-entity-hit]')).toBeNull();
  });

  it("空卡/空文本零命中不炸", async () => {
    const areaRef = { current: document.createElement("textarea") };
    const { container } = render(
      <div className="relative">
        <EntityHighlightOverlay text="" cards={[]} textareaRef={areaRef} />
      </div>,
    );
    await waitFor(() => {
      expect(container.querySelector('[data-entity-overlay="on"]')).toBeTruthy();
    });
    expect(container.querySelector('[data-entity-hit]')).toBeNull();
  });
});
