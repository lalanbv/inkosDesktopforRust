import type { WorkspaceId } from "@/lib/workspaces";

/** 一个工作区记住的布局快照（869 号：当前范围 = 侧面板开合）。 */
export interface WorkspaceSnapshot {
  sidePanelVisible: boolean;
}

export interface WorkspacesSnapshot {
  /** 当前激活的工作区；null = 从未切换过（无捕获、无快照语义）。 */
  activeId: WorkspaceId | null;
  /** 各工作区最近一次切出时的布局捕获。 */
  snapshots: Partial<Record<WorkspaceId, WorkspaceSnapshot>>;
  /** 全局侧面板开合（Cmd+B 的当前值，启动恢复用；切换工作区时按快照改写）。 */
  sidePanelVisible: boolean;
}

export interface WorkspacesStore extends WorkspacesSnapshot {
  /**
   * 切换工作区：切出侧捕获当前侧面板开合进原工作区快照，
   * 切入侧应用其快照（无捕获时用该工作区默认值）。
   * 同 id 重复切换 = 状态零改动（不捕获不应用，防回写/重置用户手动调整；
   * 导航回主页由调用方执行）。
   */
  switchTo: (next: WorkspaceId) => void;

  /** 全局侧面板开合（Cmd+B）：写当前值，下次启动恢复。 */
  setSidePanelVisible: (visible: boolean) => void;
}
