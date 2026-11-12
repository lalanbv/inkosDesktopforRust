//! ArtifactView 编辑草稿的 sessionStorage 持久化（998 号）。
//!
//! ## 为什么需要（996 同族第二面，数据丢失级）
//!
//! ArtifactView 挂在 ContextDock 的 `visible` 条件渲染下（ContextDock:96
//! `if (!visible) return null`）：dock 开合、面板切换（sidebarView 变更）、页面
//! 切换（App 层条件渲染）都会卸载重建组件——本地 useState 的
//! editing/editContent 重置 = **未保存输入丢失**。996 号已修同族的筛选态丢失
//! （localStorage 持久化），本模块把同形应用于编辑草稿。
//!
//! ## 会话语义（为何 sessionStorage 而非 localStorage）
//!
//! 编辑草稿是**易失工作数据**而非 UI 偏好：应用重启后恢复「编辑中」状态语义
//! 怪（文件可能已被外部修改），sessionStorage 的会话级生命周期恰好——
//! webview 会话内 dock 开合/切换保留，应用重启自然丢弃。键含 bookId+目标，
//! 多项目多文件互不串扰。

export interface ArtifactEditDraft {
  readonly editing: boolean;
  readonly content: string;
}

const EDIT_DRAFT_PREFIX = "inkos.artifactEdit:";

/** 草稿键：bookId + 目标（章节号或 truth 文件路径），多项目多文件互不串扰。 */
export function artifactEditDraftKey(
  bookId: string,
  isChapter: boolean,
  chapter: number | null,
  file: string | null,
): string {
  const target = isChapter ? `ch:${chapter}` : file ?? "";
  return `${EDIT_DRAFT_PREFIX}${bookId}:${target}`;
}

/** 读草稿；无存储 / 无草稿 / JSON 损坏 → null（回落非编辑态）。 */
export function loadArtifactEditDraft(key: string): ArtifactEditDraft | null {
  try {
    if (typeof sessionStorage === "undefined") return null;
    const raw = sessionStorage.getItem(key);
    if (raw === null) return null;
    const parsed = JSON.parse(raw) as Partial<ArtifactEditDraft>;
    if (typeof parsed.content !== "string" || typeof parsed.editing !== "boolean") return null;
    return { editing: parsed.editing, content: parsed.content };
  } catch {
    return null;
  }
}

/** 写草稿；隐私模式 / 配额满静默丢弃（草稿回落会话内态，不影响主功能）。 */
export function saveArtifactEditDraft(key: string, draft: ArtifactEditDraft): void {
  try {
    if (typeof sessionStorage === "undefined") return;
    sessionStorage.setItem(key, JSON.stringify(draft));
  } catch {
    // 同上
  }
}

/** 清草稿（保存成功 / 显式取消时）。 */
export function clearArtifactEditDraft(key: string): void {
  try {
    if (typeof sessionStorage === "undefined") return;
    sessionStorage.removeItem(key);
  } catch {
    // 同上
  }
}
