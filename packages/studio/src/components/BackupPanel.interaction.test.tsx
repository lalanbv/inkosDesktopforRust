// @vitest-environment jsdom
//! 455 号：BackupPanel 交互测试——预览解包回归锁定（443 号修复：服务端
//! 线格式 {preview:{...}} 嵌套，面板须解包；此前平铺读取数字全 undefined）
//! + 确认恢复调用断言。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { BackupPanel } from "./BackupPanel";

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  if (path.includes("/backup/snapshots")) {
    return { snapshots: [{ name: "pre-restore-1731100000000.tar", size: 4096 }] };
  }
  if (path.includes("/backup/rollback")) {
    if (!path.includes("confirm=1")) {
      return { preview: { fileCount: 12, overwriting: ["books/b1/book.json"], newFiles: 0 } };
    }
    return { ok: true, restored: 11, snapshot: true };
  }
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

  it("954 回滚区: 快照列表渲染 + 回滚两段式（预览覆盖面 → 确认）+ 列表刷新", async () => {
    const user = userEvent.setup();
    render(<BackupPanel />);

    // 挂载即列举快照；millis 名解析为本地时间+大小展示
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("可回滚快照（恢复前自动留档）");
    });
    expect(document.body.textContent).toContain("4096");
    expect(document.body.textContent).toContain("回滚到此快照");

    // 回滚预览：POST /backup/rollback（无 confirm）显示覆盖面
    const rollbackBtn = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "回滚到此快照",
    ) as HTMLButtonElement;
    await user.click(rollbackBtn);
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("回滚该快照：将覆盖 1 个文件");
    });
    expect(document.body.textContent).toContain("将覆盖：books/b1/book.json");

    // 确认回滚：带 confirm=1；成功后列表刷新（列举调用 ≥2 次）
    fetchJsonMock.mockClear();
    const confirmBtn = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "确认回滚",
    ) as HTMLButtonElement;
    await user.click(confirmBtn);
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("已回滚到快照时刻（恢复 11 个文件；回滚前状态已另存快照）");
    });
    const calls = fetchJsonMock.mock.calls.map(([path]) => path as string);
    expect(calls.some((p) => p.includes("/backup/rollback") && p.includes("confirm=1"))).toBe(true);
    expect(calls.some((p) => p.includes("/backup/snapshots"))).toBe(true);
  });
});
