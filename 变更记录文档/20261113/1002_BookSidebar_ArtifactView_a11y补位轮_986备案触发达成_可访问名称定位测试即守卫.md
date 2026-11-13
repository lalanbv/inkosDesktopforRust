# 1002 号：BookSidebar/ArtifactView a11y 补位轮——986 备案触发条件达成（新交互面落地后守卫补位）

日期：2026-11-13。开工三查净（HEAD=73d26839、工作区空、无并行会话）。

## ① 触发条件达成：996-1001 五轮新增/触碰交互面的 a11y 核查

986 备案「未守卫新交互面如后续走查发现 critical 再扩」。996-1001 五轮触碰的 BookSidebar/ArtifactView 域逐个核查 a11y 形态，命中 873 同族 critical 漏网 5 处：

- ArtifactView 编辑 textarea **无可访问名称**（998 轮集成测试只能靠 `.lucide-pencil` 类选择器定位按钮即侧面实证）；
- 返回/编辑/保存/取消四枚图标钮全部无可访问名称（纯图标无文本）。

## ② 修复：873/986 惯用形 aria-label + 字典注册 + 测试升级为守卫

- 四钮 + textarea 补 `aria-label={t(...)}`（该文件惯例 `t` 而非 `tr`——ArtifactView 原无 t 定义，接线 `useI18n()` hook）；字典注册 4 key（`book.backToFileList` / `book.editFileContent` / `book.saveChanges` / `book.cancelEditing`，双语）。
- **集成测试同步升级为 a11y 回归守卫**：`iconButton` helper 从 icon 类选择器（`.lucide-pencil`）改为 **`button[aria-label=...]` 可访问名称定位**——按钮若退化为无 aria-label，查询失败即红（a11y 回归在测试层结构性拦截，与 986 守卫站同判据）。

## ③ 门禁

typecheck 过；集成 3 例 + helper 3 例 6/6 绿（可访问名称定位实证生效）；studio 全套件 156 文件 859 例全绿；U+FFFD 净；Rust 树零改动（引用 1001）。提交路径限定四文件：组件 + 集成测试 + i18n 字典 + 本文档。

## 备案

1. BookSidebar 移动端遮罩 div（onClick 关闭）非语义按钮，不设 aria-label（SR 用户走关闭钮路径）。
2. a11y 守卫面扩到 BookSidebar/ArtifactView 域——后续该域新增交互钮按本形（aria-label + 可访问名称定位测试）落地。

下一号自 1003 起；出口回归常备；用户侧待办 = Fork 推送（含 955-1002）。
