import { create } from "zustand";
import { isWorkspaceId, workspaceDef } from "@/lib/workspaces";
import type { WorkspaceId } from "@/lib/workspaces";
import type { WorkspacesSnapshot, WorkspacesStore, WorkspaceSnapshot } from "./types";

// Same storage convention as the tabs snapshot (`inkos:studio:tabs`).
export const WORKSPACES_STORAGE_KEY = "inkos:studio:workspaces";

interface WorkspacesStorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function getWorkspacesStorage(): WorkspacesStorageLike | null {
  if (typeof window === "undefined") {
    return null;
  }
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** 启动默认：未激活工作区、无快照、侧面板展开（与旧 useState(true) 行为一致）。 */
export const EMPTY_WORKSPACES: WorkspacesSnapshot = {
  activeId: null,
  snapshots: {},
  sidePanelVisible: true,
};

/** 单个快照的合法性清洗：非布尔字段整体丢弃（回退该工作区默认值）。 */
function sanitizeSnapshot(raw: unknown): WorkspaceSnapshot | null {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return null;
  const sidePanelVisible = (raw as { sidePanelVisible?: unknown }).sidePanelVisible;
  return typeof sidePanelVisible === "boolean" ? { sidePanelVisible } : null;
}

/** 持久化读取（纯函数，可测）：坏 JSON/坏 id/坏快照逐项静默降级。 */
export function readStoredWorkspaces(
  storage: Pick<WorkspacesStorageLike, "getItem"> | null | undefined,
): WorkspacesSnapshot {
  let raw: string | null = null;
  try {
    raw = storage?.getItem(WORKSPACES_STORAGE_KEY) ?? null;
  } catch {
    return EMPTY_WORKSPACES;
  }
  if (!raw) return EMPTY_WORKSPACES;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
      return EMPTY_WORKSPACES;
    }
    const body = parsed as Record<string, unknown>;
    const out: WorkspacesSnapshot = { ...EMPTY_WORKSPACES, snapshots: {} };
    if (typeof body.activeId === "string" && isWorkspaceId(body.activeId)) {
      out.activeId = body.activeId;
    }
    if (typeof body.sidePanelVisible === "boolean") {
      out.sidePanelVisible = body.sidePanelVisible;
    }
    if (typeof body.snapshots === "object" && body.snapshots !== null && !Array.isArray(body.snapshots)) {
      for (const [id, value] of Object.entries(body.snapshots as Record<string, unknown>)) {
        if (!isWorkspaceId(id)) continue;
        const snapshot = sanitizeSnapshot(value);
        if (snapshot) out.snapshots[id] = snapshot;
      }
    }
    return out;
  } catch {
    return EMPTY_WORKSPACES;
  }
}

function persist(snapshot: WorkspacesSnapshot): void {
  try {
    getWorkspacesStorage()?.setItem(WORKSPACES_STORAGE_KEY, JSON.stringify(snapshot));
  } catch {
    // 私密模式等：会话内生效即可（与 tabs/theme 同款策略）
  }
}

export const useWorkspacesStore = create<WorkspacesStore>()((set, get) => ({
  ...readStoredWorkspaces(getWorkspacesStorage()),

  switchTo: (next) => {
    const state = get();
    // 同 id 重复切换 = 「回到该工作区主页」：状态零改动（不捕获也不应用，
    // 否则会把切入后用户手动 Cmd+B 的状态回写/重置掉），导航由调用方执行。
    if (state.activeId === next) return;
    const snapshots = { ...state.snapshots };
    // 切出捕获：把当前侧面板开合写进原工作区快照（首次从未激活时不捕获——
    // 切换前的布局不属于任何工作区）。
    if (state.activeId) {
      snapshots[state.activeId] = { sidePanelVisible: state.sidePanelVisible };
    }
    const captured = snapshots[next];
    const sidePanelVisible = captured
      ? captured.sidePanelVisible
      : workspaceDef(next).defaultSidePanelVisible;
    const nextSnapshot: WorkspacesSnapshot = { activeId: next, snapshots, sidePanelVisible };
    persist(nextSnapshot);
    set(nextSnapshot);
  },

  setSidePanelVisible: (visible) => {
    const nextSnapshot: WorkspacesSnapshot = {
      activeId: get().activeId,
      snapshots: get().snapshots,
      sidePanelVisible: visible,
    };
    persist(nextSnapshot);
    set({ sidePanelVisible: visible });
  },
}));
