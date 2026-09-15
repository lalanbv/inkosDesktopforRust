# 476 号：ContextLensPanel 走查+交互测试——四态接线收口

日期：2026-09-15　分支：develop　基线：9a3d4e17（475 号）

## 选题背景

474/475 号盘点序列的下一未测面板 `ContextLensPanel.tsx`（212 行，R21/391 号上下文透视：纯读回放每章治理装配条目+压缩留痕）。440 号已做真机走查（12/12 对齐+空态+441 压缩分支全要素吻合），本号补组件交互测面。

## 走查结论（零缺陷，纯测面）

双 effect 均有 `cancelled` 守卫（章节列表加载与章留痕拉取的竞态防护在位）；`chapter=0` 作"无章节"哨兵安全（引擎章号 1 起）；`tierLabel` 未知层级回退原 id（防御在位）；空态/失败态文案齐备。备注段为「留痕备注：」+join 拼接单段——测试断言教训一则（逐条断言须正则，getByText 整段匹配不到单条）。

## 改动

新增 `packages/studio/src/components/ContextLensPanel.interaction.test.tsx`（4 用例，jsdom，无 cmdk 依赖故无需 ResizeObserver 桩）：

1. **默认末章+全要素渲染**：加载章列表后 select 默认值="3"（末章）；条目行层级中文标签（本书事实/临时未注册）+「保护」「编译产物」双徽标+~tokens；汇总行已编译压缩分支（`3 条来源 · 受保护 1 · 约 4200 tokens · 预算 8192（已编译压缩）`）；压缩留痕块（2 条可压缩来源逐条+token）与备注段；
2. **章切换**：`user.selectOptions` 切第 1 章 → 拉取 `/context-lens/1` → 「预算内未压缩」分支、无压缩留痕块；
3. **失败态**：切第 2 章（mock 抛 HTTP 404）→ 错误信息原样透出；
4. **空态**：无留痕书 → 「暂无装配留痕」文案。

## 验证

- 单文件：4/4 绿（首跑 3 绿 + 1 断言按实际渲染形状修正为正则，580ms）；
- 全量：`pnpm -r test` **core 2112 + studio 889（净增 4）+ cli 232 = 3233 全绿**；
- `tsc --noEmit` 零错误；Rust 侧零改动。

## 后续

- 剩余未测面板：DeconstructPanel（160 行）/DirectorPanel（128 行）/TaskRoutingPanel（126 行）/RunLogPanel（99 行）——按需逐号补齐；
- vitest 5.0.0 迁移评估、npm 接受风险 4 条滚动跟踪维持挂起。
