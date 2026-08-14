# 46 号｜Phase 3 agents 域：consolidator 移植、revise 审核环与 compose/consolidate/repair-state 端点

> 里程碑：books 域再上 3 端点（compose/consolidate/repair-state）+ /revise 升级
> 为**完整审核环**（pre-audit → 修稿 → post-audit → strict 门控 → 落盘）。
> Rust 侧业务端点达 **12 个**。consolidator.ts（218 行）全量移植。

## 改动

### 新建 `src/agents/consolidator.rs`（~420 行，含测试）

移植 consolidator.ts 全量：
- `consolidate`：晋级预处理（advancedCount ≥ 2 翻 promoted——与 write-next 落盘
  前通道同源）→ 卷边界解析 → 完结卷判定（endCh ≤ maxChapter 且有行）→
  每完结卷 LLM 概括（≤500 词 system prompt 逐字）→ volume_summaries 追加 +
  明细归档 `summaries_archive/vol_A-B.md` → chapter_summaries 仅留当前卷
- `parse_volume_boundaries`：`第N卷（第A-B章）` / `Volume N (Chapters A-B)`
  双形态（名字 = 区间前文本去尾随开括号）
- `parse_summary_table`：表头（章节/Chapter/--- 行）与数据行（首列数字 > 0）分离
- `ConsolidatorChat` trait 注入（AgentRouter 适配）

### /revise 审核环（`run_revise_chain`）

对齐 Node reviseDraft 核心链：
- **pre-audit**（FullCycleAuditor 完整编排）→ blocking 计数（非 info）
- 无问题且无显式修订（brief/rewrite/rework）→ `unchanged` +
  `"No warning, critical, or AI-tell issues to fix."`（Node 逐字文案）
- 修稿（pre 问题清单驱动）→ **post-audit（temperature 0）** → strict 门控
  （不变差 && 改善）→ 不达标 `unchanged` + skippedReason（before/after 计数）
- 落盘：章节文件（标题行保留）+ **仅最新章**真相回写（Node 语义）
- 返回 `ReviseChainResult {applied, status, revisedContent, fixedIssues,
  skippedReason}`（对齐 TS ReviseResult 核心面）

### 三新端点（books_routes.rs 扩展）

- `POST /compose`（L3568 契约）：plan（持久化复用）→ 35 号
  compose_governed_chapter（LlmOutlineSelector/LlmContextCompiler 端口）→
  ComposeChapterResult 形状（intent/context/ruleStack/trace 四路径 bookDir 相对）
- `POST /consolidate`（L3587 契约）：同步 → ConsolidationResult JSON + SSE
  `consolidate:complete/error`
- `POST /repair-state/:chapter`（L3594 契约）：**降级守卫三段**（无章节/
  非降级/非最新——Node 逐字文案）→ settle（allowReapply）→ validate →
  失败重试链（38 号）→ 真相回写 + 快照 + **索引状态回翻**（降级注记
  baseStatus 恢复 + injectedIssues 清除）→ RepairStateResult

## 验证

- **lib 单测**：882 passed / 0 failed（consolidator 4：卷边界双形态/表解析/
  归档全链/空输入短路 + books_routes 6：五端点错误路径 + repair 守卫）
- **E2E 契约**：9 passed——新增四主链：**compose**（200 四路径形状 + 三工件
  落盘）、**consolidate**（archivedVolumes=1 + 留存表 + SSE）、**repair-state**
  （非降级 Node 文案）、**revise 审核环**（pre 有问题 → 修稿 → post PASS →
  applied=true + 章节文件标题保留改写 + SSE start/complete）
- **golden 差分**：76 域全绿
- **export-bindings**：1041 passed
- **TS 全量**：185 文件 / 1798 测试全绿
- **clippy**：`cargo clippy --lib --tests --bins` 零警告

## parity/工程要点

1. **mock 分派顺序坑**：修稿系统提示含「审稿意见」——E2E mock 的 audit 分支
   （匹配"审"）先命中导致 reviser 收到审计 JSON、修订稿回退原文。修稿分支
   必须先于 audit 分派（测试注释固化）
2. **审核 mock 的温度区分**：post-audit 固定 temperature 0——mock 按
   `body.temperature == 0.0` 分派 PASS，pre 保持有问题（真实链的温度语义
   成为可测面）
3. **仅最新章拥有真相**：revise 落盘对旧章不回写 current_state/pending_hooks
   （Node 语义：改旧章不倒卷 live state）
4. **门控简化备案**：Node evaluateMergedAudit 含 AI-tell 计数与
   restoreActionableAuditIfLost；Rust 首版以 blocking + issues 总数双指标
   近似（全链移植待 47 号）
5. **修复过程的自我事故**：清理死代码 run_revise 时误删共享装配/imports 块
  ——编译器逐一定位后全量恢复（lib+tests 882 全绿 + clippy 零警告复核）

## 下一步

- ⬜ **47 号**：evaluateMergedAudit 全量（AI-tell 计数 + 恢复丢失问题 +
  revisionGate 三档）补齐 revise 环；`/eval`、`/analytics`、`/export` 等
  books 域剩余端点；sessions/state 配置域端点（Phase 2）。
