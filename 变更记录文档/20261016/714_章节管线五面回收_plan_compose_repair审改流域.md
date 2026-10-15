# 714 号：章节管线五面回收——plan/compose/repair-state + approve/reject 审改流域（回收清单 20→15）

> 目标线：自主持续开发（TS/Rust/桌面壳）。本批=BookDetail 活消费的章节管线
> 与审改流域五面；consolidate 据实留守（715 批随三 util 连片恢复）。

## 一、嫁接内容（TS 侧 2.0 基底，Rust 侧零改动）

### 1. core runner 三方法（fork 逐字 + 2.0 内核适配）
- **planChapter / composeChapter**：fork 逐字（createGovernedArtifacts 2.0 同
  签名在树——resolveGovernedPlan 持久化复用语义即 699 号进度链）。返回形状
  以 Rust 线格式为真源（PlanChapterResult 5 键 / ComposeChapterResult 9 键）；
  compose 的 `ruleStackPath` 诚实填空串（2.0 composer 无独立规则栈产物——
  Rust composer.rs rule_stack_path 对应面被 trace 体系吸收；写面探针状态码
  口径 + UI 只读 chapterNumber/title，保形不伪装）。
- **repairChapterState**：fork 公共壳逐字（book 锁 + state-degraded/latest 双
  护栏，与 Rust run_repair_state books_routes.rs:1452 同构）；settle 内核复用
  2.0 进化形态 `_resyncChapterArtifactsLocked`（fork 私有 settle 链在 2.0 已被
  withStateProjectionContext/reconcile 覆盖——护栏是契约面，内核是进化面）。

### 2. ChapterMetaSchema 增可选 status 键（709 自毁键教训第四例）
- 实锤：strict schema 无 status，approve 写 `status:"approved"` 落盘后
  loadChapterIndex 再读即 ZodError（整本书章节索引不可加载）。Rust
  ChapterStatus 14 态为真源镜像（`CHAPTER_STATUSES` 导出）；strict 保留，
  键级扩展（709 先例：根 strict + 可选键）。
- 投影自动透传：GET /books/:id chapters 原文直出 → BookDetail 行徽标/
  「全部通过」按钮/行级通过驳回按钮按 status 条件渲染首次可达（2.0 章
  status 此前恒 undefined，审改流域按钮自 688 号起不可达——与本批嫁接前
  的 fork-only 路由 404 同构的 UI 断链）。

### 3. studio 五路由（isSafeBookId 守卫 + fork 逐字）
approve（未命中静默 200 map 语义 = Rust 同款）/ reject（404 typed 文案 +
rollbackToChapter 回滚 + discarded 清单）/ plan / compose / repair-state
（SSE 广播 repair-state:complete/error 同 fork）。

## 二、fixture 与 mock 基建（三处契约面修复）

1. **fixture 补播 TS 腿结构化 runtime 四件套**（714 新增）：plan/compose/
   repair 管线面读 manifest/current_state/hooks/chapter_summaries——缺失时
   planChapter 链 ENOENT 恒 500（contract-diff 双腿不对称实锤后补）。
   `createInitialRuntimeState`=store 官方初始化（schema 演进随真源走，
   不手写 JSON）。
2. **mock 增 TS planner 工具协议分派支**：2.0 planner=submitStructured
   submit_chapter_memo 工具调用（Rust planner=纯 chat 文本「创作总编」支
   已有）。按请求 tools[] 检测返回工具调用 arguments——双腿分派各自保真
   （713 教训：LLM 面探针必须配 mock 真分派）。
3. **mock ReferenceError 复盘**：首版分派支在 try 块外引用块级 `p` → 全部
   chat 请求 500（directions 200→500 回归当场暴露）。修复=解析与 persona
   特征同块（toolNames 提入 try）。**教训：mock 手术后的全量回归必须先跑
   contract-diff 再继续**（本次靠 directions 面状态翻转当场抓获）。

## 三、探针与域表（20→15，+6 探针）

- SHARED_FACE 增：plan POST（mock 双腿分派 200/200）/ compose POST（200/200，
  persisted plan 复用语义）/ repair-state/1（非 state-degraded 双端同护栏
  500）/ approve 1（200 落盘）/ reject 1（rollbackTo 0 快照缺失双端同护栏
  500）/ reject 2（未命中 404 typed 体）。:num/:chapter 不在 instShared 替换
  表——字面写死 fixture 第 1 章。
