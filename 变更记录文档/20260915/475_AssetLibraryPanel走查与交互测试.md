# 475 号：AssetLibraryPanel 走查+交互测试——切库展开态串显修复与六链收口

日期：2026-09-15　分支：develop　基线：947b1c80（474 号）

## 选题背景

474 号盘点指认的剩余最大未测面板 `AssetLibraryPanel.tsx`（261 行，R4/364 号三库资产管理：列表/新增/删除/便携包导入导出/采用到本书）。447 号已做过面板级功能走查（零缺陷），本号补组件交互测面并做缺陷类排查。

## 走查发现并修复一处真实缺陷（红绿验证）

**切库后展开态跨库串显**：`expandedId` 在 kind 切换时不清理，而资产 id 由名称派生、**不带库前缀**——同一 id 可在题材基底/推进模式/世界样本三库并存。在 A 库展开某资产后切到 B 库，B 库同 id 资产沿用旧展开态自动展开，展示的是另一库资产的正文（数据不串、纯展示面串）。

修复（1 行+注释）：kind 切换 onClick 内 `setExpandedId(null)`。红绿实证：测试先写，修复前恰好只有该回归用例红（"异库正文"可见），修复后 6/6 绿。

走查其余面健康：id 派生对纯中文名有 `asset_${Date.now()}` 兜底；upsert 空名/空正文禁用保存；覆盖同 id 属文档化语义（placeholder 明示"新增/覆盖同 id"，导入资产的 samples 被覆盖丢弃为 replace 契约的有意行为，备案不改）；删除失败提示内置种子受保护；采用/导出/导入错误路径均有 notice 兜底。

## 改动

- `packages/studio/src/components/AssetLibraryPanel.tsx`：切库清展开态修复；
- 新增 `packages/studio/src/components/AssetLibraryPanel.interaction.test.tsx`（6 用例，jsdom）：

1. **三库 tab 切换**：分别拉取对应快照端点，种子徽标随 `seeded` 显隐；
2. **新增链**：保存按钮空表单禁用→填写启用；PUT 载荷断言 id 派生（"Epic Saga"→`epic_saga`）+ 多行字段拆数组；保存后 notice「已保存」+表单清空；
3. **删除链**：DELETE 路径（encodeURIComponent id）+重拉；失败注入→「删除失败（内置种子不可删）」；
4. **导入链**：`userEvent.upload` 直注隐藏 file input（455 号惯例），文件原文作 POST body，结果计数进通知；
5. **采用链**：refs 载荷 `{kind,id}` 精确断言 + notice 计数；无 `bookId` 时按钮不渲染（queryByText 负断言）；
6. **导出链+修复回归**：导出端点拉取且无失败提示（jsdom 无 `createObjectURL` 已桩）；切库后同 id 异库资产不得沿用展开态。

## 验证

- 单文件：修复前 5 过 1 红 → 修复后 6/6 绿（956ms）；
- 全量：`pnpm -r test` **core 2112 + studio 885（净增 6）+ cli 232 = 3229 全绿**；
- `tsc --noEmit` 零错误；Rust 侧零改动。

## 后续

- 剩余未测面板：ContextLensPanel（212 行）、DeconstructPanel/DirectorPanel/TaskRoutingPanel/RunLogPanel——按需逐号补齐；
- vitest 5.0.0 迁移评估、npm 接受风险 4 条滚动跟踪维持挂起。
