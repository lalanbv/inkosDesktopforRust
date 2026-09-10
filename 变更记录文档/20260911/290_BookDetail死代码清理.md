# 290 号：BookDetail exportHref 死代码清理（289 号备案清偿）

- 日期：2026-09-11
- 分支：develop
- 关联：289 号（导出走查——本批清偿其备案的清理候选）
- 编号衔接：查 20260911 目录最大号 289，顺延 290
- 推送核验：origin/develop = a1b34a33 未动；本地领先 274–290 共 17 提交待推

## 一、处置

`BookDetail.tsx` L528 的 `exportHref` 常量（`/books/:id/export?format=...` 下载链接形态）为**无渲染死代码**：组件内零消费（真实导出走 `export-save` 落盘 + alert 路径，见 289 号），全仓亦无外部引用。已删除该行。

保留不动：`exportFormat`/`exportApprovedOnly` 状态与格式选择 UI（export-save 请求体仍消费）。

## 二、验证

`npx tsc --noEmit` ✓ 0 错；studio 全量测试 90 文件/793 测 ✓；`pnpm build` ✓。纯删除性改动，运行路径零影响。

## 三、遗留

274–290 共 17 提交待推送；并行会话三件第十九轮在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
