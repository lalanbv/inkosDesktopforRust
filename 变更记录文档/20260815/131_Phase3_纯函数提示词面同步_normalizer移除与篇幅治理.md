# 131 号（Phase3）：纯函数/提示词面同步——normalizer 阶段移除与篇幅治理

日期：2026-08-18 · 前置：130 号（合并 d3ba425e 值级漂移 9 套件备案）

## 背景

130 号将 9 个 golden 奼件以 `#[ignore]` 备案（上游 e7c04465 值级漂移）。本轮
按再生的 leaf.json 真值逐面移植，全部解除。核心是**length-normalizer 阶段
的整体移除**与**篇幅治理（lengthBudget）的新机制**。

## 实现

### A. normalizer 阶段移除（上游语义逐字）

- `models/length_governance.rs`：`LengthNormalizeMode` 与
  `LengthSpec.normalizeMode` 删除；`LengthTelemetry` 的
  postWriterNormalizeCount/normalizeApplied → **repairApplied**（审核环修复
  是否改变正文——TS `snapshots.length > 1 && final !== initial` 同式）。
- `utils/length_metrics.rs`：build_length_spec 去 normalizeMode；
  choose_normalize_mode 删除。
- `agents/length_normalizer.rs` **整模块删除**（525 行）；agent_router 的
  LengthNormalizerChat 端口、bin agent 列表、装配点全清。
- `chapter_review_cycle.rs`：环内长度归一步骤移除（TS "No in-loop normalize
  needed"——修订稿字数漂移由 lengthInRange 硬门控 + 快照择优兜底）；
  `ChapterReviewCycleResult` 对齐 TS（preAuditWordCount 环首取值 +
  repairApplied）；ReviewCycleError::Normalizer 删。
- `write_next.rs`：normalize 阶段/NormalizerAdapter 删；manual 分支
  repairApplied=false；build_length_warnings 文案对齐（"第N章未达到篇幅预算
  （min-max，实际 N）。"/"Chapter N is outside its length budget (...)"）。

### B. 篇幅治理新机制（lengthBudget）

- planner：system prompt 新增 "## 场景与篇幅预算" 结构节 + 输出要求新条目
  （2-5 真实场景、合计落硬区间、禁止凑字数；en "Scene and length budget"
  同款）；user 模板卷外约束新增 "章节篇幅预算：目标 N unit；建议…；硬区间…"
  行；`PlannerLengthBudget` 入参（unit：en→"words"/zh→"字"，TS planner.ts
  换算逐字）；PlanChapterInput/PlanChapterMemoInput += chapter_word_count
  （TS input.book.chapterWordCount 基准——write-next 覆盖不作用于此）。
- fallback memo（重试耗尽降级）：整段文案对齐 TS 新版（含场景节，硬区间/
  目标来自 lengthSpec）。
- `chapter_memo_parser.rs`：REQUIRED_SECTIONS 头位新增 "## 场景与篇幅预算"
  （minContentChars 20）——planner/mock 输出无该节即解析失败（8 案例
  expected 全部变为 missing sections 的真值来源）。
- settler user prompt：伏笔池节标题 → "## 当前伏笔池（含活跃伏笔与本章
  语义相关的休眠种子）"；settler system 伏笔规则族升级（休眠种子复用/
  语义职责归属/newHookCandidates 仅全新承诺三条款 + 规则 3/4 改写）。
- state-validator：`ValidationResult.repairRequired`（首行 REPAIR 裁决 →
  true；JSON 显式 true；verdict 正则补 REPAIR 三态——原二态是移植缺陷）。

### C. reviser 契约瘦身 + 修订端点 settle 化

- `ReviseOutput` 删 updatedState/updatedLedger/updatedHooks（修订不再直更
  真相——状态归结算器）；parse 的 UPDATED_* 标签提取删；governed 表合并
  调用删；auto/legacy system prompt 的 UPDATED 三块输出格式段删；legacy
  原则段对齐（"4. 正文必须服从既有事实和伏笔约束，但不要输出或重写状态
  文件；宿主会根据修订正文重新结算"）。
- 修订端点（books_routes）真相覆盖改 **settle 数据源**（TS reviseDraft 新
  链：revise → settle → 覆盖审计/落盘）。
- `SettleChapterStateInput.baseline_chapter`（TS baselineChapter 重放语义）：
  settle 读 `story/snapshots/{N}` 基准 truth；新增
  `load_runtime_state_snapshot_at_chapter`（state 四 JSON 优先 + markdown
  重建兜底）与 `build_runtime_state_artifacts_from_snapshot`
  （runtime_state_store）；修订端点传 targetChapter-1。

### D. TS dump 测试缺陷修复（参数错位）

golden-leaf-dump 的 reviser 面调用 `parseOutput(content, gp2, mode, …)` 比新
签名多传 gp2——**全部 parse 案例错位走 legacy 分支**（期望值 "polish"/"auto"
等是错位产物）。修签名后重生成 leaf.json（期望值回到真实语义：
legacy-fallback→原章、rewrite-only→拒补丁回原文、patch-only→应用补丁）。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1176** 过 |
| `cargo test --test golden_leaf` | **70** 过 + **0 ignored**（9 套件全部解除） |
| `cargo test --test e2e_write_next_contract` | **188** 过 |
| `cargo test --features export-bindings --lib` | **1334** 过 |
| `cargo clippy --all-targets` | 零警告 |
| TS vitest（packages/core） | **195 文件 / 1859** 全过 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. **132 号候选**：reviseDraft 完整链对齐（settle 后的 stateValidator 校验 +
   retrySettlementAfterValidationFailure 环 + revisionDiagnostics 完整形态
   ——本轮端点仅对齐数据源，校验环仍是简化版）；runner harness 类重构
   （agentCtxFor/worker-agent/production harness）；skills 生产模式绑定。
2. write-next 主链的 `writeProductionRunSnapshot`（生产运行快照，TS 新增）。
3. think 剥离器（MiniMax 内联 think 块）与 trajectory 遥测头。

## 事故记录

- 一个 python 脚本把 `s = … if False else None` 误赋 None 后 `open(p,'w')`
  已截断 runtime_state_store.rs——git checkout 恢复后重新追加并全量回归
  （教训：条件 replace 永远不用 `x if False else None` 惯用法）。
