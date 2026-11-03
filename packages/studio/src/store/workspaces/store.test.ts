import { describe, it, expect, afterEach, vi } from "vitest";
import { readStoredWorkspaces, EMPTY_WORKSPACES, WORKSPACES_STORAGE_KEY } from "./store";
import { useWorkspacesStore } from "./index";

function fakeStorage(entries: Record<string, string>) {
  return {
    getItem: (key: string) => (key in entries ? entries[key] : null),
    setItem: (key: string, value: string) => {
      entries[key] = value;
    },
  };
}

const VALID_STORE = JSON.stringify({
  activeId: "writing",
  sidePanelVisible: false,
  snapshots: { analysis: { sidePanelVisible: false }, bogus: { sidePanelVisible: true } },
});

describe("readStoredWorkspaces", () => {
  it("returns the empty snapshot for missing storage or missing key", () => {
    expect(readStoredWorkspaces(null)).toEqual(EMPTY_WORKSPACES);
    expect(readStoredWorkspaces(undefined)).toEqual(EMPTY_WORKSPACES);
    expect(readStoredWorkspaces(fakeStorage({}))).toEqual(EMPTY_WORKSPACES);
  });

  it("degrades silently on bad JSON or wrong shapes (tabs 同款约定)", () => {
    expect(readStoredWorkspaces(fakeStorage({ [WORKSPACES_STORAGE_KEY]: "{not json" }))).toEqual(EMPTY_WORKSPACES);
    expect(readStoredWorkspaces(fakeStorage({ [WORKSPACES_STORAGE_KEY]: "42" }))).toEqual(EMPTY_WORKSPACES);
    expect(readStoredWorkspaces(fakeStorage({ [WORKSPACES_STORAGE_KEY]: '["writing"]' }))).toEqual(EMPTY_WORKSPACES);
  });

  it("restores a valid snapshot and drops unknown workspace ids per-key", () => {
    const snapshot = readStoredWorkspaces(fakeStorage({ [WORKSPACES_STORAGE_KEY]: VALID_STORE }));
    expect(snapshot.activeId).toBe("writing");
    expect(snapshot.sidePanelVisible).toBe(false);
    expect(snapshot.snapshots.analysis).toEqual({ sidePanelVisible: false });
    // 未知 id（bogus）被逐键丢弃，不进任何已知工作区的快照
    expect(Object.keys(snapshot.snapshots)).toEqual(["analysis"]);
  });

  it("drops snapshots whose fields are not booleans", () => {
    const raw = JSON.stringify({ snapshots: { writing: { sidePanelVisible: "yes" } } });
    const snapshot = readStoredWorkspaces(fakeStorage({ [WORKSPACES_STORAGE_KEY]: raw }));
    expect(snapshot.snapshots.writing).toBeUndefined();
  });
});

describe("useWorkspacesStore.switchTo", () => {
  it("first switch captures nothing and applies the target's default side panel", () => {
    useWorkspacesStore.setState({ ...EMPTY_WORKSPACES, snapshots: {} });
    useWorkspacesStore.getState().switchTo("analysis");
    const state = useWorkspacesStore.getState();
    expect(state.activeId).toBe("analysis");
    expect(state.sidePanelVisible).toBe(false); // analysis 默认收起侧面板
    expect(state.snapshots).toEqual({});
  });

  it("switching away captures the current layout, switching back restores it", () => {
    useWorkspacesStore.setState({ ...EMPTY_WORKSPACES, snapshots: {} });
    // 进入写作（默认侧面板展开）→ 用户 Cmd+B 收起 → 切去分析 → 切回写作
    useWorkspacesStore.getState().switchTo("writing");
    useWorkspacesStore.getState().setSidePanelVisible(false);
    useWorkspacesStore.getState().switchTo("analysis");
    expect(useWorkspacesStore.getState().sidePanelVisible).toBe(false); // analysis 无快照，默认 false
    useWorkspacesStore.getState().switchTo("writing");
    const state = useWorkspacesStore.getState();
    expect(state.activeId).toBe("writing");
    expect(state.sidePanelVisible).toBe(false); // 写作 remembered 用户收起
    // 分析工作区也被刚才的切出捕获（此刻的 analysis 值 = false）
    expect(state.snapshots.analysis).toEqual({ sidePanelVisible: false });
  });

  it("each workspace remembers its own layout independently", () => {
    useWorkspacesStore.setState({ ...EMPTY_WORKSPACES, snapshots: {} });
    useWorkspacesStore.getState().switchTo("writing"); // 展开
    useWorkspacesStore.getState().setSidePanelVisible(false);
    useWorkspacesStore.getState().switchTo("review"); // 展开（review 默认 true，无快照）
    expect(useWorkspacesStore.getState().sidePanelVisible).toBe(true);
    useWorkspacesStore.getState().setSidePanelVisible(false);
    useWorkspacesStore.getState().switchTo("film");
    useWorkspacesStore.getState().switchTo("review");
    expect(useWorkspacesStore.getState().sidePanelVisible).toBe(false); // review 记住了用户的收起
    useWorkspacesStore.getState().switchTo("writing");
    expect(useWorkspacesStore.getState().sidePanelVisible).toBe(false); // writing 也记住了自己的
  });

  it("same-id switch does not self-capture (replay instead of echo)", () => {
    useWorkspacesStore.setState({ ...EMPTY_WORKSPACES, snapshots: {} });
    useWorkspacesStore.getState().switchTo("writing");
    useWorkspacesStore.getState().setSidePanelVisible(false);
    useWorkspacesStore.getState().switchTo("writing");
    const state = useWorkspacesStore.getState();
    expect(state.sidePanelVisible).toBe(false); // 重放自身值
    expect(Object.keys(state.snapshots)).not.toContain("writing"); // 未把当前值回写为快照
  });
});

describe("useWorkspacesStore persistence round-trip", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("switchTo writes a snapshot that readStoredWorkspaces restores exactly", () => {
    // stubGlobal 注入 window.localStorage：store 的 persist 在无 window 环境
    // 静默跳过（tabs 同款限制），这里用假存储做真实写路径验证。
    const storage = fakeStorage({});
    vi.stubGlobal("window", { localStorage: storage });
    useWorkspacesStore.setState({ ...EMPTY_WORKSPACES, snapshots: {} });
    useWorkspacesStore.getState().switchTo("writing");
    useWorkspacesStore.getState().setSidePanelVisible(false);
    useWorkspacesStore.getState().switchTo("review");
    const stored = readStoredWorkspaces(storage);
    expect(stored.activeId).toBe("review");
    expect(stored.sidePanelVisible).toBe(true);
    expect(stored.snapshots.writing).toEqual({ sidePanelVisible: false });
  });
});
