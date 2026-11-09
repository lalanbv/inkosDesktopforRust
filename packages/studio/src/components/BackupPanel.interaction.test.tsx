// @vitest-environment jsdom
//! 455 号：BackupPanel 交互测试——预览解包回归锁定（443 号修复：服务端
//! 线格式 {preview:{...}} 嵌套，面板须解包；此前平铺读取数字全 undefined）
//! + 确认恢复调用断言。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { BackupPanel } from "./BackupPanel";

const defaultFetch = async (path: string, _init?: { method?: string; body?: string }): Promise<unknown> => {
  if (path.includes("/backup/snapshots")) {
    return {
      snapshots: [
        { name: "pre-restore-1731100000001.tar", size: 4096 },
        { name: "pre-restore-1731100000000.tar", size: 4096 },
      ],
    };
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
};
const fetchJsonMock = vi.fn(defaultFetch);
const invalidateApiPathsMock = vi.fn((paths: ReadonlyArray<string>) => {
  window.dispatchEvent(new CustomEvent("inkos:api-invalidate", { detail: { paths: [...paths] } }));
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
  invalidateApiPaths: (paths: ReadonlyArray<string>) => invalidateApiPathsMock(paths),
}));

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
  fetchJsonMock.mockImplementation(defaultFetch);
  invalidateApiPathsMock.mockClear();
});

const buttonByText = (text: string): HTMLButtonElement | undefined =>
  Array.from(document.querySelectorAll("button")).find((b) => b.textContent?.trim() === text);

