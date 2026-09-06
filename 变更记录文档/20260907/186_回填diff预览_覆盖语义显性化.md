# 186 · 回填 diff 预览：写入前的现有内容 × 勾选内容差异视图

- 日期：2026-09-07
- 模块：packages/studio（pages/series-backfill-diff.ts 新增 + 测试、pages/SeriesBackfillPanel.tsx、use-i18n.ts）、packages/studio/api/server.ts（existing 端点）、engine-rs（server/series_backfill_routes.rs existing 端点 + 双路由注册）
- 类型：feat（C3 可选增强第一项，价值最高且无需产品决策）
- 关联：184 号（回填向导）、185 号（契约 duel）

## 一、范围与设计

apply 的写入语义是**整体覆盖** `story/series_backfill.md`——未勾选的既有条目会被移除。本批把这一语义在写入前显性化：

- **双端 `GET /api/v1/books/:id/series-backfill/existing`**：读目标书现有 series_backfill.md，缺失 → `{"content":null}`（可缺失面不制造 404）。
- **前端本地渲染将写入内容**（`renderBackfillMarkdown` 与双端 apply 渲染模板**逐字对齐**——双端渲染一致是 diff 可信的前提，偏差会被 diff 自身暴露）。
- **LCS 行级 diff**（`diffLines`，O(n·m) DP，行数百级可接受）：kept / removed / added 三色分类；**头部元数据行（标题/来源/抽取时间）不参与 diff**——重新抽取必然刷新「抽取于」，算进差异只是噪音。
- **UI**：预览区新增 diff 视图（+N/-N 计数 + 红色删除线/绿色新增/灰色保留），勾选变化即时更新；提示「写入为整体覆盖：取消勾选的既有条目将被移除」。
- apply 成功后立即重拉服务端文件作为 diff 新基线（否则基线停留在写入前，覆盖语义显示错）。

## 二、验证

| 项 | 结果 |
| --- | --- |
| studio typecheck（双 tsconfig） | 干净 |
| studio vitest 全量 | **769/769 绿**（diff 纯函数 4 用例：渲染一致性/LCS 分类/首次写入全 added/覆盖语义 removed） |
| engine-rs cargo test 全量 + clippy | 全绿 + 零告警（existing 端点；1237 lib + 194 + 70） |
| 浏览器端到端（真实引擎） | ①首次写入（existing null）：added 12 / removed 0；②apply 写入 2 条后再取消勾选 1 条 → removed 4 行（含「## [character] 林动」「隐忍坚韧的主角…」）；③恢复勾选 → removed 0；④重新抽取刷新头部时间戳 → 头部行不出现在 diff（噪音过滤生效） |

## 三、过程教训

1. **diff 预览暴露了双端渲染模板的真实偏差**（前端头部少一段 `（id）`）——「预览与写入用同一渲染」要么同源要么逐字对齐测试锁定；本批以后者落地（前端模板注释标注对齐要求）。
2. **apply 后的基线刷新**：覆盖写入成功后若不重拉服务端文件，diff 基线停留在写入前——后续勾选变化的 removed 语义全部失真。副作用成功后同步刷新派生状态。
3. python replace 的 anchor 静默失配两次复发——183/186 号教训合并：碎片化编辑必须 `assert anchor in s` + 替换后立即 typecheck。

## 四、遗留

- C 系可选增强剩余（按价值排序，待真实写作流反馈再动）：节拍与章节状态联动着色 > 节拍拖拽排序 > 抽取 prompt 类别扩展。
- 生成端点默认关闭待产品决策。
- 推送须在 Fork 图形端执行（既有约定）。
