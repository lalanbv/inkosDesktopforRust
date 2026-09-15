# 478 号：DirectorPanel 走查——Node 回退端保存断裂修复 + keywords 回填防御

日期：2026-09-15　分支：develop　基线：f071b979（477 号）

## 走查坐实两处缺陷（双端对照 + 442 缺陷类）

**① Node 回退端 PUT /books/:id/director 契约断裂（真缺陷，静默 no-op）**：所有客户端（DirectorPanel 保存、BookCreate 建书灵感卡 354 号）与 Rust 端（`ops_routes.rs put_director` 读 `body.patch`）均以 `{patch:{...}}` 包裹合并键，而 Node 回退端 handler（`server.ts`）读的是**顶层** `body.runMode/body["inspiration"]`——回退端下两个写入方全部静默无效（返回 ok:true，UI 还显示「已保存」，实际什么都没合并）。修复：对齐 patch 契约（`body.patch ?? {}`，runMode 白名单同步从 patch 读）。Rust 端本就正确，零改动。

**② inspiration 整体替换下的 keywords 静默清空（442 缺陷类，潜在）**：`put_director` 合并是顶层键替换——`inspiration` 键一旦在 patch 中即整体覆盖。DirectorPanel 保存固定发 `keywords: []`，而 keywords 是双端方向候选 prompt 的真实消费字段（非空时渲染「- 关键词：…」）。今日无非空写入方（BookCreate 写的就是空数组、directions 流程不落会话），故为潜在而非现行；但 UI 一保存即抹。修复（442 号 populateFields 同款防御）：加载时回填 keywords 进 state，保存原样回传。

## 改动

- `packages/studio/src/api/server.ts`：PUT director 读 `body.patch`（+注释说明断裂根因）；
- `packages/studio/src/components/DirectorPanel.tsx`：keywords 状态回填+回传（带缺陷类注释）；
- 新增 `packages/studio/src/components/DirectorPanel.interaction.test.tsx`（3 用例）：加载回填五要素（灵感/模式/阶段/已存章节/续跑建议）、保存载荷 patch 包裹+keywords 原样回传（回归）、编辑+切模式后新值携带且 keywords 不丢、失败 notice 透出；
- `packages/studio/src/api/server.test.ts` 追加 `director session PUT patch contract`（3 用例）：patch 体合并落盘（updatedAt/bookId 盖章）、patch 未涉键不动存量（directions/keywords 保留）、patch 内非法 runMode 400。

## 验证

- 交互 3/3 绿；契约测**红绿验证**：git stash 修复后 3/3 红、恢复后 3/3 绿；
- 全量：`pnpm -r test` **core 2112 + studio 899（净增 6）+ cli 232 = 3243 全绿**；
- `tsc --noEmit` 零错误；Rust 侧零改动。

## 后续

- 剩余未测面板：TaskRoutingPanel（126 行）/RunLogPanel（99 行）——按需逐号补齐；
- vitest 5.0.0 迁移评估、npm 接受风险 4 条滚动跟踪维持挂起。
