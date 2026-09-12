# 371 号：G11 best-of-N 契约层——多版选优计划与候选选优（择机项启动）

- 日期：2026-09-13
- 类型：契约层 + 配置面（双端镜像 + golden 差分）
- 规划依据：[328 规划 G11 / 355 二轮择机项](../../开发时SpecCoding'sPlan/inkosDesktop/07_产品演进规划/对标调研分析与改进规划_v2.md)（审查评分选优接入 qualityGates；354 号清算结论「G11 可启动，G3 前置就位」）

## 做了什么

1. **契约层双端**（`best-of-n.ts` / `best_of_n.rs`）：
   - `resolveBestOfNPlan`：未启用 → 0 追加；启用 → 首版分数 ≥ minScore
     → 0 追加（首版够好）；分数缺失（审查未评分）或 < minScore →
     candidates-1 追加（保守触发）；candidates clamp 2–3（缺省 2）；
     minScore 缺省 75；
   - `selectBestCandidate`：score 降序稳定（同分保持输入序；缺分排在
     有分之后）；空候选抛错。
   - 评分源 = 既定 continuity 管道 `AuditResult.overallScore`（0–100），
     选优结果接 qualityGates（354 号前置）。
2. **配置面**：`BookConfig.governance.bestOfN = {enabled?, candidates?,
   minScore?}`（TS zod + 书级治理同域；默认关——不改变现行为）。
3. **golden**：`best-of-n-vectors.json`（plan×5 + select×3）；TS
   `golden-best-of-n.test.ts`（3 用例）与 Rust
   `golden_best_of_n_diff.rs`（2 差分）读同文件全绿（第二十三守门域）。
   **serde_json Number 键序/整型差异教训：差分断言逐字段比较，golden
   分数带小数点**（367 号同款问题第二次，模式确认）。

## 兼容性决策

- 默认关（enabled 缺省 false）：G11 是质量优先的付费能力（N 倍生成+审查
  成本），由书级 governance.bestOfN.enabled 显式打开。
- 本批为契约层+配置面；**写作主链重生成循环接线留 372 号**（涉及
  writeNextChapter creative 段改造与审查评分串联，宁缺毋滥单独成批）。

## 验收（golden×8 + duel + 双端全绿）

- core：tsc 干净；vitest **230 文件 / 2053 用例**全绿（370 基线 229/2050
  + 1 文件 3 用例）。studio tsc 干净；vitest 802 全绿。
- engine-rs：`INKOS_DUEL=1 cargo test` 28/29 目标 ok + 隔离复跑
  strangler_duel **10/10 全绿**（首跑 bin 进程 60s 未就绪=release 重编译
  资源型假失败，非逻辑回归，341/348 号同模式）。
- bindings **177** 全绿；bench:gate 静默窗通过（-2.3%）。

## 影响面

- 新增 4 文件（TS 契约/golden 测试/共享向量/Rust 镜像+差分），修改
  3 文件（best_of_n/mod.rs/book.ts/index.ts）。
- 未触碰并行会话在途文件（package.json 根文件、rustbin.rs、audit-rust.mjs）。

## 里程碑

- **G11 best-of-N 契约层落地（择机项启动）。** 后续：372 号 G11 接线
  （写作主链重生成选优循环 + qualityGates 串联 + 配置 UI）。三轮规划
  再调研立项可与此并行。
