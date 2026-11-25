// @vitest-environment jsdom
//! R58/1676 号：驳回钮两步确认。驳回是回滚的入口——此前点一下就回滚到上一章的快照，
//! 误触即回退一整段进度。断言：点了先弹确认（文案点名回滚到第几章、去向是回收站）、
//! 确认之前不发请求；取消零请求；确认只发一次。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { RejectChapterButton } from "./BookDetail";
import type { TFunction } from "../hooks/use-i18n";

const messages: Record<string, string> = {
  "book.reject": "驳回",
  "book.confirmReject": "驳回后回滚到第 {n} 章的快照，这一章及其后的章节与状态移入回收站。",
  "common.cancel": "取消",
};
const t = ((key: string) => messages[key] ?? key) as TFunction;

function renderReject(onReject: () => Promise<void>, chapterNumber = 4) {
  const view = render(<RejectChapterButton chapterNumber={chapterNumber} onReject={onReject} t={t} />);
  const trigger = view.getByTitle("驳回");
  return { ...view, trigger };
}

describe("驳回钮两步确认（R58/1676）", () => {
  afterEach(() => cleanup());

  it("点了先弹确认，点名回滚到第几章与去向；确认之前不发请求", () => {
    const onReject = vi.fn(async () => undefined);
    const { trigger, getByText, queryByText } = renderReject(onReject);
    expect(queryByText(messages["book.confirmReject"]!)).toBeNull();

    fireEvent.click(trigger);
    expect(onReject).not.toHaveBeenCalled();
    expect(getByText("驳回后回滚到第 3 章的快照，这一章及其后的章节与状态移入回收站。")).toBeTruthy();
  });

  it("取消：关掉确认，零请求", () => {
    const onReject = vi.fn(async () => undefined);
    const { trigger, getByText, queryByText } = renderReject(onReject);
    fireEvent.click(trigger);
    fireEvent.click(getByText("取消"));
    expect(onReject).not.toHaveBeenCalled();
    expect(queryByText(messages["book.confirmReject"]!)).toBeNull();
    expect(queryByText("驳回后回滚到第 3 章的快照，这一章及其后的章节与状态移入回收站。")).toBeNull();
  });

  it("确认：发一次请求，关掉确认", async () => {
    const onReject = vi.fn(async () => undefined);
    const { trigger, getAllByText, queryByText } = renderReject(onReject);
    fireEvent.click(trigger);
    // 弹层标题也是「驳回」——确认钮是其中唯一的按钮（触发钮只有 title、没有文字）。
    const confirm = getAllByText("驳回").find((element) => element.tagName === "BUTTON");
    expect(confirm).toBeTruthy();
    fireEvent.click(confirm!);
    await Promise.resolve();
    expect(onReject).toHaveBeenCalledTimes(1);
    expect(queryByText("驳回后回滚到第 3 章的快照，这一章及其后的章节与状态移入回收站。")).toBeNull();
  });

  it("第 1 章的驳回确认写的是回滚到第 0 章（开书时的快照）", () => {
    const { trigger, getByText } = renderReject(vi.fn(async () => undefined), 1);
    fireEvent.click(trigger);
    expect(getByText(/回滚到第 0 章/u)).toBeTruthy();
  });
});
