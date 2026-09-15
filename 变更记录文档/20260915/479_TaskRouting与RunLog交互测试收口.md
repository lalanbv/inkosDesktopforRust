# 479 号：TaskRoutingPanel + RunLogPanel 走查+交互测试——组件测试清单全量收口

日期：2026-09-15　分支：develop　基线：d8fbec56（478 号）

## 选题背景

未测面板序列最后两件：`TaskRoutingPanel.tsx`（126 行，G16/346 号按任务模型路由设置面）与 `RunLogPanel.tsx`（99 行，R26/403 号调用级运行遥测）。收官后 studio 主要面板交互测试清单全量清零。

## 走查结论

**TaskRoutingPanel**：发现死代码一处——`modelFor`（声明后全文件零调用，JSX 直接读 `routing.tasks?.[kind]?.model`，输入框语义=只显示任务级覆盖、留空=全局）即删除（477 号 publishedId 同类，零行为变更）。其余健康：空 model 保存前清理、全空任务条目剔除不落空对象、defaults 从加载态保留进保存载荷、失败经 notice 透出。

**RunLogPanel**：零缺陷。纯读投影只记元数据（无 prompt/正文）、空缓冲/失败整卡不渲染（445/469 号 no-store 缓存头已盖陈旧面）、三态结果 chip（成功/失败[ fatal]/瞬态）、接管徽标带 attempt/round、环形缓冲逐出提示按 total>kept 出现。

## 改动

- `packages/studio/src/components/TaskRoutingPanel.tsx`：删除死函数 modelFor；
- 新增 `TaskRoutingPanel.interaction.test.tsx`（4 用例）：加载回填+未覆盖任务占位、保存载荷（defaults 保留+新任务合并）、清空后任务剔除（不落空字段）、失败透出；
- 新增 `RunLogPanel.interaction.test.tsx`（3 用例）：倒序渲染+耗时格式化（500ms/1.5s）+三态 chip+接管徽标、瞬态 chip+逐出提示、空缓冲与拉取失败整卡不渲染（container.textContent 空断言）。

## 验证

- 单文件 7/7 绿（两次 vi.mock 漏右括号 esbuild 语法错即修——477 号同款笔误两犯，值得警惕）；
- 全量：`pnpm -r test` **core 2112 + studio 906（净增 7）+ cli 232 = 3250 全绿**；
- `tsc --noEmit` 零错误；Rust 侧零改动。

## 里程碑

**studio 组件交互测试清单全量收口**（451 号基建起，455/456/457/458/459/462/464/465/468/471/474/475/476/477/478/479 逐号推进）：全部主要面板（Backup/AntiAi/ChapterWorkspace/Recall/Codex/SeriesCanon/ChapterReader/NotificationCenter/QuickOpen/CommandPalette/AssetLibrary/ContextLens/Deconstruct/Director/TaskRouting/RunLog）+ 次要组件 + ErrorBoundary 均有交互或单测覆盖。后续新面板随实现随测（466 号 TESTING.md 惯例）。

## 后续

- vitest 5.0.0 迁移评估、npm 接受风险 4 条滚动跟踪维持挂起；
- 交互测试面已尽，下一轮选题转向：真实长跑数据采集（R16/R19 数据门控）、Rust 侧测面/工程债、或按用户真实使用反馈打磨。
