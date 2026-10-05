// @vitest-environment jsdom
//! 644 号：WriteStatusBanner 三态回归——642 号把停止广播换成中性文案
//! 「写作已按您的要求停止。」后，遗留的 `includes("Operation aborted")` 判定
//! 不再命中，用户主动停止被红色失败分支包裹（188 号语义回归，活体走查实锤）。
//! 断言：双端中性文案与遗留 abort 字面 → 中性形态；其余错误 → 失败形态。
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render } from "@testing-library/react";
import { WriteStatusBanner } from "./BookDetail";
import type { TFunction } from "../hooks/use-i18n";

const t = ((key: string) => key) as TFunction;

function renderBanner(props: { writing?: boolean; drafting?: boolean; lastError?: string | null }) {
  return render(
    <WriteStatusBanner
      writing={props.writing ?? false}
      drafting={props.drafting ?? false}
      lastError={props.lastError ?? null}
      t={t}
    />,
  );
}

describe("WriteStatusBanner 三态（644 号回归）", () => {
  afterEach(() => cleanup());

  it("642 号中性停止文案 → 中性形态（旧判定红：被失败分支包裹）", () => {
    const { container } = renderBanner({ lastError: "写作已按您的要求停止。" });
    const banner = container.querySelector("div.rounded-2xl");
    expect(banner).not.toBeNull();
    expect(banner!.className).not.toContain("border-destructive");
    expect(banner!.className).toContain("bg-secondary/30");
    expect(banner!.textContent).toBe("book.writeStopped");
  });

  it("遗留 Operation aborted 字面 → 中性形态（既有语义回归保护）", () => {
    const { container } = renderBanner({ lastError: "Operation aborted" });
    const banner = container.querySelector("div.rounded-2xl");
    expect(banner).not.toBeNull();
    expect(banner!.className).not.toContain("border-destructive");
    expect(banner!.textContent).toBe("book.writeStopped");
  });

  it("真失败错误 → 失败形态 + pipelineFailed 前缀", () => {
    const { container } = renderBanner({ lastError: "LLM 连接超时" });
    const banner = container.querySelector("div.rounded-2xl");
    expect(banner).not.toBeNull();
    expect(banner!.className).toContain("border-destructive");
    expect(banner!.textContent).toBe("book.pipelineFailed: LLM 连接超时");
  });

  it("writing 中 → 写作中提示（primary 形态）", () => {
    const { container } = renderBanner({ writing: true });
    const banner = container.querySelector("div.rounded-2xl");
    expect(banner).not.toBeNull();
    expect(banner!.className).toContain("border-primary/20");
    expect(banner!.textContent).toBe("book.pipelineWriting");
  });

  it("无 writing/drafting/lastError → 不渲染", () => {
    const { container } = renderBanner({});
    expect(container.querySelector("div.rounded-2xl")).toBeNull();
  });
});