- FORK_ONLY 移 5 条 → **15**（consolidate 留守=715 批随 story-markdown/
  hook-lifecycle/hook-promotion 三 util ~700 行连片恢复后接 ConsolidatorAgent
  ——rerunPromotionPass 依赖闭包评估后据实切批，不硬塞本批）。
- 度量：**共享面 55 对比 0 分歧 + fork-only 15/15 + 上游新增 9/9**。
- 一次 /books chaptersWritten 偶发分叉（node 腿 fixture 部分失败容忍路径），
  复跑自愈——695 报告模式容忍语义的已知波动面，备案不拦截。

## 四、回归锁（chapter-pipeline-recycle-routes.test.ts，4 测）

- approve：状态写面 200 + **status 键 roundtrip 可读**（strict 自毁键回归锁）
  + 未命中静默 200。
- reject：未命中 404 + rollbackTo 0 护栏 500 + CHAPTER_STATUSES 14 态真源
  对齐（approved/rejected/state-degraded 在列）。
- plan/compose：无 LLM 500 错误体非 TypeError（713 判别锁口径）+ 编码路径
  穿越 400。
- repair-state：护栏三连（空书 / 非 state-degraded / 未命中章）。
- 播种纪律：**canonical works/{id}/source 布局**（books/ legacy 播种
  StateManager 读不到——709 号正典布局锁在测试基建的再应用）+ mock LLM
  配置（custom:Mock）让 buildPipelineConfig 通过、护栏可达。

## 五、门禁与活体走查

- 矩阵：studio 678/678（+4）· core 930/930 零漂移 · gate:ts 八步全绿 ·
  contract-diff 55/0（gate 内复跑确认）。
- 走查（walkthrough-env node 腿 + IAB，713 同标准）：
  - **plan**：API 200 全 5 键 → 落盘 intent.md/plan.json/context.json/trace.json
    四件；二次调用 persisted plan 复用（幂等无重写=699 进度链语义活体）。
  - **compose**：**UI「组装」点击实时产出 context/trace 更新**（mtime 实证）
    + API 200 九键（ruleStackPath 保形空串）。
  - **approve 审改流**：手工置 ready-for-review → UI 行徽标「待审核」→
    **「全部通过 (1)」按钮出现 → 点击 → API 读回 approved**（自 688 号起
    首次在 node 回退腿可达）。
  - **reject 快乐路径**：造第 2 章 + snapshots/1（顶层 current_state.md/
    pending_hooks.md + state/ 四件套——prepareStateRestore 双层结构实测锚定）
    → POST reject 2 → 200 `{rolledBackTo:1, discarded:[2]}` → 章文件实删、
    index 回 [1]（与 Rust reject_chapter 全同构）。
  - 走查小坑两则：①IAB alert() 反馈抓不到（runBookAction 成功走 alert 对话
    框，IAB dialog 时序不稳）——产物 mtime/API 读回替代断言；②UI 断言先查
    i18n 真实文案（「全部通过」≠「批准」，搜索词错会误判按钮缺失）。

## 六、教训

1. **strict schema 自毁键第四例**（ChapterMeta.status）——写面新增键必须
   先审计「谁会读这个文件」；Rust 模型是真源，TS 可选键扩展是收敛方向。
2. **护栏与内核分层移植**：fork 公共壳（护栏=契约面）逐字，settle 内核
   （实现面）用 2.0 进化件——护栏双端同构让 contract-diff 探针打到同护栏
   500 对称（比快乐路径更稳定可信的对称证据）。
3. **据实切批**：consolidate 依赖闭包评估出 ~700 行三 util 连片——硬塞本批
   会牺牲验收链完整性；留守并在域表注释批次去向。
4. mock 是测试基建不是产品码——但**改后必须全量回归**（块级作用域引用
   错误让全部 LLM 面 500，靠 directions 状态翻转当场抓获）。
5. fixture 播种用官方初始化函数（createInitialRuntimeState）不手写 JSON
   ——schema 演进随真源走。

## 涉及文件

- packages/core/src/pipeline/runner.ts（三方法 + 两类型）
- packages/core/src/models/chapter.ts（CHAPTER_STATUSES + 可选 status 键）
- packages/core/src/index.ts（CHAPTER_STATUSES 导出）
- packages/studio/src/api/server.ts（五路由）
- packages/studio/src/__tests__/chapter-pipeline-recycle-routes.test.ts（新增 4 测）
- scripts/walkthrough-fixture.mjs（runtime 四件套补播）
- scripts/walkthrough-mock.mjs（submit_chapter_memo 分派支 + 作用域修复）
- scripts/engine-contract-diff.mjs（SHARED_FACE +6 / FORK_ONLY −5）
