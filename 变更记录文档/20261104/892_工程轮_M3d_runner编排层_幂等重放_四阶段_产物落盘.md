# 892 号 · 工程轮：M3d——runner 编排层（幂等重放 + produceShort 四阶段 + 产物落盘 + 失败恢复 + 书锁）

> 886 设计 §三 M3d。runner 本体 ≈2900 行（含测试）+ 底座升级三件；testgate
> 2060+584、clippy 双 0、gate:ts 八步全绿（含 engine-contract-diff 跨引擎交替腿）。

## 一、交付面

### short_fiction_runner.rs（新，≈2900 行含测试）

TS `pipeline/short-fiction-runner.ts`（1773 行）的编排层移植：
- **入口 + 书锁**（L194-225）：`runShortFictionProduction`——storyId 显式或
  title slug 化 → 每作品 tokio Mutex（adopt.rs `acquire_work_mutation_lock`
  提为 pub(crate) 复用）→ 幂等重放短路 → produceShort → 失败候选记录。
- **loadCompletedShortRun 幂等重放**（L227-306）：七工件齐备 → final draft
  strict 解析 + validateForFinal → package-warning 在场即失 → cover 图像门 →
  retryStages 门 → inputHash（draft+outline+intent）双阶段核对 →
  **requestHash 核对**（evidenceVersion 2 + reference + constraints + comparison
  scope 面 + authorRequest + model；override 语义 = TS `options 定义项 ?? saved
  target`，双缺省键省略）→ package.reviewHash == shortPackageReviewHash(delivery)
  → 未 persisted 历史回退观察（production-history-unavailable）。
- **produceShort 四阶段**（L349-723）：target 合并（saved target 补齐）→
  outline 续读/DRAFT_SOURCE_CHANGED 注册版核对 → outline 代理/续跑分支 →
  productionState v2 提交 → draft 阶段（writer 批次 + persistDraftBatch 落盘
  登记 + continueDraft 完成环 3 次尝试 + validateForFinal（preserve 宽松形态）+
  v001 落盘 + reviewedArtifact/reviewedHash 字节核对）→ review 阶段（对照面
  parent_revision/episode_start 读基线 + reuseReview 缓存 + 失败 draft-warning
  降级 + stages.review 提交）→ package 阶段（refreshDelivery + reusePackage
  缓存 + **一次修正环 pass 0/1**（每审查锚定确切包装 + 修正再生成
  `revision:{previous,observations}` 第三条 user 消息）+ package-warning 降级）→
  封面门（disabled / SHORT_COVER_PACKAGE_REQUIRED / 生成本体 fail-closed
  SHORT_COVER_UNAVAILABLE——模块头备案）→ refreshDelivery 交付合同观察
  （长度/标题/开篇合同 + verified review issues）→ 最终同步（标题别名 +
  changed paths + SHORT_DELIVERY_CONTRACT_CODES 过滤）。
- **stage acceptPaths 族**：review（draft-v001/draft-warning/production-state +
  revised 时 manuscript+changed）/package（sales 三件+review+state）/draft
  （manuscript+changed）/outline（outline+state）+ previousTitlePath 淘汰面；
  `runShortFictionStage` 单阶段续跑入口（前置工件门 + retryStages 装配）。
- **产物落盘**（L1103-1200）：writeDraftArtifacts（full.md+draft.json+四位章节
  文件+obsolete 删除）/writeFinalArtifacts（final 四件 + 标题别名
  shortTitlePath（safeFileName，保留名比对终名）+ 前标题别名删除）/
  writePackageArtifacts（sales 三件）——全部 textWrite（trimEnd+\n）原子集提交。
- **失败恢复**（L322-332）：shortDraftRecoveryError——v001-partial 候选解析 +
  state.target 合并的 minimum 重算 → incomplete/persisted/valid 章号集 →
  recovery payload（action short-fiction__draft_short_fiction + parameters
  null 面）→ message 尾部 Persisted draft checkpoint；produce 失败且 Work
  在场 → syncWorkSourceArtifacts(accept:false) 候选记录（AggregateError 语义）。

### short_fiction_writer_chain.rs（新）

- **writer_continue_draft**（TS continueDraft L451-508）：空批 merge 槽位
  padding → SHORT_TITLE_MISMATCH 守卫 → 独立开篇修复环
  （submit_short_opening_hook：distance 改善即并入 + onBatchComplete +
  validateOpeningHook 严格回喂，maxTokens 2048）→ 缺章完成计划（operation
  三分支逐字 + measured length/target/min/max 指令串）→ applyRevisionPlan。
- **writer_plan_revision**（TS planRevision L220-242）：maxTurns 6 环 +
  submit_short_revision_plan 扁平章槽 schema（allowStoryTitle=scope 授权面）+
  supporting 双工具 + validateRevisionPlan 回喂（normalize_plan_result 的
  chapter_N_sourceNumber/instruction 归一）+ 终态二次校验。
- **writer_revise_draft**（TS reviseDraft L207-218）：单章快路径（无
  opening/outline/title 权限）→ planRevision → `outlineMarkdown ?? input` 补齐
  → applyRevisionPlan。
