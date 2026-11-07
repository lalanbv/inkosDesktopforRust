// @vitest-environment jsdom
//! 906 号：WorkInspector 交互面补强——mount 冒烟（page-mount-smoke-2）只验渲染，
//! 版本工作台的写面交互（adopt 请求体 expectedCurrentRevisionId 语义/编辑钮
//! disabled 判定=防改历史/compare diff 行序/错误呈现/episode 弹层）在此锁定。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { WorkInspector } from "./WorkInspector";

const { fetchJsonMock, refetchMock } = vi.hoisted(() => ({
  fetchJsonMock: vi.fn(),
  refetchMock: vi.fn(),
}));

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: RequestInit) => fetchJsonMock(path, init),
  useApi: (url: string) => {
    if (url.startsWith("/works/")) {
      return {
        data: {
          work: {
            id: "w1",
            title: "镜花水月",
            profileId: "short-story",
            language: "zh",
            status: "published",
            artifacts: [
              {
                id: "a1",
                kind: "story-manuscript",
                currentRevisionId: "r1",
                revisions: [
                  { id: "r1", path: "works/w1/story.md", contentType: "text/markdown", status: "current", byteLength: 2, createdAt: "2026-11-08T00:00:00Z" },
                  { id: "r2", path: "works/w1/story.md", contentType: "text/markdown", status: "superseded", byteLength: 2, createdAt: "2026-11-07T00:00:00Z" },
                ],
              },
            ],
          },
          episodes: [{ id: "ep1", status: "completed", startedAt: "2026-11-08T01:00:00Z" }],
        },
        loading: false,
        error: null,
        refetch: refetchMock,
      };
    }
    return { data: undefined, loading: false, error: null, refetch: refetchMock };
  },
}));

vi.mock("./CreativeMethodsEditor", () => ({ CreativeMethodsEditor: () => <div data-testid="methods-stub" /> }));

const REVISION_BODY: Record<string, string> = {
  r1: "第一行 当前版\n第二行",
  r2: "第一行 所选版\n第二行",
};

function revisionPayload(id: string) {
  return { revision: { id, path: "works/w1/story.md", contentType: "text/markdown", status: id === "r1" ? "current" : "superseded", byteLength: 2, createdAt: "2026-11-08T00:00:00Z" }, artifactId: "a1", content: REVISION_BODY[id] };
}

beforeEach(() => {
  fetchJsonMock.mockReset();
  refetchMock.mockReset();
  fetchJsonMock.mockImplementation((path: string, init?: RequestInit) => {
    if (path.includes("/revisions/r1") && (!init?.method || init.method === "GET")) return Promise.resolve(revisionPayload("r1"));
    if (path.includes("/revisions/r2")) return Promise.resolve(revisionPayload("r2"));
    return Promise.reject(new Error(`unrouted fetch: ${init?.method ?? "GET"} ${path}`));
  });
});

afterEach(() => cleanup());

async function openInspectorAndSelectSuperseded() {
  render(<WorkInspector workId="w1" onBack={() => {}} onChat={() => {}} />);
  // 打开当前版本 r1（生成物卡片）。
  fireEvent.click(screen.getByRole("button", { name: /story\.md/ }));
  await waitFor(() => expect(screen.getByText((_, el) => el?.tagName === "PRE" && (el.textContent ?? "").includes("第一行 当前版"))).toBeTruthy());
  // 版本切换 select（aria-label 875 号）切到 r2。
  const select = screen.getByLabelText(/选择作品版本|Select revision/) as HTMLSelectElement;
  fireEvent.change(select, { target: { value: "r2" } });
  await waitFor(() => expect(screen.getByText((_, el) => el?.tagName === "PRE" && (el.textContent ?? "").includes("第一行 所选版"))).toBeTruthy());
}

