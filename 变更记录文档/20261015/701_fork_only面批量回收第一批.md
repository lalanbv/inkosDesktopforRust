# 701 号：fork-only GET 面批量回收第一批（零 SQLite 族，回收清单 52→46）

日期：2026-10-15。类型：fix(core)+fix(studio)+fix(scripts)。前置：700 号（3141aa20）。

## 一、承接

691 号模式（asset-library 单族回收）升级为**批量回收**：第一批=零 SQLite 依赖族的 7 路由（run-log、tension-curve、task-routing GET/PUT、style-profiles GET/POST、writing-stats、best-of-n GET/PUT、anti-ai-rules GET/PUT）——回收清单 **52 → 46**。

## 二、实现

- **tension-curve.ts**：`parseChapterSummariesMarkdown` 随上游删除的 story-markdown.ts 迁入本模块（自包含：表格行解析+张力列 clamp，TensionRow 结构兼容直接传入 analyzeTensionCurve）；
- **core index 导出**：globalRunLog/projectRunLog/analyzeTensionCurve/parseChapterSummariesMarkdown/ANTI_AI_RULE_SEEDS/validateAntiAiRule（+tension 曲线族）；
- **server.ts 8 路由嫁接**：run-log（globalRunLog 环形缓冲投影）、tension-curve（真相源 chapter_summaries.md 读+解析）、task-routing GET/PUT、style-profiles GET/POST、writing-stats（state.listBooks/loadChapterIndex + studio lib writing-stats；2.0 ChapterMeta 无 status 章→行状态恒 active 适配）、best-of-n GET/PUT（book.json governance 段）、anti-ai-rules GET/PUT（种子兜底+校验落盘）。
- **contract-diff 域表同步**：7 路由移出 FORK_ONLY_ROUTES、入 SHARED_FACE。

## 三、度量（三域报告实测）

**共享面 13 对比 / 0 分歧（扩容后保持全绿）**；fork-only **46/46** 确认（52→46）；上游新增 9/9+2。
新入共享面逐点：tension-curve 400/400（无章节一致性）、task-routing 200/200、style-profiles 200/200、best-of-n 400/400、anti-ai-rules 400/400——**五种状态形态双端一致**。

## 四、施工实录（三断点）

1. **域表 token 移除留洞**：每行双条目形态的列表按 token 删后留悬空逗号行→数组洞→route=undefined→replaceAll 崩——node --check 兜住后逐行清理；
2. **shared 集扩展块锚点漂移**（同 692 型）：以实存文本重锚；
3. **深度对称回归**：fixture 自适应重构误删 fork 腿的 REPORT_MODE 分支→rust 腿重跑全链 chaptersWritten=2 vs TS=1——698 号深度对称分支补回（两机制共存：形态自适应+深度对称）。

## 五、教训

1. **批量移植的最小单位=「引擎模块+index 导出+路由」三件套**，与 691 单族流程同构可复制；SQLite 依赖族（promises/quality-trend/roster）需先做 node:sqlite 只读 Mini 端口（702 批次）。
2. **活体度量门禁会自证其身**：域表更新出错（数组洞）立即以崩溃暴露——度量门禁的可信度建立在它自己先过 syntax/checks 上。
3. 深度对称（698）与形态自适应（697）是两个独立机制，重构时逐一核对存活——698 的 REPORT_MODE 分支误删即刻被三域报告量回（chaptersWritten 2v1）。
