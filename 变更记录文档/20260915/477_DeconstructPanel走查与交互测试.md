# 477 号：DeconstructPanel 走查+交互测试——死状态清偿与解析/载荷契约收口

日期：2026-09-15　分支：develop　基线：80e5c922（476 号）

## 选题背景

剩余未测面板序列第四件 `DeconstructPanel.tsx`（160 行，G5/351 号拆书工作台：逐章证据行 → 统一拆书面端点 → 可发布材料池）。446 号已真机端到端验证过发布到材料池链（materialId 落盘 + 双产物），本号补组件交互测面。

## 走查发现并清偿死代码

`publishedId` state（声明 + setPublishedId 赋值共两处）**从未被读取**——发布的材料 id 实际经 notice 文案展示，该 state 为 351 号遗留死状态。删除两行，零行为变更（tsc+全量测试背书）。

其余面健康：`parseChapters` 非章号行跳过、人物按 `[,，、]` 三分隔符拆分、章型/伏笔仅非空时携带；busy 状态互斥双按钮；`run` 与 `publish` 走同端点仅差 `publish:true`+可选 `name`；失败路径错误信息经 notice 原样透出。设计观察备案：`language` 取 UI 语言而非书籍写作语言（作者按界面语言读产物的有意取舍，不改）。

## 改动

- `packages/studio/src/components/DeconstructPanel.tsx`：删除死状态两行；
- 新增 `packages/studio/src/components/DeconstructPanel.interaction.test.tsx`（4 用例，jsdom）：

1. **分析链**：两行证据（顿号/英文逗号混合人物、含章型伏笔 vs 空人物行）→ POST 载荷精确断言（characters 拆分、可选字段条件携带、`depth:"full"`+`language:"zh"`）；notice「已生成并落盘。」+ markdown 产物渲染；
2. **无效输入**：非章号行 → 不发包 + 格式提示；
3. **发布链**：载荷带 `publish:true`+可选 `name`；产物 id「已发布到材料池：mat_1」进通知（`publishedMaterialId` 有无两分支中回包携带路径）；
4. **失败路径**：`mockRejectedValueOnce` → 错误信息经 notice 透出。

## 验证

- 单文件：4/4 绿（首跑一次 esbuild 语法错（vi.mock 漏右括号）即修，635ms）；
- 全量：`pnpm -r test` **core 2112 + studio 893（净增 4）+ cli 232 = 3237 全绿**；
- `tsc --noEmit` 零错误；Rust 侧零改动。

## 后续

- 剩余未测面板：DirectorPanel（128 行）/TaskRoutingPanel（126 行）/RunLogPanel（99 行）——按需逐号补齐；
- vitest 5.0.0 迁移评估、npm 接受风险 4 条滚动跟踪维持挂起。