describe("WorkInspector 交互面（906 号）", () => {
  it("打开非当前版本 → adopt 携带 expectedCurrentRevisionId 且成功后清选+refetch", async () => {
    await openInspectorAndSelectSuperseded();
    fetchJsonMock.mockImplementation((path: string, init?: RequestInit) => {
      if (init?.method === "POST" && path.endsWith("/adopt")) return Promise.resolve({ ok: true });
      if (path.includes("/revisions/")) return Promise.resolve(revisionPayload(path.includes("r2") ? "r2" : "r1"));
      return Promise.reject(new Error(`unrouted: ${path}`));
    });
    // adopt 钮渲染门槛=comparison 在场（组件有意要求先比较再采用）。
    fireEvent.click(screen.getByRole("button", { name: /比较当前版本|Compare with current/ }));
    await waitFor(() => expect(screen.getByText((_, el) => el?.tagName === "PRE" && (el.textContent ?? "").includes("− 第一行 当前版"))).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /采用这个版本|Adopt this revision/ }));
    await waitFor(() => expect(refetchMock).toHaveBeenCalled());
    const adoptCall = fetchJsonMock.mock.calls.find(([path, init]) => String(path).endsWith("/adopt") && init?.method === "POST");
    expect(adoptCall).toBeTruthy();
    const body = JSON.parse(String((adoptCall![1] as RequestInit).body));
    // adopt 语义核心：expectedCurrentRevisionId 必须是组件当前认知的 current（r1），
    // 而非被采用版本（r2）——冲突检测的锚。
    expect(body).toEqual({ expectedCurrentRevisionId: "r1" });
    // 成功后抽屉收起（所选内容消失）。
    expect(screen.queryByText((_, el) => el?.tagName === "PRE" && (el.textContent ?? "").includes("第一行 所选版"))).toBeNull();
  });

  it("非当前版本编辑钮 disabled（防改历史）；当前版本可编辑且保存走 project/artifacts PUT", async () => {
    await openInspectorAndSelectSuperseded();
    const editSuperseded = screen.getByRole("button", { name: /编辑|Edit/ }) as HTMLButtonElement;
    expect(editSuperseded.disabled).toBe(true);
    // 切回当前版本 r1 → 编辑启用 → 保存。
    fetchJsonMock.mockImplementation((path: string, init?: RequestInit) => {
      if (init?.method === "PUT" && path.includes("/project/artifacts/")) return Promise.resolve({ ok: true });
      if (path.includes("/revisions/")) return Promise.resolve(revisionPayload(path.includes("r2") ? "r2" : "r1"));
      return Promise.reject(new Error(`unrouted: ${path}`));
    });
    const select = screen.getByLabelText(/选择作品版本|Select revision/) as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "r1" } });
    await waitFor(() => expect(screen.getByText((_, el) => el?.tagName === "PRE" && (el.textContent ?? "").includes("第一行 当前版"))).toBeTruthy());
    const editCurrent = screen.getByRole("button", { name: /编辑|Edit/ }) as HTMLButtonElement;
    expect(editCurrent.disabled).toBe(false);
    fireEvent.click(editCurrent);
    const textarea = screen.getByLabelText(/编辑版本内容|Edit revision content/) as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "手工修订稿" } });
    fireEvent.click(screen.getByRole("button", { name: /保存新版本|Save revision/ }));
    await waitFor(() => expect(refetchMock).toHaveBeenCalled());
    const saveCall = fetchJsonMock.mock.calls.find(([path, init]) => String(path).includes("/project/artifacts/") && init?.method === "PUT");
    expect(saveCall).toBeTruthy();
    expect(String(saveCall![0])).toBe("/project/artifacts/works/w1/works/w1/story.md");
    const saveBody = JSON.parse(String((saveCall![1] as RequestInit).body));
    expect(saveBody).toEqual({ content: "手工修订稿", expectedRevisionId: "r1" });
  });

  it("比较当前版本 → 差异区 − 当前/+ 所选 行序正确", async () => {
    await openInspectorAndSelectSuperseded();
    fireEvent.click(screen.getByRole("button", { name: /比较当前版本|Compare with current/ }));
    await waitFor(() => expect(screen.getByText(/变更内容|Changes/)).toBeTruthy());
    const diff = screen.getByText((_, element) => element?.tagName === "PRE" && (element.textContent ?? "").includes("− 第一行 当前版")) as HTMLElement;
    expect(diff.textContent).toBe("− 第一行 当前版\n+ 第一行 所选版");
  });

  it("fetch 失败 → role=alert 错误呈现且抽屉保留", async () => {
    render(<WorkInspector workId="w1" onBack={() => {}} onChat={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: /story\.md/ }));
    // 首次 fetch 已由 beforeEach 路由成功；此处用 unrouted 的 select 值触发失败。
    await waitFor(() => expect(screen.getByLabelText(/选择作品版本|Select revision/)).toBeTruthy());
    const select = screen.getByLabelText(/选择作品版本|Select revision/) as HTMLSelectElement;
    fetchJsonMock.mockImplementationOnce(() => Promise.reject(new Error("engine offline")));
    fireEvent.change(select, { target: { value: "r2" } });
    await waitFor(() => expect(screen.getAllByRole("alert").length).toBeGreaterThan(0));
    expect(screen.getAllByRole("alert")[0]!.textContent).toContain("engine offline");
  });

  it("episode 点击 → 事件弹层渲染 seq·type 并可关闭", async () => {
    fetchJsonMock.mockImplementation((path: string) => {
      if (path.startsWith("/episodes/")) return Promise.resolve({ events: [{ seq: 1, type: "episode.started", payload: { ok: true } }] });
      if (path.includes("/revisions/")) return Promise.resolve(revisionPayload("r1"));
      return Promise.reject(new Error(`unrouted: ${path}`));
    });
    render(<WorkInspector workId="w1" onBack={() => {}} onChat={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: /ep1/ }));
    await waitFor(() => expect(screen.getByText("1 · episode.started")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /关闭执行记录|Close execution history/ }));
    expect(screen.queryByText("1 · episode.started")).toBeNull();
  });
});
