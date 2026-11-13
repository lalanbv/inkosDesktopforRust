//! BookDetail 设置表单草稿的 sessionStorage 持久化（1001 号）。
//!
//! ## 为什么需要（998 同族第三面，条件挂载域第二族）
//!
//! BookDetail 挂在 App 层 `view.page === "book-settings"` 条件渲染下：页面切换
//! 卸载组件，settings 表单的本地覆盖层 state（`settingsWordCount ?? book.*`）
//! 重置 null → 用户未保存的表单修改丢失（回落旧真源值，无任何提示）。996 同族
//! 第一面（dock 域筛选态）、998 第二面（dock 域编辑草稿）已修，本模块覆盖
//! App 层页面切换域的表单草稿。
//!
//! ## 会话语义
//!
//! 同 998 号 artifact-edit-draft：表单草稿是易失工作数据——sessionStorage 会话
//! 级（页面切换保留、应用重启丢弃）。键含 bookId，多书互不串扰。
//!
//! ## status 枚举防御（1008 号）
//!
//! status 用 core `BookStatusSchema` 校验：sessionStorage 被篡改 / 旧协议残留
//! 的非法值被逐字段过滤，不进表单、不污染真源。枚举真源=core，BookDetail
//! 曾因本地手打 union 漂移丢过 "incubating"。

// 1008 号：走 core subpath 导入——主入口会拖 node:async_hooks 链进浏览器
// bundle（rollup 拒绝）；subpath 只拉 models/book.js（纯 zod，浏览器安全）。
import { BookStatusSchema, type BookStatus } from "@actalk/inkos-core/models/book";

export interface BookSettingsDraft {
  readonly wordCount?: number;
  readonly targetChapters?: number;
  readonly status?: BookStatus;
}

const PREFIX = "inkos.bookSettingsDraft:";

export function bookSettingsDraftKey(bookId: string): string {
  return `${PREFIX}${bookId}`;
}

/** 读草稿；无存储 / 无草稿 / 字段类型错 / status 非法枚举值 → null（回落真源值）。 */
export function loadBookSettingsDraft(bookId: string): BookSettingsDraft | null {
  try {
    if (typeof sessionStorage === "undefined") return null;
    const raw = sessionStorage.getItem(bookSettingsDraftKey(bookId));
    if (raw === null) return null;
    const parsed = JSON.parse(raw) as Partial<BookSettingsDraft>;
    const draft: { wordCount?: number; targetChapters?: number; status?: BookStatus } = {};
    if (typeof parsed.wordCount === "number" && Number.isFinite(parsed.wordCount)) {
      draft.wordCount = parsed.wordCount;
    }
    if (typeof parsed.targetChapters === "number" && Number.isFinite(parsed.targetChapters)) {
      draft.targetChapters = parsed.targetChapters;
    }
    // 1008 号：枚举真源校验（非法值过滤，不进表单）
    const status = BookStatusSchema.safeParse(parsed.status);
    if (status.success) draft.status = status.data;
    return Object.keys(draft).length > 0 ? draft : null;
  } catch {
    return null;
  }
}

/** 写草稿（传入字段为当前已修改字段——未修改字段不写入，保持「覆盖层」语义）。 */
export function saveBookSettingsDraft(bookId: string, draft: BookSettingsDraft): void {
  try {
    if (typeof sessionStorage === "undefined") return;
    sessionStorage.setItem(bookSettingsDraftKey(bookId), JSON.stringify(draft));
  } catch {
    // 隐私模式 / 配额满：草稿回落会话内态，不影响主功能
  }
}

/** 清草稿（保存成功后——真源已更新，覆盖层回落 book.*）。 */
export function clearBookSettingsDraft(bookId: string): void {
  try {
    if (typeof sessionStorage === "undefined") return;
    sessionStorage.removeItem(bookSettingsDraftKey(bookId));
  } catch {
    // 同上
  }
}
