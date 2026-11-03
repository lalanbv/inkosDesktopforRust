// @vitest-environment jsdom
//! 874 号：axe 结构性规则防回归——把 872/873 号真浏览器审计的一次性成果
//! 转为持久守卫。锁定：StatusDot role=img（aria-prohibited-attr）、TabStrip
//! 导航地标与关闭钮名称、影游场景 textarea 名称（label, critical）、导入页
//! 目标书籍 select 名称（select-name, critical）。
//!
//! jsdom 无布局/计算色——color-contrast 无法在此判定，仍归真浏览器审计
//! SOP（注入 axe-core 逐页足时复测）；页面级地标/标题规则对孤立组件无意义，
//! 同样由真浏览器审计覆盖。必红自检夹具证明所依赖的规则在本配置下确实活着
//! （847/848 号门禁纪律：检测器须喂必红用例，防静默恒绿）。
//!
//! 875 号追加：WorkInspector 版本抽屉（关闭钮名称 button-name critical / 编辑
//! textarea 名称 label critical / 抽屉 aside 地标名称——landmark-unique 需侧栏
//! 并存才触发，此处直接断言 aria-label 在位）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { StoryGraphSchema } from "@actalk/inkos-core/interactive-film/graph-schema";
import { StatusBar } from "../components/StatusBar";
import { TabStrip } from "../components/TabStrip";
import { StoryGraphTree } from "../pages/StoryGraphTree";
import { ImportManager } from "../pages/ImportManager";
import { WorkInspector } from "../pages/WorkInspector";
import type { TFunction } from "../hooks/use-i18n";

const GRAPH = StoryGraphSchema.parse({
  schemaVersion: 1,
  projectId: "p1",
  title: "镜中歧路",
  variables: [],
  nodes: [
    { id: "n1", type: "start", sceneDesc: "苏檀坠入镜面深处。", choices: [] },
    { id: "n2", type: "normal", sceneDesc: "镜影提出交易。", choices: [] },
  ],
  endings: [],
});

const REVISION = {
  id: "r2",
  path: "artifacts/outline/revisions/r2.md",
  contentType: "text/markdown",
  status: "current",
  byteLength: 12,
  createdAt: "2026-11-03T00:00:00.000Z",
};

const API_DATA: Record<string, unknown> = {
  "/projects/p1/story-graph": GRAPH,
  "/books": { books: [{ id: "b1", title: "书 A" }] },
  "/works/w1": {
    work: {
      id: "w1",
      title: "雨夜碎镜",
      profileId: "short-fiction",
      language: "zh",
      status: "active",
      artifacts: [{ id: "outline", kind: "outline", currentRevisionId: "r2", revisions: [REVISION] }],
    },
    episodes: [],
  },
  "/profiles": { profiles: [] },
};

vi.mock("../hooks/use-api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../hooks/use-api")>()),
  useApi: (path: string) => ({
    data: API_DATA[path] ?? null,
    loading: false,
    error: null,
    refetch: vi.fn(),
    mutate: vi.fn(),
  }),
  fetchJson: async (path: string) => {
    if (path === "/works/w1/artifacts/outline/revisions/r2") {
      return { revision: REVISION, artifactId: "outline", content: "# 短篇大纲 v2\n" };
    }
    throw new Error(`unexpected fetchJson ${path}`);
  },
}));

const t = ((key: string) => key) as TFunction;
const nav = new Proxy({}, { get: () => () => {} }) as never;

const AXE_OPTIONS: axe.RunOptions = {
  rules: {
    "color-contrast": { enabled: false },
    region: { enabled: false },
    "page-has-heading-one": { enabled: false },
    "landmark-one-main": { enabled: false },
  },
};

async function violationsOf(root: Element): Promise<string[]> {
  const results = await axe.run(root, AXE_OPTIONS);
  return results.violations.map((v) => `${v.id}(${v.impact})x${v.nodes.length}`);
}

afterEach(() => {
  cleanup();
});

describe("a11y 结构性防回归（874）", () => {
  it("必红自检：已知违规夹具必须被检出（防 jsdom 下检测器静默恒绿）", async () => {
    const { container } = render(
      <div>
        <span className="dot" aria-label="状态" />
        <textarea />
        <select>
          <option value="">选择</option>
        </select>
        <button type="button" />
      </div>,
    );
    const ids = (await axe.run(container, AXE_OPTIONS)).violations.map((v) => v.id);
    expect(ids).toEqual(
      expect.arrayContaining(["aria-prohibited-attr", "label", "select-name", "button-name"]),
    );
  });

  it("StatusBar：状态点 role=img + 工作区切换器按钮均有名称", async () => {
    const { container } = render(
      <StatusBar
        sseConnected
        onReconnect={() => {}}
        daemonRunning
        onOpenBottomPanel={() => {}}
        workspaceActiveId="writing"
        onSwitchWorkspace={() => {}}
      />,
    );
    expect(await violationsOf(container)).toEqual([]);
  });

  it("TabStrip：导航地标 + 关闭按钮名称", async () => {
    const { container } = render(
      <TabStrip
        tabs={[
          { id: "book:b1#1", route: { page: "book", bookId: "b1" }, title: "书 A", pinned: false, preview: true },
          { id: "logs#2", route: { page: "logs" }, title: "日志", pinned: true, preview: false },
        ]}
        activeId="book:b1#1"
        onActivate={() => {}}
        onClose={() => {}}
        onCloseOthers={() => {}}
        onPin={() => {}}
      />,
    );
    expect(container.querySelector('[role="navigation"]')).not.toBeNull();
    expect(await violationsOf(container)).toEqual([]);
  });

  it("StoryGraphTree：场景 textarea 有可访问名称（873 critical）", async () => {
    const { container } = render(<StoryGraphTree projectId="p1" nav={nav} theme="light" t={t} />);
    expect(container.querySelectorAll("textarea")).toHaveLength(2);
    expect(await violationsOf(container)).toEqual([]);
  });

  it("ImportManager：目标书籍 select 有可访问名称（873 critical）", async () => {
    const { container } = render(<ImportManager nav={nav} theme="light" t={t} />);
    expect(container.querySelector("select")).not.toBeNull();
    expect(await violationsOf(container)).toEqual([]);
  });

  it("WorkInspector：版本抽屉关闭钮/编辑 textarea/抽屉地标均有名称（875 critical）", async () => {
    const user = userEvent.setup();
    const { container } = render(<WorkInspector workId="w1" onBack={() => {}} onChat={() => {}} />);
    const artifactButton = Array.from(container.querySelectorAll("button")).find((b) =>
      (b.textContent ?? "").includes("artifacts/outline/revisions/r2.md"),
    );
    expect(artifactButton).toBeDefined();
    await user.click(artifactButton!);
    await vi.waitFor(() => expect(container.querySelector("aside")).not.toBeNull());
    const drawer = container.querySelector("aside")!;
    expect(drawer.getAttribute("aria-label")).toBeTruthy();
    expect(await violationsOf(container)).toEqual([]);

    const editButton = Array.from(drawer.querySelectorAll("button")).find((b) => (b.textContent ?? "").includes("编辑"));
    await user.click(editButton!);
    await vi.waitFor(() => expect(drawer.querySelector("textarea")).not.toBeNull());
    expect(await violationsOf(container)).toEqual([]);
  });
});
