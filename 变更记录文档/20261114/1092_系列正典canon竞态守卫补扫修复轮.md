# 1092 号 · 系列正典 canon 竞态守卫补扫修复轮

- **类型**：fix+test（生产 1 文件净增 10 行 + 新回归测试 1 文件 2 例）
- **范围**：`packages/studio/src/components/SeriesCanonPanel.tsx`（守卫）+
  `SeriesCanonPanel.test.tsx`（新增）

## 一、家族扫定性（1008 族补扫，修洞按族扫方法论）

全 studio 源码 `useEffect`+`fetchJson` 共现 28 文件为候选面；其中 **12 文件零
`cancelled`/seq 守卫**，逐一以「效果依赖是否原位变化」定性：

| 文件 | 定性 |
|---|---|
| AntiAiAndExperiencePanel / CodexPanel / QualityTrendCard / RosterCandidatesCard / TensionCurveCard（deps `[bookId]`） | **无竞态面**：BookDetail 以 `key={view.bookId}` 挂载（App.tsx:699），换书整页重挂载，`[bookId]` 效果不存在原位重发；unmount 后 setState 在 React 18 为无害 no-op |
| SeriesCanonPanel 效果一（deps `[bookId]`） | 同上，无竞态面 |
| ServiceConfigSourceCard（`[]`）/ WritingStatsCard（`[]`） | mount-only 无重发面 |
| ChapterWorkspacePanel | 效果内无 fetch（事件处理器驱动+busy 门） |
| **SeriesCanonPanel 效果二（deps `[seriesId]`）** | **真缺口**：seriesId 是原位变化的内部 state——绑定保存→loadBinding→setSeriesId 重发效果→loadCanon 重取；`loadCanon` 无守卫 |

已守卫面核验（顺带确认）：AssetLibraryPanel / BackupPanel / PlayHud 均已是 seq
latest-wins（1008 族先例，注释在位）；ChaptersSection / CharacterSection /
FoundationSection / SummarySection / DirectorPanel 等均带 cancelled/seq 守卫。

## 二、缺陷与修复

- **缺陷**：`loadCanon` 无守卫。seriesId A→B 改绑后，A 系 canon 响应晚到
  `setEntries(A)` 覆写 B 系条目（:56 原形态）；其失败晚到 `setEntries([])`
  清空 B 系好数据（:64-66 原形态）——1008 号 ChaptersSection 同款双缺陷形态。
- **修复**：`canonSeqRef` seq latest-wins（惯用形同 AssetLibraryPanel/PlayHud）。
  **关键细节**：seq 在 `!id` 早退分支**前**递增——解绑清空也占号，压住在途旧
  id 响应晚到「复活」条目。
- 备案不扩面：`loadBinding` 仅挂载+saveBinding 顺序调用点，其交错面是双击
  保存的双提交族（955 号在途互斥族），非本竞态族；不加无活体证明的投机守卫。

## 三、回归锁（红绿闭环）

- 惯用形同 ChaptersSection.test.tsx：jsdom + deferred 五响应链（挂载绑定→
  alpha canon[晚到]→PUT 改绑→绑定重读→beta canon[新]）+ `vi.stubGlobal("fetch")`
  + 真实 DOM 改绑点击 + `setAppLanguage("zh")` 确定性锚；URL 断言钉住竞态面
  （`/series/alpha/canon` 与 `/series/beta/canon` 调用序）。
- 例 1 覆写形态 / 例 2 清空形态。
- **红绿证据链**：绿 2/2 → **NEUTER1090-1** 摘 try 守卫→恰红例 1（覆写复现）
  例 2 绿 → **NEUTER1090-2** 恢复 try+摘 catch→恰红例 2（清空复现）例 1 绿
  → 互逆恢复（diff 零 NEUTER 残留，净增=预期修复面 10 行）→ 复绿 2/2。
  （NEUTER 编号沿用动工时号，撞号改号后正文以 1092 为准。）

## 四、验证

- studio 全套件 **165 文件 900 例全绿**（898 基线+2 净增）+ typecheck 双
  tsconfig 过 + 两触文件 U+FFFD 扫净（exit=1 无匹配）；engine-rs / src-tauri /
  core / cli 零改动。

## 五、编号备案

- 让号 1090（并行会话完整十腿复验轮 ae2a7f04 20:59:14 落库于本轮动工期间），
  再让号 1091（并行会话稳态确认轻巡检轮写档在途），本轮取 1092。

## 下一号

自 1093 起；出口=下一巡检周期 / earendil-mz 单包合并（触发条件制持续）；用户侧
待办=Fork 推送（含 955-1092）。
