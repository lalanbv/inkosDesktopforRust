// @vitest-environment jsdom
//! 1650 号：卷管理卡对话框 React 化回归——wry 0.57 的 WKUIDelegate 未实现
//! runJavaScriptTextInputPanel/ConfirmPanel（桌面壳内 prompt 返回 null、
//! confirm 恒 false=新建/改名/删卷死按钮）。锁三链：
//! ①新建卷：PromptDialog 输入 → PUT volumes 携新卷
//! ②改名：PromptDialog 预填当前名 → 覆写 → PUT volumes
//! ③删卷：ConfirmDialog 确认 → PUT volumes 去掉该卷（取消=零请求）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { VolumeManageCard } from "./VolumeManageCard";

const fetchJsonMock = vi.fn(async (_path: string, _init?: unknown) => ({ ok: true, volumes: [] }));
const refetchMock = vi.fn();
const useApiMock = vi.fn((_path: string) => ({
  data: { volumes: [{ id: "vol-1", title: "第一卷", order: 1 }] },
  error: null,
  loading: false,
  refetch: refetchMock,
}));

vi.mock("../hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: (path: string, init?: unknown) => fetchJsonMock(path, init),
  postApi: vi.fn(),
}));

function lastPutBody(): { volumes?: Array<{ id: string; title: string; order: number }> } {
  const call = fetchJsonMock.mock.calls.find(
    (args) => String(args[0]).includes("/volumes") && (args[1] as { method?: string } | undefined)?.method === "PUT",
  );
  return call ? (JSON.parse((call[1] as { body: string }).body) as ReturnType<typeof lastPutBody>) : {};
}

/** 弹层内按钮按文案甄别（portal 挂 body；确认/取消文案与卡内按钮区分靠 .fixed 容器）。 */
function dialogButton(text: string): HTMLButtonElement {
  const btn = Array.from(document.querySelectorAll("button")).find(
    (b) => b.textContent?.trim() === text && !!b.closest("div.fixed.inset-0"),
  );
  expect(btn).toBeDefined();
  return btn as HTMLButtonElement;
}

beforeEach(() => {
  fetchJsonMock.mockClear();
  refetchMock.mockClear();
});

afterEach(() => cleanup());

describe("VolumeManageCard 对话框链（1650 号 React 化）", () => {
  it("新建卷：PromptDialog 输入 → PUT volumes 携新卷（壳内 prompt 死路修复）", async () => {
    const user = userEvent.setup();
    render(<VolumeManageCard bookId="b1" chapters={[{ number: 1 }]} />);
    await user.click(screen.getByText("＋ 新建卷"));
    const input = await vi.waitFor(() => {
      const el = document.querySelector("div.fixed.inset-0 input") as HTMLInputElement;
      expect(el).not.toBeNull();
      return el;
    });
    await user.type(input, "试炼卷");
    fireEvent.click(dialogButton("保存"));
    await vi.waitFor(() => {
      const body = lastPutBody();
      expect(body.volumes).toHaveLength(2);
      expect(body.volumes?.[1]).toMatchObject({ title: "试炼卷", order: 2 });
    });
    expect(refetchMock).toHaveBeenCalled();
  });

  it("改名：PromptDialog 预填当前名 → 覆写提交（Enter 通道）", async () => {
    const user = userEvent.setup();
    render(<VolumeManageCard bookId="b1" chapters={[]} />);
    await user.click(screen.getByText("改名"));
    const input = await vi.waitFor(() => {
      const el = document.querySelector("div.fixed.inset-0 input") as HTMLInputElement;
      expect(el).not.toBeNull();
      return el;
    });
    expect(input.value).toBe("第一卷");
    await user.clear(input);
    await user.type(input, "觉醒卷{Enter}");
    await vi.waitFor(() => {
      const body = lastPutBody();
      expect(body.volumes).toEqual([{ id: "vol-1", title: "觉醒卷", order: 1 }]);
    });
  });

  it("删卷：ConfirmDialog 确认 → PUT volumes 移除；取消零请求", async () => {
    const user = userEvent.setup();
    render(<VolumeManageCard bookId="b1" chapters={[{ number: 2, volumeId: "vol-1" }]} />);
    await user.click(screen.getByText("删卷"));
    // 先取消：零 PUT。
    fireEvent.click(dialogButton("取消"));
    expect(fetchJsonMock.mock.calls.find(([p, i]) => String(p).includes("/volumes") && (i as { method?: string })?.method === "PUT")).toBeUndefined();
    // 再删：确认 → PUT 剩空列表（删卷迁移由引擎事务承接）。
    await user.click(screen.getByText("删卷"));
    fireEvent.click(dialogButton("删除"));
    await vi.waitFor(() => {
      expect(lastPutBody().volumes).toEqual([]);
    });
  });
});
