// @vitest-environment jsdom
//! 451 号：交互测试基建首件——TimelineEditDialog 草稿透传回归测试。
//!
//! 背景（450 号缺陷）：弹窗本地 title/note 草稿未传给 onWriteFromBeat，
//! 父级读 editTarget 旧值 → 未保存的节拍编辑对「按此节拍写下一章」静默
//! 失效。静态 renderToString 测试无法覆盖「输入后点击」的交互时序，
//! 故引入 @testing-library + jsdom（仅本文件启用，整包其余测试仍 node）。
//!
//! 回归断言：点击「按此节拍写下一章」必须携带输入框当前草稿，
//! 而非打开弹窗时的 target 旧值。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TimelineEditDialog } from "./BookTimeline";
import type { Nav } from "../lib/nav";
import type { TFunction } from "../hooks/use-i18n";

const nav = {} as Nav;
const t = ((key: string) => key) as TFunction;

function renderDialog(overrides: {
  onWriteFromBeat: ReturnType<typeof vi.fn>;
  target?: { plotlineId: string; plotlineName: string; chapter: number; title: string; note: string };
}) {
  const target = overrides.target ?? {
    plotlineId: "main",
    plotlineName: "主线",
    chapter: 1,
    title: "镜中醒来",
    note: "",
  };
  return render(
    <TimelineEditDialog
      bookId="b1"
      target={target}
      saving={false}
      writing={false}
      onWriteFromBeat={overrides.onWriteFromBeat}
      onSubmit={() => {}}
      onCancel={() => {}}
      nav={nav}
      t={t}
    />,
  );
}

describe("TimelineEditDialog 草稿透传（450 号回归）", () => {
  afterEach(() => cleanup());

  it("按此节拍写下一章携带输入框当前草稿（而非 target 旧值）", async () => {
    const user = userEvent.setup();
    const onWriteFromBeat = vi.fn();
    renderDialog({ onWriteFromBeat });

    const title = document.querySelector('[data-slot="timeline-beat-title"]') as HTMLInputElement;
    const note = document.querySelector('[data-slot="timeline-beat-note"]') as HTMLTextAreaElement;
    await user.clear(title);
    await user.type(title, "夜探镜渊");
    await user.type(note, "苏檀夜探镜渊，镜灵现身。");

    const writeBtn = document.querySelector('[data-slot="timeline-write-from-beat"]') as HTMLButtonElement;
    await user.click(writeBtn);

    expect(onWriteFromBeat).toHaveBeenCalledTimes(1);
    expect(onWriteFromBeat).toHaveBeenCalledWith({ title: "夜探镜渊", note: "苏檀夜探镜渊，镜灵现身。" });
  });

  it("未编辑时点击透传 target 原值（行为不变）", async () => {
    const user = userEvent.setup();
    const onWriteFromBeat = vi.fn();
    renderDialog({ onWriteFromBeat });

    const writeBtn = document.querySelector('[data-slot="timeline-write-from-beat"]') as HTMLButtonElement;
    await user.click(writeBtn);
    expect(onWriteFromBeat).toHaveBeenCalledWith({ title: "镜中醒来", note: "" });
  });
});
