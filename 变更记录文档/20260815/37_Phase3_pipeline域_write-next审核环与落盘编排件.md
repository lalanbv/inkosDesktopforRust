# 37 号｜Phase 3 pipeline 域：write-next 审核环与落盘编排件（runner 前置三件套）

> 里程碑：`_writeNextChapterLocked` 的三个自包含编排件移植——
> length-normalizer（长度单次修正）、chapter-review-cycle（审计→修订评分环，
> write-next 内环核心）、chapter-persistence（章节工件落盘编排）+ 状态降级
> note 子集。39 号 runner 主体所需的全部"编排件"至此就绪。

## 背景

36 号后 write-next 链只剩 runner 编排本体。它依赖三个自包含模块（合计
~670 行 TS）：审核环（回调注入、可独立测试）、长度归一化 agent、落盘编排。
先行独立移植以保证 39 号主体装配时零逻辑风险。state-validator +
truth-validation（重试结算链）划入 38 号。

## 改动

### 新建 `engine-rs/src/agents/length_normalizer.rs`（~530 行，含测试）

- `normalize_chapter`：mode 判定（spec.normalizeMode 为 none 时
  chooseNormalizeMode）→ LLM(0.2) → 净化 → 双护栏（截断检测 / 对侧硬边界
  越界 → 保留原文）→ 警告分级（硬/软区间外）
- `LengthNormalizerChat` trait 注入；prompt（系统/用户）逐字移植
- 净化链（golden 守门）：fence 提取 → 包装行剥离（5 组中英 wrapper 正则，
  >50% 剥离疑似误伤退回 trim）→ 空串回退原文
- `looks_truncated`：收尾标点/代码栅栏判正常，流中截（含尾部空白 + 中点
  标点特判）判截断

### 新建 `engine-rs/src/pipeline/chapter_review_cycle.rs`（~700 行，含测试）

- `run_chapter_review_cycle`：评估（LLM 审计 + AI 味 + 敏感词 + 确定性后写
  检查每轮重跑/回退初始 postWriteErrors + 字数硬门控）→ 修订（auto 模式）
  → 复评（temperature 0）→ 快照择优（区间内优先 / 分数 ε=3 净提升）→
  必要时回退最高分版本
- 端口注入：`CycleReviser` / `CycleAuditor` / `DraftLengthNormalizer`
  （async trait）+ `ReviewCycleCallbacks`（同步回调，类型别名化）
- 常量：`DEFAULT_MAX_REVIEW_ITERATIONS=1`、`PASS_SCORE_THRESHOLD=85`、
  `NET_IMPROVEMENT_EPSILON=3`

### 新建 `engine-rs/src/pipeline/chapter_persistence.rs`（~430 行，含测试）

- `persist_chapter_artifacts`：saveChapter →（非降级）saveTruthFiles →
  索引 upsert（同号保留 createdAt）→ markBookActive → 审计漂移指引
  （降级清空；失败静默——TS `.catch(() => undefined)`）→（非降级）快照 +
  事实历史同步
- `PersistenceHooks` trait 注入全部副作用；`now` 时间源可注入
- UTC 时间戳：`civil_from_unix`（Howard Hinnant 算法，无 chrono 依赖）

### 新建 `engine-rs/src/pipeline/chapter_state_recovery.rs`（~160 行，含测试）

note 三件（chapter-persistence 依赖）：`build_state_degraded_review_note` /
`parse_state_degraded_review_note`（字段级校验，坏载荷 None）/
`resolve_state_degraded_base_status`（无注记时按 `[critical]` 行判定）。
重试结算链依赖 StateValidatorAgent，随 38 号移植。

### golden 双侧

- TS dump +2 域 22 例：`length_normalizer_suite`（15 例：prompt 双语/净化
  五态/截断矩阵/警告三级/越界双侧——类私有方法实例括号访问）、
  `state_degraded_note`（7 例：构建/解析/解析拒收/基状态三态）
- 审核环为异步编排（TS 侧需全 mock 回调），由 Rust 集成测试镜像 TS
  `chapter-review-cycle.test.ts` 的七个场景（见下）

## parity 要点（移植难点）

1. **长度不进 reviser 问题清单**：normalize 是专用步骤（环首硬漂移触发），
   `lengthInRange` 只作通过线硬门控；环内不再 normalize（REVISED_CONTENT
   字数漂移由快照择优兜底）
2. **`postReviseCount = revisedWordCount`**：TS 怪癖——存字数非轮次，逐字保留
3. **快照 reduce 无初值语义**：首元素起步；同区间需 `>= best + ε` 才置换；
   回退条件 =（best 在区间且当前不在）或 `best >= current + ε`
4. **包装行剥离的 50% 护栏按 UTF-16 码元**：`stripped.length < trimmed.length * 0.5`
5. **`StateDegradedReviewNote` 的 camelCase**：TS JSON 字段是
   `baseStatus`/`injectedIssues`——首版漏 `rename_all` 导致自产自解析失败
   （单测三连锁抓出）
6. **空内容断言的顺序**：reviser 空串输出先走「无新内容退出」分支，纯空白
   才触发 assert（TS 判空在前）
7. **TS 返回的 entry 带新 createdAt**：索引内同号条目保留旧值
   （`{...entry, createdAt: e.createdAt}` map 进索引），返回值本身不保留
8. **user prompt 模板换行**：controlBlock 与 `## Chapter Content` 间有一个
   换行——golden `user-full` 差分抓出后修正（本会话第二例模板换行 bug）

## 验证

- **lib 单测**：816 passed / 0 failed（+24 新单测：normalizer 七态[mode none/
  压缩/截断保留/越界保留/净化/截断矩阵/prompt 逐字]、审核环八场景[即过线/
  解析失败跳修/postWriteErrors 注入/达线退出/无净提升退出/无新内容退出/
  空内容断言/表面净化]、persistence 四态[全链/降级跳步/upsert 保留/
  漂移吞错]、note 两件、civil 算法三时点）
- **golden 差分**：74 域全绿（+2 新域 22 例；差分抓出模板换行真 bug）
- **export-bindings**：975 passed
- **TS 全量**：185 文件 / 1798 测试全绿（leaf dump 重生成）
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **38 号**：state-validator.ts（310 行）+ chapter-truth-validation.ts
  （145 行）+ chapter-state-recovery 重试结算链（`retrySettlementAfterValidationFailure`/
  `buildStateValidationFeedback`/`buildStateDegradedIssues`/
  `buildStateDegradedPersistenceOutput`）+ runner 的 `buildPersistenceOutput`
  （settler 输出 → 持久化产物装配）。
- ⬜ **39 号**：`_writeNextChapterLocked` 主体（prepareWriteInput[复用 35 号
  plan 持久化] → review-cycle[本号] → promotion pass → buildPersistenceOutput
  [38 号] → truth-validation[38 号] → persistChapterArtifacts[本号] → 通知/
  webhook）+ task-store 异步任务模型 + `/api/v1/books/:id/write-next` 路由挂载。
