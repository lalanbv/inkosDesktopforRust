# 815 号：quality-trend 首叉深入收口——rust `/resync` 补审计段（TS `resync+audit` 语义对齐）

日期：2026-11-02。前置：808 号登记（quality-trend `/trend/points` 长度 2 vs 1=TS resync 审计导入章执行深度差 + 数值分协议残差）。

## 一、定性（本轮完成）

- 夹具写链**双腿都调** `POST /resync/1`（walkthrough-fixture.mjs:309，"settle→validate 链，memory.db 前置"）。
- TS `/resync/:chapter` = `executeExplicitCapabilityTool(resync_chapter_state)` → `resyncChapterStateAndAudit` = **resync + reviewChapter**（审计更新索引观察）→ 导入章 ch1 获得 observations → 趋势点。
- rust `/resync/:chapter` = `run_resync_chain` 仅工件同步，`audit_result` 刻意桩（"issues 恒空" 对齐 TS 结果 payload 形状）→ ch1 无 review_metrics 行 → 少一个趋势点。
- 语义差异实锤：TS 工具名即 `resync_chapter_state`=**resync+audit**；rust 缺 audit 半段=功能缺口（非形状错位）。

## 二、修复

1. **抽取**：write_next 内联的指标记录块（R3/361）抽为 `pipeline::write_next::record_chapter_review_metric(book_dir, chapter_number, &audit_result)`（severity 计数 + contentHash 幂等 + 修订覆盖；失败仅告警）。tracing `target:` 须常量 → 固定 target "review-metrics"。
2. **路由审计段**：`resync` 路由在 `run_resync_chain` 成功后 `record_resync_audit`——扫描 `chapters/` 取目标章正文（`leading_number` 同口径）→ 审计 → 沉淀指标。**链内不加**：`run_resync_chain` 另被交互编辑工具复用（TS 对偶 `_resyncChapterArtifactsLocked` 本就不审计，HTTP 路由才走 resync+audit）。
3. **端口修正（关键）**：首版用 `RoutedAgent` 文本审稿（PASS/FAIL 行协议）——mock 返回 645 JSON 契约时无 PASS 行 → `passed=false`、分值回落 50（活体实锤 ch1 50/false 假阴性、failingChapters [1]）。改为 **FullCycleAuditor 真实编排端口**（同 write-next 审核环/审计路由，645 JSON 契约）→ ch1 得 88/true。
4. 交互编辑工具路径维持不审计（与 TS 对偶一致）。

测试：`record_chapter_review_metric_writes_counts_and_is_idempotent`（severity 计数/分值/verdict 落库、同内容重放仍一行、修订分值覆盖）。

## 三、活体（收敛度量）

diff 71/1 维持，quality-trend 首叉 `/trend/points` 长度（2 vs 1）→ `/trend/points/1/issueCount`（值差）：
- **结构层全收敛**：points 长度 2=2、chapter 1=1/2=2、`passed` 双腿 true、`failingChapters []=[]`、urgency 三项逐项同（H01/H02/H04）。
- **rust ch1 审计 88/true/0 issue**（端口修正后），averageScore 88（rust 有、node 无=数值分协议缺口）。
- 剩余分叉=两条**已登记残差**：① rust 有 `overallScore/averageScore/scoredChapters`（2.0 审改协议无数值分，上游面）② ch2 issueCount 1 vs 0（mock 两协议审改内容不对称：rust 审稿 JSON 1 issue vs TS submit_chapter_review 零发现）。

## 四、验证

cargo:testgate engine-rs 47 目标 **1976**（+1）+ src-tauri 25 目标 584 + RustSec 双 0；clippy 双 0；gate:ts 八步全绿（含 engine-contract-diff + legacy 11/11）；权威 pnpm -r test core 179/987 + studio 137/725 + cli 23/81（TS 零触碰，三包全绿）。

## 五、登记（695 工项余项）

- quality-trend 值差两件（数值分协议缺口=上游面；mock 审改内容不对称=mock 记录工具支族）。
- mock 记录工具支（需状态机）；渲染副本合并重构（择机）。
- TS `resyncChapterStateAndAudit` 的结果 payload audit 为桩而 rust 也为桩（双侧一致），实际审计副作用走索引观察/metrics——形状与副作用分离的既有设计，维持。

## 六、教训

1. **端点语义以"副作用"对齐，不以"payload 形状"对齐**：双侧 resync 的 auditResult 都是桩（形状一致），但 TS 的真实审计走副作用（索引观察）而 rust 无——只比 payload 会漏掉功能缺口。
2. **同实体多端口要看调用方用什么**：`CycleAuditor` 有两个实现（RoutedAgent 文本协议 / FullCycleAuditor JSON 编排）——新调用点必须选与既有消费面（write-next 指标）相同的端口，否则解析协议错配产出假阴性（50/false）。
3. **"执行深度差" 定性到端点语义即闭环**：808 的"TS resync 审计导入章"本轮落到具体端点（resync+audit vs 仅同步）后，修复就是一段路由代码。
