import type { HashRoute } from "@/hooks/use-hash-route";
import type { NavSectionId } from "@/lib/nav-sections";
import type { RecentEntry } from "@/store/recents";

/**
 * 工作区预设（十二原则 #8，869 号）：写作/审阅/分析/互动影视四套一键切换的
 * 模式布局，类 Blender/Godot 工作区语义——
 * - 切出时把当前侧面板开合捕获进原工作区快照（「各自记住」）；
 * - 切入时应用目标工作区快照（首次用默认值）并跳转该工作区的规范目的地。
 *
 * 范围裁定（与既有记忆机制的关系，防双真源）：
 * - dock/底部面板开合已由 P3-4 按页记忆（usePerPageVisibility）承担，
 *   本系统**不**再存一份——切换写入会形成第二真源；
 * - 面板宽度由 usePanelWidth 全局键记忆（唯一可见宽度面板=dock 且仅写作
 *   工作区可见，按工作区分键无实际收益），维持全局共享；
 * - 本系统的记忆增量 = 侧面板（Cmd+B）按工作区 + 规范目的地跳转。
 */

export type WorkspaceId = "writing" | "review" | "analysis" | "film";

export interface WorkspaceDef {
  id: WorkspaceId;
  titleZh: string;
  titleEn: string;
  /** lucide 图标名（渲染层 StatusBar/CommandPalette 映射）。 */
  icon: string;
  /** 切换时活动栏同步选中的区。 */
  section: NavSectionId;
  /** 侧面板默认开合（该工作区首次切入、尚无捕获快照时生效）。 */
  defaultSidePanelVisible: boolean;
}

export const WORKSPACE_IDS: ReadonlyArray<WorkspaceId> = [
  "writing",
  "review",
  "analysis",
  "film",
];

export const WORKSPACE_DEFS: ReadonlyArray<WorkspaceDef> = [
  {
    id: "writing",
    titleZh: "写作",
    titleEn: "Writing",
    icon: "feather",
    section: "create",
    defaultSidePanelVisible: true,
  },
  {
    id: "review",
    titleZh: "审阅",
    titleEn: "Review",
    icon: "book-open",
    section: "create",
    defaultSidePanelVisible: true,
  },
  {
    id: "analysis",
    titleZh: "分析",
    titleEn: "Analysis",
    icon: "trending-up",
    section: "tools",
    defaultSidePanelVisible: false,
  },
  {
    id: "film",
    titleZh: "互动影视",
    titleEn: "Interactive Film",
    icon: "film",
    section: "film",
    defaultSidePanelVisible: true,
  },
];

export function isWorkspaceId(value: string): value is WorkspaceId {
  return (WORKSPACE_IDS as ReadonlyArray<string>).includes(value);
}

export function workspaceDef(id: WorkspaceId): WorkspaceDef {
  return WORKSPACE_DEFS.find((def) => def.id === id) ?? WORKSPACE_DEFS[0];
}

/**
 * 规范目的地解析（纯函数，可测）：从「最近访问」推导该工作区的落点，
 * 推导不出返回 null（调用方回落首页，不臆造数据）。
 * recents 数组序 = 最新在前（applyRecentPush 去重置顶）。
 */
export function resolveWorkspaceDestination(
  id: WorkspaceId,
  recents: ReadonlyArray<RecentEntry>,
): HashRoute | null {
  switch (id) {
    case "writing": {
      // 最近的书页直落；退而求其次取任一带 bookId 的记录（章节/设定/分析等）。
      const bookId =
        recents.find((entry) => entry.page === "book" && entry.bookId)?.bookId ??
        recents.find((entry) => entry.bookId)?.bookId;
      return bookId ? { page: "book", bookId } : null;
    }
    case "review": {
      const bookId = recents.find((entry) => entry.bookId)?.bookId;
      return bookId ? { page: "book-timeline", bookId } : null;
    }
    case "analysis":
      // 雷达不依赖书籍参数，永远可达。
      return { page: "radar" };
    case "film": {
      const projectId = recents.find((entry) => entry.projectId)?.projectId;
      return projectId ? { page: "film-studio", projectId } : null;
    }
  }
}