- **compress_revise_chapter_fn 装配**：ReviserAgent spot-fix 包装
  （revision-scope governed context 逐字 + build_length_spec + candidate_sink）
  → ReviseChapterFn 注入 applyRevisionPlan。

### short_fiction_tools.rs（新）

- `calculate_story_interval`/`calculate_review_values`（story-interval.ts/
  review-calculation.ts 逐字：receipt 形态 + STORY_INTERVAL_CLOCK_MISMATCH/
  PRECISION_MISMATCH + REVIEW_CALCULATION_UNDEFINED 码面）。
- `shortRevisionPlanSubmissionSchema`（扁平 chapter_N 双槽）+
  ShortAuthorScope 契约 + shortAuthorScopeRequest（双 system 逐字 + 章号
  存在性 validate）。

### 底座升级（既有模块）

- **short_fiction_revision.rs**：applyRevisionPlan 升级到 TS 完整语义——
  validate 闭包（单章非严格+严格双验 + 候选改良 distance 面逐字 + 每 chapter
  validateForFinal）+ **SHORT_CHAPTER_TOO_LONG compress 回退经 ReviseChapterFn**
  （merge_improved_candidates 候选按序采纳复刻 TS onCandidate）+ 进度回调
  async 化（MutexGuard 不跨 await）+ validateRevisionPlan 首行校验 +
  chapter_numbers/author_scope/opening_hook_chars 上下文面。ReviseChapterFn
  签名重设计（ReviseChapterRequest/Outcome——candidate sink 显式）。
- **short_fiction_agents.rs**：ShortAgents.author_request（TS
  currentExecutionAuthorRequest 前置 authority 注入，professionalGuidance=false
  不注入——reviewPackage）；**reviewer_review_package**（package/manuscript 源 +
  配对校验 SHORT_PACKAGE_REVIEW_PAIR_REQUIRED（记录时+终态）+ categoryRequired
  硬约束）；**supporting 三件补齐**（889 缺漏：record_review_observation 恒伴
  calculate_review_values + calculate_story_interval）；packaging_generate
  revision 面（第三条 user 消息逐字）。
- **source_sync.rs**：captureWorkSourceState（字节指纹 + .db/.sqlite WAL 并入
  ——TS 同源注释语义）/changedWorkSourcePaths（前后指纹并集 diff 排序）。
- **adopt.rs**：acquire_work_mutation_lock 提为 pub(crate)。

## 二、测试（runner 3 用例 + 链/工具 8 用例全绿）

1. **四阶段 mock 端到端 + 幂等重放**：outline/writer/review(记录+终审)/
   package(修正环) 全脚本 → 14 产物路径断言（work.json+七件+chapters+reviews+
   state）→ 二轮空脚本重放命中（任何代理调用即 script exhausted）→ 变异删
   sales-package.json → 重放失效 → 新脚本重跑全链。
2. **失败恢复**：批过 merge 但 min=50 越界 → 补章环脚本耗尽必红 → recovery
   payload（persistedChapterNumbers=2/incomplete=2/action 面）+ v001-partial
   落盘断言。
3. 链测试：continue_draft 补章金标准（章 2 经 completion plan 补齐+进度通知）/
   title mismatch 必红/单章快路径（不进 plan 代理）/normalize 扁平槽归一；
   工具测试：interval receipt 金标准+双错误码/calculation receipt+三错误形态/
   plan schema 槽位面/author scope validate 双向。

## 三、验证清单

testgate 2060+584 / clippy 双 0（25→0：MutexGuard 跨 await 二处真患 + json!
spread 二处 889 同族坑）/ gate:ts 八步全绿（含 engine-contract-diff）/ U+FFFD
净。测试自坑：test 断言路径双 source 前缀误写、shortTitlePath("full.md") 保留
名比对的是终名（TS 同形 Some("full.md.md")）、批量空正文在 merge 即拒（TS 同形
——恢复场景须用 min 越界触发）。

## 四、备案（893 M3e 接线清单）

- **reviseShortFictionProduction**（TS L742-926 checkpoint 编排）与 applyRevision
  Plan 的 scoped 执行分支（authorSource/authorScope → reviseChapter + 章题提交）
  随 893 服务面接线（writer_revise_draft 已留 author_scope 入参位）。
- **封面生成本体**：2.0 buildCoverImagePrompt（JSON 请求块+双语指导）+
  reference 编辑路由（images/edits、responses input_image、gemini inlineData）
  + skills 注入（inkos-story-cover）——旧 cover.rs（78 号 generic prompt 形态）
  服务旧链不动；生成路径 fail-closed SHORT_COVER_UNAVAILABLE。
- recordExecutionEvidence 观测点（source-material-used/skills-applied 等）与
  activatedSkills 哈希面——M3e 从 episode 上下文装配。
- runtimes 四上下文路由 + short_fiction_run 能力动作 + 旧链退役 duel = 893。

**下一轮 = 893 号 M3e：服务面接线**（确认卡→能力动作 + runtimes 四上下文
路由 + revise checkpoint 入口 + 旧链退役决斗 + 交替互证扩展）。**下一号自
893 起；出口不变；用户侧待办=Fork 推送 26 个提交。**
