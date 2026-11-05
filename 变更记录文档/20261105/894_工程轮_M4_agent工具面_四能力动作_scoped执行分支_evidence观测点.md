# 894 号 · 工程轮：M4——agent 工具面（四能力动作 + revise 工具 + scoped 执行分支 + evidence 观测点 + coverModel 覆盖）

> 886 设计 §三 M4。capability 路由 ≈430 行 + scoped 链装配 ≈200 行 + e2e 两组；
> testgate 2064+584、clippy 双 0、gate:ts 八步全绿、交替互证 33/33、U+FFFD 净。

## 一、交付面

### scoped 执行分支（production/short_fiction_revision.rs + short_fiction_writer_chain.rs）

TS applyRevisionPlan L291-328 scoped 分支接通：
- **ReviseChapterRequest 扩 ScopedReviseInput**（instruction_json={revisionBrief,
  chapterInstruction}+章题+governed context（original-manuscript excerpt=
  otherChapters JSON protected）+author 源引用五件）。
- **scoped_or_compress_revise_chapter_fn 装配**：scoped=select（SourceLocatorAgent
  chapter identity）→ resolveAuthorTextPermission（selectorVersion 7 + body 剥壳
  ——TS body() 章题在场才剥）→ Scoped 合同=直接 ReviserAgent（authorized 注入）
  /Ranges=authorize=None（未授权自主 edit_ranges 面）；compress=892 回退委托。
- **apply_revision_plan 分支**：scoped_active = authorSource && authorScope &&
  (!wholeManuscript || !restructuring)（TS 逐字）；原章缺失→OUT_OF_SCOPE；
  prose_authorized（chapterNumbers/whole+空列表）→ revise 调用 + **onCandidate
  距离改良逐个并入**（firstReplacement=原章内容即直接采纳）；章题授权
  （chapterTitleNumbers）→ submit_short_revision_title 提交（TS L315-321 逐字）
  否则原题保留；严格单章校验+并槽+进度。

### 四能力动作 + revise 工具（server/short_capability_route.rs 新模块）

- **POST /api/v1/works/:workId/capabilities/short-fiction/:actionId** 五动作：
  plan/draft/review/package_short_fiction（runShortFictionStage 前置工件门+
  retryStages 装配）+ revise_short_fiction（checkpoint 编排接线）。
- **episode 环**（adopt.rs 同款事件序）：recover→create→episode-started→
  action-started→work-artifacts-synced→action-completed→episode-completed→
  finish(Completed)；profile 能力守卫 + work 互斥锁。
- **EvidenceSink 观测点**（execution.record → episode 事件）：source-material-
  used（worker+source+lineage）/edit-scope-selected（authority=author_request+
  scope）/skills-applied（id+source+sha256+references——cover skill 注入面）。
- **结果面 TS 逐字**：stage text=`${stage} ${stageStatus}: ${paths}`，details
  kind=short_stage_completed|short_stage_failed；revise text=`Revised short
  fiction "<id>" with N review observation(s).`，kind=short_fiction_revised。
- **已持锁入口**：`revise_short_fiction_production_locked`（tokio Mutex 不可
  重入——路由环已持锁，双重 lock 即自死锁；M1a episode_store 堆栈采样先例
  同源诊断法复用：sample 定位 kevent park + 探针二分）。

### coverModel resolve 覆盖（llm/cover.rs）

`resolve_cover_generation_request_with`（显式入参优先于 project config/env）+
`generate_cover_image_artifact` 增 cover_model/evidence 双参（skills-applied
证据面 inline）。

### 短链契约面补全

ShortProductionStageResult/ShortFictionRevisionResult 加 Serialize（camelCase
——capability details 面）；ShortProductionStage 加 lowercase rename。

## 二、测试（runner scoped 1 用例 + capability e2e 1 用例全绿）

1. **revise_scoped_branch_runs_reviser_chain**（runner）：author_request+baseline
   → scope 派生 → locate → scoped replacements → review 双轮 → revisionChanges
   ==[1]。
2. **capability_stage_actions_episode_flow**（e2e）：work 种子 → 四动作逐调用
   （产物断言 outline/drafts/final/reviews/production-state）→ episode 事件序
   （每动作一 episode：started→action-started→synced→…→episode-completed）→
   revise 动作（scope 派生+scoped 单段 replacements 链 12 调用+final 落替换
   文本）。

### 测试自坑（三条实证）

- **mock 脚本 mismatch 弹即失**：mismatch 时条目已 pop——脚本序漂移即连环错位
  （探针 request/popped 双面打点实证）。
- **授权 scoped=单段直接 replacements**：edit_ranges 双段形态=未授权 spot-fix
  自主定位面；runner 单测 ScriptedAgent 不校验工具名掩盖了该语义（e2e 严格
  校验暴露）——TS submitSpotFix authorized 直接 replacements 逐字对齐。
- **MutexGuard 跨 await Send 误差二度**：`for candidate in sink.lock()…clone()`
  循环头临时活整个循环体——owned 化先行（892 同族坑第三实例）。

## 三、验证清单

testgate 2064+584 / clippy 双 0 / gate:ts 八步全绿（含 engine-contract-diff）/
交替互证 33/33 / e2e_write_next_contract 211 全绿 / lib 1603 / U+FFFD 净。

## 四、备案（下一轮 = 895 号：旧链退役 + M4 收尾）

- **旧链退役 duel**：pipeline/short_fiction_runner.rs（78 号）+agents/short_
  fiction.rs 旧六代理+server execute_short_run 旧实现已无消费方（893 切链），
  代码保留待 live duel（INKOS_DUEL=1 真跑对照绿后删除——695 纪律+duel-requires-
  inkos-duel-env 记忆条）。895 首件事。
- plan_short_fiction 的 TS OutlineParameters（direction 必填+outline 无
  maxChaptersPerCall）参数差异化——Rust 侧统一 params 面（行为等价，schema 面
  M4 收尾时对齐）。
- propose_action 确认卡→能力动作的 Studio 前端路由（confirmation 卡面）未动
  ——桌面端 short_run 确认流走 agent_route（893 已切新链），capability 路由
  供 agent 工具循环与外部调用。
- 工作台 revise 工具（TS createShortFictionReviseTool 的 AgentTool 循环注册）
  ——engine 侧以 capability 动作承载（HTTP 面），agent_loop 工具注册表接线随
  895 评估（当前 propose_action 确认流已覆盖用户路径）。

**下一轮 = 895 号：旧链退役（live duel 后删除 78 号 runner+旧代理+死代码）+
M4 收尾（schema 参数面差异化+工具注册表评估）；下一号自 895 起；出口不变；
用户侧待办=Fork 推送 28 个提交。**
