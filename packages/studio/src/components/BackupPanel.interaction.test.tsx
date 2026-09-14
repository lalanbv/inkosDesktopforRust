// @vitest-environment jsdom
//! 455 号：BackupPanel 交互测试——预览解包回归锁定（443 号修复：服务端
//! 线格式 {preview:{...}} 嵌套，面板须解包；此前平铺读取数字全 undefined）
//! + 确认恢复调用断言。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { BackupPanel } from "./BackupPanel";

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  if (path.includes("/backup/import")) {
    const confirm = path.includes("confirm=1");
    if (!confirm) {
      return { preview: { fileCount: 3, overwriting: ["books/b1/book.json"], newFiles: 2 } };
    }
    return { ok: true, restored: 3, snapshot: true };
  }
  return {};
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
});

describe("BackupPanel 交互（443 号预览解包回归）", () => {
  it("选择文件后渲染预览数字并触发确认恢复调用", async () => {
    const user = userEvent.setup();
    render(<BackupPanel />);

    const input = document.querySelector('input[type="file"]') as HTMLInputElement;
    const gz = new File([new Uint8Array([0x1f, 0x8b, 0, 1])], "backup.tar.gz", {
      type: "application/gzip",
    });
    await user.upload(input, gz);

    // 预览数字可读（修复前为 undefined——平铺读取嵌套线格式）
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("包内 3 个文件：新增 2，将覆盖 1");
    });
    expect(document.body.textContent).toContain("将覆盖：books/b1/book.json");

    // 确认恢复：带 confirm=1 再次调用，成功通知含恢复数与快照提示
    const confirmBtn = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "确认恢复",
    ) as HTMLButtonElement;
    await user.click(confirmBtn);
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("已恢复 3 个文件（原文件快照在 backups/）");
    });
    const calls = fetchJsonMock.mock.calls.map(([path]) => path as string);
    expect(calls.filter((p) => p.includes("/backup/import")).length).toBe(2);
    expect(calls.some((p) => p.includes("confirm=1"))).toBe(true);
  });
});
