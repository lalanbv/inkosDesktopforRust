// @vitest-environment jsdom
//! 911 号：GenreManager 写面交互（CRUD+copy）——mount 冒烟只验渲染；本文件
//! 锁定写面契约形态：create body 的 chapterTypes/fatigueWords 逗号拆数组、
//! edit body 的 {profile:{...},body} 包裹形、DELETE 经 ConfirmDialog 确认、
//! copy 的 alert 回执（window.alert stub）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { GenreManager } from "./GenreManager";

const { fetchJsonMock, postApiMock, refetchMock } = vi.hoisted(() => ({
  fetchJsonMock: vi.fn(),
  postApiMock: vi.fn(),
  refetchMock: vi.fn(),
}));

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: RequestInit) => fetchJsonMock(path, init),
  postApi: (path: string, body?: unknown) => postApiMock(path, body),
  useApi: (url: string) => {
    if (url === "/genres") {
      return {
        data: { genres: [{ id: "g1", name: "古风", source: "project", language: "zh" }] },
        loading: false,
        error: null,
        refetch: refetchMock,
      };
    }
    if (url === "/genres/g1") {
      return {
        data: {
          profile: {
            name: "古风", id: "g1", language: "zh", chapterTypes: [], fatigueWords: [],
            numericalSystem: false, powerScaling: false, eraResearch: false, pacingRule: "",
          },
          body: "规则正文",
        },
        loading: false,
        error: null,
        refetch: refetchMock,
      };
    }
    return { data: undefined, loading: false, error: null, refetch: refetchMock };
  },
}));

vi.mock("../hooks/use-colors", () => ({ useColors: () => ({ link: "", btnPrimary: "", btnSecondary: "", btnDanger: "", cardStatic: "" }) }));
vi.mock("../hooks/use-i18n", () => ({ useI18n: () => ({ lang: "zh" }) }));

const t = (key: string) => key;
const alertSpy = vi.spyOn(window, "alert").mockImplementation(() => {});

beforeEach(() => {
  fetchJsonMock.mockReset();
  postApiMock.mockReset();
  refetchMock.mockReset();
  alertSpy.mockClear();
  fetchJsonMock.mockImplementation((path: string, init?: RequestInit) => {
    if (path === "/genres/g1" && (!init?.method || init.method === "GET")) {
      return Promise.resolve({
        profile: {
          name: "古风", id: "g1", language: "zh", chapterTypes: [], fatigueWords: [],
          numericalSystem: false, powerScaling: false, eraResearch: false, pacingRule: "",
        },
        body: "规则正文",
      });
    }
    if (init?.method === "PUT" || init?.method === "DELETE") return Promise.resolve({ ok: true });
    return Promise.reject(new Error(`unrouted fetch: ${init?.method ?? "GET"} ${path}`));
  });
  postApiMock.mockResolvedValue({ ok: true });
});

afterEach(() => cleanup());

async function selectG1() {
  render(<GenreManager nav={{ toDashboard: () => {} }} theme="dark" t={t} />);
  fireEvent.click(screen.getByRole("button", { name: /古风/ }));
  await waitFor(() => expect(screen.getByRole("button", { name: /common\.edit/ })).toBeTruthy());
}

function inputAfterLabel(labelText: string): HTMLInputElement {
  const label = screen.getAllByText(labelText).find((el) => el.tagName === "LABEL")!;
  return label.nextElementSibling as HTMLInputElement;
}

describe("GenreManager 写面交互（911 号）", () => {
  it("create：chapterTypes/fatigueWords 逗号拆数组入 POST /genres/create body", async () => {
    render(<GenreManager nav={{ toDashboard: () => {} }} theme="dark" t={t} />);
    fireEvent.click(screen.getByRole("button", { name: /genre\.createNew/ }));
    fireEvent.change(inputAfterLabel("ID"), { target: { value: "g9" } });
    fireEvent.change(inputAfterLabel("genre.name"), { target: { value: "新类型" } });
    fireEvent.change(inputAfterLabel("genre.chapterTypes (genre.commaSeparated)"), { target: { value: "开局, 冲突, 收束" } });
    // 表单内提交钮与页头新建钮同名，取第二个（表单内）。
    const submit = screen.getAllByRole("button", { name: /genre\.createNew/ })[1]!;
    fireEvent.click(submit);
    await waitFor(() => expect(refetchMock).toHaveBeenCalled());
    const create = postApiMock.mock.calls.find(([path]) => path === "/genres/create");
    expect(create).toBeTruthy();
    expect(create![1]).toMatchObject({
      id: "g9",
      name: "新类型",
      chapterTypes: ["开局", "冲突", "收束"], // 拆逗号+trim+滤空
      fatigueWords: [],
    });
    expect(alertSpy).not.toHaveBeenCalled();
  });

  it("edit：PUT /genres/:id body={profile:{...},body} 包裹形", async () => {
    await selectG1();
    fireEvent.click(screen.getByRole("button", { name: /common\.edit/ }));
    const nameBox = screen.getByDisplayValue("古风") as HTMLInputElement;
    fireEvent.change(nameBox, { target: { value: "古风（修订）" } });
    fireEvent.click(screen.getByRole("button", { name: /genre\.saveChanges/ }));
    await waitFor(() => expect(refetchMock).toHaveBeenCalled());
    const put = fetchJsonMock.mock.calls.find(([path, init]) => path === "/genres/g1" && init?.method === "PUT");
    expect(put).toBeTruthy();
    const body = JSON.parse(String((put![1] as RequestInit).body));
    // 包裹形：表单字段进 profile 子对象，正文独立同级。
    expect(body.profile).toMatchObject({ id: "g1", name: "古风（修订）", chapterTypes: [] });
    expect(body.body).toBe("规则正文");
  });

  it("delete：ConfirmDialog 确认后 DELETE 并清选", async () => {
    await selectG1();
    fireEvent.click(screen.getAllByRole("button", { name: /common\.delete/ })[0]!); // 详情面板删除钮
    fireEvent.click(screen.getAllByRole("button", { name: /common\.delete/ }).at(-1)!); // 对话框确认钮同名，取最后
    await waitFor(() => expect(refetchMock).toHaveBeenCalled());
    const del = fetchJsonMock.mock.calls.find(([path, init]) => path === "/genres/g1" && init?.method === "DELETE");
    expect(del).toBeTruthy();
  });

  it("copy：POST /genres/:id/copy + alert 回执", async () => {
    await selectG1();
    fireEvent.click(screen.getByRole("button", { name: /genre\.copyToProject/ }));
    await waitFor(() => expect(alertSpy).toHaveBeenCalledWith("Copied g1 to project genres/"));
    expect(postApiMock.mock.calls.some(([p]) => p === "/genres/g1/copy")).toBe(true);
  });
});
