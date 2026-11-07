// @vitest-environment jsdom
//! 910 号：CreativeMethodsEditor 交互面（885 引擎 M2 写面的 UI 消费组件）——
//! 三写流锁定：PUT /profiles/:id（原样 JSON 体）、POST /profiles（新类型）、
//! PUT /skills/:id/documents（documents 全量数组体）；+错误呈现（role=status）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { CreativeMethodsEditor } from "./CreativeMethodsEditor";

const { fetchJsonMock } = vi.hoisted(() => ({ fetchJsonMock: vi.fn() }));

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: RequestInit) => fetchJsonMock(path, init),
  useApi: () => ({
    data: {
      profiles: [
        {
          id: "short-story",
          title: "短篇",
          requiredSkillIds: ["sk-outline"],
          recommendedSkillIds: ["sk-style", "sk-outline"],
          version: 2,
        },
      ],
    },
    loading: false,
    error: null,
    refetch: vi.fn(),
  }),
}));

beforeEach(() => {
  fetchJsonMock.mockReset();
  fetchJsonMock.mockImplementation((path: string, init?: RequestInit) => {
    if (path === "/skills/sk-outline/documents" && (!init?.method || init.method === "GET")) {
      return Promise.resolve({
        documents: [
          { path: "SKILL.md", content: "大纲技能正文" },
          { path: "refs/tonality.md", content: "语气参考" },
        ],
      });
    }
    if (init?.method === "PUT" || init?.method === "POST") return Promise.resolve({ ok: true });
    return Promise.reject(new Error(`unrouted fetch: ${init?.method ?? "GET"} ${path}`));
  });
});

afterEach(() => cleanup());

describe("CreativeMethodsEditor 写面交互（910 号）", () => {
  it("编辑作品配置 → PUT /profiles/:id 原样 JSON 体 + 保存回执", async () => {
    render(<CreativeMethodsEditor profileId="short-story" />);
    fireEvent.click(screen.getByRole("button", { name: /编辑作品配置|Edit profile/ }));
    const textarea = screen.getByLabelText(/作品配置|Profile configuration/) as HTMLTextAreaElement;
    const edited = textarea.value.replace('"short-story"', '"short-story-v2"');
    fireEvent.change(textarea, { target: { value: edited } });
    fireEvent.click(screen.getByRole("button", { name: /保存作品配置|Save profile/ }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("作品配置已保存"));
    const put = fetchJsonMock.mock.calls.find(([path, init]) => path === "/profiles/short-story" && init?.method === "PUT");
    expect(put).toBeTruthy();
    // PUT 体=textarea 原文（原样 JSON 字符串，组件不重排字段）。
    expect((put![1] as RequestInit).body).toBe(edited);
  });

  it("保存为新类型 → POST /profiles 原样体 + 新类型回执", async () => {
    render(<CreativeMethodsEditor profileId="short-story" />);
    fireEvent.click(screen.getByRole("button", { name: /编辑作品配置|Edit profile/ }));
    const textarea = screen.getByLabelText(/作品配置|Profile configuration/) as HTMLTextAreaElement;
    const edited = textarea.value.replace('"short-story"', '"fanfic-new"');
    fireEvent.change(textarea, { target: { value: edited } });
    fireEvent.click(screen.getByRole("button", { name: /保存为新类型|Save as new profile/ }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("新类型已保存"));
    const post = fetchJsonMock.mock.calls.find(([path, init]) => path === "/profiles" && init?.method === "POST");
    expect(post).toBeTruthy();
    expect((post![1] as RequestInit).body).toBe(edited);
  });

  it("skill 文档打开 → 编辑 → PUT /skills/:id/documents 全量数组体", async () => {
    render(<CreativeMethodsEditor profileId="short-story" />);
    fireEvent.click(screen.getByRole("button", { name: "sk-outline" }));
    await waitFor(() => expect((screen.getByLabelText(/方法正文|Method text/) as HTMLTextAreaElement).value).toBe("大纲技能正文"));
    // 文档切换（SKILL.md → refs/tonality.md）。
    fireEvent.change(screen.getByLabelText(/方法文档|Method document/), { target: { value: "refs/tonality.md" } });
    const editor = screen.getByLabelText(/方法正文|Method text/) as HTMLTextAreaElement;
    expect(editor.value).toBe("语气参考");
    fireEvent.change(editor, { target: { value: "语气参考（修订）" } });
    fireEvent.click(screen.getByRole("button", { name: /保存项目方法|Save project method/ }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("项目方法与参考文档已保存"));
    const put = fetchJsonMock.mock.calls.find(([path, init]) => path === "/skills/sk-outline/documents" && init?.method === "PUT");
    expect(put).toBeTruthy();
    const body = JSON.parse(String((put![1] as RequestInit).body));
    // PUT 体=documents 全量数组（含未编辑文档原样+已编辑文档新值）。
    expect(body.documents).toEqual([
      { path: "SKILL.md", content: "大纲技能正文" },
      { path: "refs/tonality.md", content: "语气参考（修订）" },
    ]);
  });

  it("写面失败 → role=status 错误呈现（perform catch 形态）", async () => {
    fetchJsonMock.mockImplementation((path: string, init?: RequestInit) => {
      if (init?.method === "PUT" && path === "/profiles/short-story") return Promise.reject(new Error("profile registry conflict"));
      if (path === "/skills/sk-outline/documents" && (!init?.method || init.method === "GET")) {
        return Promise.resolve({ documents: [{ path: "SKILL.md", content: "x" }] });
      }
      return Promise.reject(new Error(`unrouted: ${path}`));
    });
    render(<CreativeMethodsEditor profileId="short-story" />);
    fireEvent.click(screen.getByRole("button", { name: /编辑作品配置|Edit profile/ }));
    fireEvent.click(screen.getByRole("button", { name: /保存作品配置|Save profile/ }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("profile registry conflict"));
  });
});
