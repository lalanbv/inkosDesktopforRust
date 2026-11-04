# 891 号 · 工程轮：M3c 终——ReviserAgent 双提交面 + 长度合同 2.0 + 修订执行链

> 886 设计 §三 M3c 第三轮（M3c 收官）。新增四模块 + 3 测试用例；testgate 2048+584、
> clippy 双 0、gate:ts 八步全绿。

## 一、交付面

### length_contract.rs（新）

TS `length-metrics.ts` L58-80 的 **2.0 显式 bounds 形态**：ShortLengthSpec
（target/countingMode/min/max——无软硬区间，886 勘察判定的 2.0 重写面之一）+
`chapterLengthDelivery`（无显式 bounds → None——宿主不做近似目标判定）+
`assertChapterLength`（越界 → CHAPTER_LENGTH_OUT_OF_RANGE，message = details
JSON 串 TS 同形）。旧 utils::length_metrics（软/硬区间制，write-next 活体）不动。

### narrative_context.rs（新）

ContextSource/ContextPackage 契约（zod strict ↔ deny_unknown_fields）+
`renderNarrativeSelectedContext`（`### Context N` + JSON(source,protection) +
reason/detail，空 excerpt 行剔除）。

### short_fiction_reviser.rs（新，≈900 行含测试）

ReviserAgent.reviseChapter 短篇链消费面：
- **buildRevisionProtocol** 五模式双语逐字（polish/rewrite/rework/anti-detect/
  spot-fix）。
- **governed 提示面**：reviewObservations 渲染（有观察逐条/无观察回退行）+
  observationList（instruction 前置）+ selectedContext 渲染 + 篇幅要求块
  （zh/en unit 文案逐字）+ spot-fix/authorized 时 numberReviewSource 编号源。
- **EditPlanContract 联合**（Ranges/Scoped 统一 apply_empty/parameters_schema/
  collect_replacements/apply_replacements/field_lengths 五面）。
- **submitSpotFix 两段**：authorized 注入=直接 scoped；否则 submit_chapter_
  edit_ranges（4096 预算）→ 行区间合同 → 骨架 apply_empty + replacementBudget
  数学（fixedContentLength/minimum/maximum；maximum<0 → CHAPTER_EDIT_SCOPE_
  CONFLICT）→ submit_chapter_range_replacements（instruction payload 逐字 +
  CHAPTER_LENGTH_OUT_OF_RANGE 回喂：scope=selected_ranges + measuredBudget
  （increaseByAtLeast/reduceByAtLeast）+ fieldLengths + candidateCommitted:false）。
- **submitRewrite**：CHAPTER_BODY_EMPTY 守卫 + 长度合同 + revisedContent 归一。
- outputBudget = min(defaults, max(8192, target*4+8192))。
- `candidate_sink`（Arc<Mutex<Vec<String>>>）替代 TS onCandidate 闭包（Send+Sync
  面——validate 闭包内 push、测试终态断言）。

### short_fiction_revision.rs（新）

writer 修订执行链：
- **validateRevisionPlan** 纯逻辑校验全支（PLAN_INVALID 标题空/章号面/instruction
  空新场景、OUT_OF_SCOPE 标题未授权+章号越界+开篇越权、OPENING_HOOK_CONTRACT
  长度、PLAN_EMPTY 全不变、SOURCE_REUSED 多目标复制 details、OUTLINE_REQUIRED
  重构缺纲）。
- **applyRevisionPlan**：槽位 padding→逐章计划执行（无 instruction=原章复制；
  有 instruction=submit_short_revision_chapter 结构化提交）→ storyTitle/
  openingHook 覆写（trim 空=删除语义）→ rawContent 再渲染 + 进度回调。
- `writer_input_view`/`chapters_to_submissions` 复用件。

## 二、测试（reviser 3 用例全绿）

1. **spot-fix 双段 golden**：轮 1 submit_chapter_edit_ranges（选行 5）→ 轮 2
   submit_chapter_range_replacements → 修订字节恒等（区间外标题行/第一段原样）
   + 双段 tool 名序断言 + toolChoice required。
2. **长度合同回喂重试**：首轮越界（"太短"）→ CHAPTER_LENGTH_OUT_OF_RANGE 回喂
   → 次轮合规成功（重试环活性证明）。
3. **恒红变异**：三轮全越界 → 必红（错误串含 CHAPTER_LENGTH_OUT_OF_RANGE）。

## 三、验证清单

testgate 2048+584 / clippy 双 0 / gate:ts 八步全绿 / production 57+3 用例 /
U+FFFD 干净。测试自坑：scripted fake 借用冲突（agents 借本地 chat/defaults 后
元组 move）——改返原始件由调用方组装（借用次序显式化）。备案：applyRevisionPlan
的 scoped/compress 分支经 ReviseChapterFn 注入位已留（892 轮 M3d runner 接线时
装配）；planRevision 的 maxTurns 6 环由基座承载（M3d 接线）。

**下一轮 = 892 号 M3d：runner 编排层**（production-state 状态机已就位（887）+
四代理全量（889/891）→ loadCompletedShortRun 幂等 + produceShort 四阶段 +
stageResult acceptPaths 族 + 产物落盘 + 失败恢复）。**下一号自 892 起；出口不变；
用户侧待办=Fork 推送 25 个提交。**
