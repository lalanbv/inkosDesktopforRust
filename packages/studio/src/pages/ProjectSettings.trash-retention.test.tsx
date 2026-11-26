// @vitest-environment jsdom
//! 1685 号（R60 第三批）：回收站保留期设置卡——读面回填、合法值 PUT、非法值本地
//! 校验不发请求（错误落 notice）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProjectSettings } from "./ProjectSettings";

const fetchJsonMock = vi.fn();
const putApiMock = vi.fn();
const useApiMock = vi.fn((_path: string) => ({ data: null, loading: false, error: null, refetch: vi.fn(), mutate: vi.fn() }));
const postApiMock = vi.fn();

vi.mock("../hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: (...args: unknown[]) => fetchJsonMock(...(args as [])),
  postApi: (...args: unknown[]) => postApiMock(...(args as [])),
  putApi: (...args: unknown[]) => putApiMock(...(args as [])),
  buildApiUrl: (path: string) => path,
}));

const nav = { toDashboard: () => undefined, toBook: () => undefined, toServices: () => undefined } as never;
const t = (key: string) => key;
const theme = "light" as const;

function renderSettings() {
  render(<ProjectSettings nav={nav} theme={theme} t={t} sse={{ messages: [] }} />);
}

async function saveRetention(value: string) {
  const input = screen.getByTestId("trash-retention-input");
  await act(async () => {
    await userEvent.clear(input);
    await userEvent.type(input, value);
  });
  await act(async () => {
    (screen.getByTestId("trash-retention-save")).click();
  });
}

afterEach(() => {
  cleanup();
  fetchJsonMock.mockReset();
  putApiMock.mockReset();
  fetchJsonMock.mockImplementation(async (path: string) => {
    if (path === "/project/trash-retention") return { retentionDays: 30 };
    return {};
  });
  putApiMock.mockImplementation(async () => ({}));
});
fetchJsonMock.mockImplementation(async (path: string) => {
  if (path === "/project/trash-retention") return { retentionDays: 30 };
  return {};
});

describe("ProjectSettings 回收站保留期卡", () => {
  it("读面回填当前值；合法值 PUT 精确整数并回读", async () => {
    renderSettings();
    const input = await screen.findByTestId("trash-retention-input");
    expect((input as HTMLInputElement).value).toBe("30");
    await saveRetention("45");
    expect(putApiMock).toHaveBeenCalledWith("/project/trash-retention", { retentionDays: 45 });
    // 成功后回读（notice 落成功文案）。
    expect(await screen.findByText("settings.saved")).toBeTruthy();
  });

  it("非法值本地拒绝、不发请求、错误落 notice", async () => {
    renderSettings();
    await screen.findByTestId("trash-retention-input");
    for (const bad of ["0", "-3", "40000", "abc"]) {
      putApiMock.mockClear();
      await saveRetention(bad);
      expect(putApiMock, bad).not.toHaveBeenCalled();
      expect(await screen.findByText(/保留天数须为/)).toBeTruthy();
    }
  });
});
