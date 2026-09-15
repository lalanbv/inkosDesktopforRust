# 474 号：CommandPalette 交互测试——⌘K 命令面板四条交互链收口

日期：2026-09-15　分支：develop　基线：dcc305d8（473 号）

## 选题背景

内部盘点（本轮）：主要面板交互测试覆盖清点发现 `CommandPalette.tsx`（⌘K 全局命令面板，235 行）为**剩余最大未测交互面**——其 lib 层纯函数早有 `lib/commands.test.ts`（19 用例）覆盖，但组件层（Dialog 壳 + recents store 接线 + 空态兜底）零测试。471 号已收 ⌘P QuickOpenPalette，本号补齐成对的 ⌘K 面。

## 走查结论（442/462 缺陷类排查）

通读 `CommandPalette.tsx` + `lib/commands.ts` + `store/recents/`：

- `allCommands` useMemo 空依赖安全（构建函数纯）；ctx 每渲染重建无过期闭包（151 号 P1-5 缓解在位）；
- `run` 先关弹层再执行命令、空态兜底 `action.bookCreate` 查找有 `?? allCommands[0]` 兜底、空组剔除与 recent<navigation<action 排序均正确；
- `recentToRoute` 坏记录（缺路由参数）静默丢弃、`buildRecentCommands` id 用 `recentIdentity` 六字段判键；
- **未发现 442（SeriesCanon）/462（CodexPanel）同类的"自动选中不回填"缺陷**。本号为纯测面收口，无产品代码改动。

## 改动

新增 `packages/studio/src/components/CommandPalette.interaction.test.tsx`（4 用例，jsdom）：

1. **空查询首屏**：最近访问组（store 注入 2 条）+ 推荐组（新建长篇小说/最近首位/项目设置）渲染；全量导航/动作命令不在首屏（`queryByText` 双负断言）；书名在最近项+推荐位双现按 `getAllByText` 计数断言。
2. **导航命令链**：中文查询"题材"过滤出"题材管理"→ 点击 → `setRoute({page:"genres"})` + `onOpenChange(false)`。
3. **动作命令链**：查询"深色"过滤出"切换到深色主题"→ 点击 → ctx `theme:dark`、路由零调用（动作/导航分野）。
4. **无匹配兜底**：`zzz-不存在` → "没有匹配的命令"空态 → 点兜底"创建新书"→ ctx `bookCreate` + 关闭。

测试基建沿用 471 号惯例：模块级 `ResizeObserver` 桩 + `scrollIntoView` 补垫（cmdk/radix 依赖）；`useRecentsStore.setState` 直注最近访问（zustand 真实 store，不经 localStorage）；ctx 桩与 `lib/commands.test.ts` 同构（调用序记录）。

## 验证

- 单文件：4/4 通过（首跑即绿，947ms）；
- 全量：`pnpm -r test` **core 2112 + studio 879（净增 4）+ cli 232 = 3223 全绿**（输出中的 writer/architect failed 行为既有的负路径断言噪声）；
- `tsc --noEmit` 零错误；
- Rust 侧本号零改动，cargo/duel 门禁不触发。

## 后续

- 剩余未测面板：AssetLibraryPanel（261 行）、ContextLensPanel（212 行）、DeconstructPanel/DirectorPanel/TaskRoutingPanel/RunLogPanel——按需逐号补齐；
- vitest 5.0.0 已发，迁移评估维持挂起（473 号备案）。
