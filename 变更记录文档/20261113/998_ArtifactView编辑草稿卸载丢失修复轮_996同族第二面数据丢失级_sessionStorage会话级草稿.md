# 998 号：ArtifactView 编辑草稿卸载丢失修复轮——996 同族第二面（数据丢失级）+ sessionStorage 会话级草稿

日期：2026-11-13。开工三查净（HEAD=4da7b1da、工作区空、无并行会话）。996 备案②复核转正轮。

## ① 备案②定性修正：「暂无同族缺陷」有漏

996 备案②只查了**重拉**路径（editContent 独立于 fetch state，重拉不覆盖）便定性「暂无同族缺陷」——漏了**卸载**路径：`editing/editContent` 是 ArtifactView 本地 useState，而组件挂在 ContextDock 的 `visible` 条件渲染下（`if (!visible) return null`）。三条卸载路径全部丢编辑中未保存输入：①dock 开合（visible 切换）②面板切换（sidebarView 变更，artifact → panels 分支替换）③页面切换（App 层条件渲染卸载整个 dock 区）。比 996 筛选态丢失（偏好级）严重一档——**未保存输入丢失（数据丢失级 UX）**。

## ② 修复：sessionStorage 草稿（996 惯例形延伸，会话级语义恰好）

- 新模块 `lib/artifact-edit-draft.ts`：`artifactEditDraftKey`（键 = `inkos.artifactEdit:{bookId}:{ch:N|truth路径}`——多项目多文件互不串扰）+ load/save/clear 三件套；损坏 JSON/字段形状错回落 null（非编辑态）；SSR typeof 卫兵 + 隐私模式 try/catch 双卫兵（996 同形）。
- **sessionStorage 而非 localStorage 的语义依据**：编辑草稿是易失工作数据而非 UI 偏好——应用重启恢复「编辑中」语义怪（文件可能已被外部改），会话级生命周期恰好：webview 会话内 dock 开合保留，应用重启自然丢弃。
- ArtifactView 接线五处：effect 目标切换时恢复草稿（无草稿回落非编辑态，与原切换语义一致）；handleEdit 建草稿；textarea onChange 实时写（卸载安全的关键——每次输入即持久化）；handleSave 成功清草稿（失败保留——不丢未保存内容）；取消清草稿。

## ③ 红绿闭环（两层测试）

- helper 单测 3 例（node 环境，内存 sessionStorage stub——996 同形）：key 构造双形互不串扰 / roundtrip+clear / 损坏 JSON 与字段形状错回落 null。
- ArtifactView 集成 3 例（jsdom + 真实 chatStore `openArtifact` + `vi.stubGlobal fetch` mock）：①**编辑→输入→卸载→重挂载恢复**（987/996 场景直证——textarea 直接出现且含未保存内容）②保存成功 PUT body 含草稿全文 + 草稿清（mock GET 恒原文，草稿清除证据 = sessionStorage null + 无 textarea——首轮断言错用「重挂载显示已保存新增」未考虑 mock 形态，勘误先于改正）③取消清草稿回原文。
- NEUTER998 必红活体：`saveArtifactEditDraft` 早退摘写入 → **恢复例独红**（卸载后无草稿可恢复；保存/取消例不受影响恒绿——语义自洽的三例分形）→ 互逆恢复 6/6 复绿。

## ④ 门禁

typecheck 双 tsconfig 过；studio 全套件 **154 文件 851 例全绿**（+6：helper 3 + 集成 3）；U+FFFD 净；Rust 树零改动（引用 997）。提交路径限定五文件：新模块 + 其测试 + BookSidebar.tsx + 集成测试 + 本文档。

## 备案

1. 草稿按目标键单份（当前会话内同文件反复编辑覆盖旧草稿）——编辑会话语义下足够；多版本历史属编辑器范畴非本面。
2. 保存失败保留草稿（catch 分支不动）——「失败不丢未保存内容」语义在本号文档显式化。
3. 996 备案②原文已在 996 文档不回改——本号即修正记录（996/997/998 三轮连续复核先例：备案定性可在后续轮复核转正，档案不回改）。

下一号自 999 起；出口回归常备；用户侧待办 = Fork 推送（含 955-998）。