const uploadBackup = async (user: ReturnType<typeof userEvent.setup>, name: string) => {
  const input = document.querySelector('input[type="file"]') as HTMLInputElement;
  await user.upload(input, new File([new Uint8Array([0x1f, 0x8b, 0, 1])], name, { type: "application/gzip" }));
};

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

    // 挂载即列举快照；millis 名解析为本地时间+大小展示；同秒两份快照带毫秒
    // 区分（955 号——走查实证同秒标签相同时用户无从分辨）。
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("可回滚快照（恢复前自动留档）");
    });
    expect(document.body.textContent).toContain("4096");
    expect(document.body.textContent).toContain("回滚到此快照");
    const rollbackBtns = Array.from(document.querySelectorAll("button")).filter(
      (b) => b.textContent?.trim() === "回滚到此快照",
    );
    expect(rollbackBtns).toHaveLength(2);
    const labels = Array.from(document.querySelectorAll("span.text-muted-foreground"))
      .map((s) => s.textContent ?? "")
      .filter((text) => text.includes("4096"));
    expect(labels).toHaveLength(2);
    expect(new Set(labels).size).toBe(2); // 同秒两份可区分（955 前两行文本完全相同）
    expect(labels.every((label) => /\.\d{3} · 4096/.test(label))).toBe(true);

    // 回滚预览：POST /backup/rollback（无 confirm）显示覆盖面+点名所选快照。
    await user.click(rollbackBtns[0]!);
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("回滚该快照（");
    });
    expect(document.body.textContent).toContain("将覆盖 1 个文件");
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

  // ── 955 号：954 新增 UI 审查修复（在途互斥/预览互斥/错误色调/旧预览收起）──

  it("955 在途互斥: 同帧连击「确认回滚」只发一次 confirm，未返回前按钮禁用", async () => {
    const user = userEvent.setup();
    let resolveConfirm: (value: unknown) => void = () => undefined;
    fetchJsonMock.mockImplementation(async (path: string, init?: { method?: string; body?: string }) => {
      if (path.includes("/backup/rollback") && path.includes("confirm=1")) {
        return new Promise((resolve) => {
          resolveConfirm = resolve;
        });
      }
      return defaultFetch(path, init);
    });
    render(<BackupPanel />);
    await vi.waitFor(() => expect(buttonByText("回滚到此快照")).toBeDefined());
    await user.click(buttonByText("回滚到此快照")!);
    await vi.waitFor(() => expect(buttonByText("确认回滚")).toBeDefined());

    // 真人双击：同一 act 内两次 DOM 级点击（state 渲染前的窗口由 ref 守卫兜住）。
    const confirmBtn = buttonByText("确认回滚")!;
    act(() => {
      confirmBtn.click();
      confirmBtn.click();
    });
    const confirmCalls = () =>
      fetchJsonMock.mock.calls.filter(([path]) => (path as string).includes("/backup/rollback") && (path as string).includes("confirm=1"));
    expect(confirmCalls()).toHaveLength(1);
    expect(buttonByText("确认回滚")!.disabled).toBe(true);
    expect(buttonByText("回滚到此快照")!.disabled).toBe(true);

    await act(async () => {
      resolveConfirm({ ok: true, restored: 11, snapshot: true });
    });
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("已回滚到快照时刻（恢复 11 个文件；回滚前状态已另存快照）");
    });
    expect(confirmCalls()).toHaveLength(1);
    expect(buttonByText("回滚到此快照")!.disabled).toBe(false); // 在途结束即解除禁用
  });

  it("955 预览互斥: 回滚预览出现时导入预览收起，新选文件时回滚预览收起", async () => {
    const user = userEvent.setup();
    render(<BackupPanel />);
    await uploadBackup(user, "backup.tar.gz");
    await vi.waitFor(() => expect(buttonByText("确认恢复")).toBeDefined());

    await user.click(buttonByText("回滚到此快照")!);
    await vi.waitFor(() => expect(buttonByText("确认回滚")).toBeDefined());
    expect(buttonByText("确认恢复")).toBeUndefined();

    await uploadBackup(user, "backup-2.tar.gz");
    await vi.waitFor(() => expect(buttonByText("确认恢复")).toBeDefined());
    expect(buttonByText("确认回滚")).toBeUndefined();
  });

  it("955 旧预览收起: 选 B 预览失败时不得残留 A 的预览（954 前确认会按 A 的数字恢复 B）", async () => {
    const user = userEvent.setup();
    render(<BackupPanel />);
    await uploadBackup(user, "a.tar.gz");
    await vi.waitFor(() => expect(document.body.textContent).toContain("包内 3 个文件：新增 2，将覆盖 1"));

    fetchJsonMock.mockImplementation(async (path: string, init?: { method?: string; body?: string }) => {
      if (path.includes("/backup/import")) throw new Error("backup-manifest.json missing");
      return defaultFetch(path, init);
    });
    await uploadBackup(user, "b.tar.gz");
    await vi.waitFor(() => expect(document.body.textContent).toContain("backup-manifest.json missing"));
    expect(document.body.textContent).not.toContain("包内 3 个文件");
    expect(buttonByText("确认恢复")).toBeUndefined();
  });

  it("955 错误色调: 失败通知为 destructive 色、成功通知为 emerald 色，失败后按钮解除禁用并重列快照", async () => {
    const user = userEvent.setup();
    fetchJsonMock.mockImplementation(async (path: string, init?: { method?: string; body?: string }) => {
      if (path.includes("/backup/rollback")) throw new Error("snapshot not found");
      return defaultFetch(path, init);
    });
    render(<BackupPanel />);
    await vi.waitFor(() => expect(buttonByText("回滚到此快照")).toBeDefined());
    const listCalls = () => fetchJsonMock.mock.calls.filter(([path]) => (path as string).includes("/backup/snapshots")).length;
    const listedBefore = listCalls();
    await user.click(buttonByText("回滚到此快照")!);
    const failure = await vi.waitFor(() => {
      const node = Array.from(document.querySelectorAll("p")).find((p) => p.textContent?.includes("snapshot not found"));
      expect(node).toBeDefined();
      return node!;
    });
    expect(failure.className).toContain("text-destructive");
    expect(failure.className).not.toContain("text-emerald-600");
    expect(buttonByText("回滚到此快照")!.disabled).toBe(false);
    // 失败后重列（快照被外部删除=死行须消失；中途失败的新快照须可见）。
    await vi.waitFor(() => expect(listCalls()).toBeGreaterThan(listedBefore));

    fetchJsonMock.mockImplementation(defaultFetch);
    await uploadBackup(user, "backup.tar.gz");
    await vi.waitFor(() => expect(buttonByText("确认恢复")).toBeDefined());
    await user.click(buttonByText("确认恢复")!);
    const success = await vi.waitFor(() => {
      const node = Array.from(document.querySelectorAll("p")).find((p) => p.textContent?.includes("已恢复 3 个文件"));
      expect(node).toBeDefined();
      return node!;
    });
    expect(success.className).toContain("text-emerald-600");
  });

  // ── 956 号：完全回滚——introduced 清单展示 + 删除复选框（默认不勾）──

  it("956 完全回滚: introduced 清单展示，复选框默认不勾=保守回滚不带 deleteIntroduced", async () => {
    const user = userEvent.setup();
    fetchJsonMock.mockImplementation(async (path: string, init?: { method?: string; body?: string }) => {
      if (path.includes("/backup/rollback") && !path.includes("confirm=1")) {
        return { preview: { fileCount: 1, overwriting: ["books/b1/book.json"], newFiles: 0, introduced: ["notes/new.md", "works/w9/work.json"] } };
      }
      return defaultFetch(path, init);
    });
    render(<BackupPanel />);
    await vi.waitFor(() => expect(buttonByText("回滚到此快照")).toBeDefined());
    await user.click(buttonByText("回滚到此快照")!);
    await vi.waitFor(() => expect(document.body.textContent).toContain("恢复期间新增 2 个文件"));
    expect(document.body.textContent).toContain("notes/new.md、works/w9/work.json");
    const checkbox = document.querySelector('input[type="checkbox"]') as HTMLInputElement;
    expect(checkbox).not.toBeNull();
    expect(checkbox.checked).toBe(false); // 默认不勾=保守语义不变

    await user.click(buttonByText("确认回滚")!);
    await vi.waitFor(() => expect(document.body.textContent).toContain("已回滚到快照时刻"));
    const confirmCall = fetchJsonMock.mock.calls
      .map(([path]) => path as string)
      .find((p) => p.includes("/backup/rollback") && p.includes("confirm=1"))!;
    expect(confirmCall).not.toContain("deleteIntroduced");
  });

  it("956 完全回滚: 勾选后 confirm 带 deleteIntroduced=1，通知含删除计数与失败明细", async () => {
    const user = userEvent.setup();
    fetchJsonMock.mockImplementation(async (path: string, init?: { method?: string; body?: string }) => {
      if (path.includes("/backup/rollback") && !path.includes("confirm=1")) {
        return { preview: { fileCount: 1, overwriting: ["books/b1/book.json"], newFiles: 0, introduced: ["notes/new.md", "notes/link.md"] } };
      }
      if (path.includes("/backup/rollback") && path.includes("confirm=1")) {
        return { ok: true, restored: 1, snapshot: true, removed: 1, deleteFailed: [{ path: "notes/link.md", error: "not a regular file" }] };
      }
      return defaultFetch(path, init);
    });
    render(<BackupPanel />);
    await vi.waitFor(() => expect(buttonByText("回滚到此快照")).toBeDefined());
    await user.click(buttonByText("回滚到此快照")!);
    await vi.waitFor(() => expect(document.querySelector('input[type="checkbox"]')).not.toBeNull());
    await user.click(document.querySelector('input[type="checkbox"]') as HTMLInputElement);
    expect((document.querySelector('input[type="checkbox"]') as HTMLInputElement).checked).toBe(true);

    await user.click(buttonByText("确认回滚")!);
    await vi.waitFor(() => expect(document.body.textContent).toContain("删除 1 个恢复期间新增文件"));
    expect(document.body.textContent).toContain("1 个删除失败（notes/link.md）");
    const confirmCall = fetchJsonMock.mock.calls
      .map(([path]) => path as string)
      .find((p) => p.includes("/backup/rollback") && p.includes("confirm=1"))!;
    expect(confirmCall).toContain("&deleteIntroduced=1");
  });

  it("956 完全回滚: 无 introduced 清单（旧快照/全覆盖）时不渲染删除区，换快照预览重置勾选", async () => {
    const user = userEvent.setup();
    let withIntroduced = true;
    fetchJsonMock.mockImplementation(async (path: string, init?: { method?: string; body?: string }) => {
      if (path.includes("/backup/rollback") && !path.includes("confirm=1")) {
        return withIntroduced
          ? { preview: { fileCount: 1, overwriting: ["books/b1/book.json"], newFiles: 0, introduced: ["notes/new.md"] } }
          : { preview: { fileCount: 1, overwriting: ["books/b1/book.json"], newFiles: 0 } };
      }
      return defaultFetch(path, init);
    });
    render(<BackupPanel />);
    await vi.waitFor(() => expect(buttonByText("回滚到此快照")).toBeDefined());
    const rows = () => Array.from(document.querySelectorAll("button")).filter((b) => b.textContent?.trim() === "回滚到此快照");
    await user.click(rows()[0]!);
    await vi.waitFor(() => expect(document.querySelector('input[type="checkbox"]')).not.toBeNull());
    await user.click(document.querySelector('input[type="checkbox"]') as HTMLInputElement);

    // 换另一份快照预览（旧形态无 introduced）：删除区不渲染，勾选不跨快照残留。
    withIntroduced = false;
    await user.click(rows()[1]!);
    await vi.waitFor(() => expect(document.body.textContent).not.toContain("恢复期间新增"));
    expect(document.querySelector('input[type="checkbox"]')).toBeNull();
    await user.click(buttonByText("确认回滚")!);
    await vi.waitFor(() => expect(document.body.textContent).toContain("已回滚到快照时刻"));
    const confirmCall = fetchJsonMock.mock.calls
      .map(([path]) => path as string)
      .find((p) => p.includes("/backup/rollback") && p.includes("confirm=1"))!;
    expect(confirmCall).not.toContain("deleteIntroduced");
  });

  it("956 P9 书目同步: 恢复与回滚确认后广播 /api/v1/books 失效（走查反挖：完全回滚删书后书籍列表停留恢复前）", async () => {
    const user = userEvent.setup();
    const invalidated: string[][] = [];
    const listener = (event: Event) => invalidated.push((event as CustomEvent<{ paths: string[] }>).detail.paths);
    window.addEventListener("inkos:api-invalidate", listener);
    try {
      render(<BackupPanel />);
      await vi.waitFor(() => expect(buttonByText("回滚到此快照")).toBeDefined());
      await user.click(buttonByText("回滚到此快照")!);
      await vi.waitFor(() => expect(buttonByText("确认回滚")).toBeDefined());
      await user.click(buttonByText("确认回滚")!);
      await vi.waitFor(() => expect(document.body.textContent).toContain("已回滚到快照时刻"));
      expect(invalidated).toContainEqual(["/api/v1/books"]);

      // 恢复确认同样广播（导入包可能增删书）。
      await uploadBackup(user, "backup.tar.gz");
      await vi.waitFor(() => expect(buttonByText("确认恢复")).toBeDefined());
      await user.click(buttonByText("确认恢复")!);
      await vi.waitFor(() => expect(document.body.textContent).toContain("已恢复 3 个文件"));
      const bookInvalidations = invalidated.filter((paths) => paths.includes("/api/v1/books"));
      expect(bookInvalidations.length).toBeGreaterThanOrEqual(2);
    } finally {
      window.removeEventListener("inkos:api-invalidate", listener);
    }
  });
});
