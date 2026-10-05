# 642 号：write-next 可停性双端对齐——直连端点补 AbortController 消费（假停止根治）

日期：2026-10-05
提交：本次（develop）
前置：640 号备案「write-next 受理窗口另案」；勘察坐实的双端契约分歧（abort 端点对 write-next：Rust 可停 / TS no-op）

## ①缺陷语义

abort 端点对 write-next 任务双端行为分裂：Rust 侧 write-next 注册进 `active_confirmed_tasks`（write_next_route.rs:107-112）且管线在 5 处 `check_aborted` 安全点轮询——abort 真停（既有 e2e `confirmed_write_next_abort_mid_draft_stops_at_safe_point` 锁死）；TS 侧直连端点（POST /books/:id/write-next）只进独立 `activeWriteNextTasks` 集合（2640 旧注释自认「混入会假停止，独立集合让停止保持诚实的无任务可停」）——`findRunningTaskController` 不查该集合，abort 端点返回 `aborted:false` 纯 no-op，长流/卡顿时用户停止无效。这是「宁可诚实承认停不掉」的旧裁决，本号改为根治停得掉（管线感知链早已现成：confirmed 路径同款 `runWithAbortSignal → AsyncLocalStorage → throwIfOperationAborted + provider 层 AbortSignal.any`）。

## ②修复（TS 三点，Rust 零改动）

1. 直连端点构造 `AbortController` 并注册进 **`activeConfirmedTasks` 同张表**（与 Rust 注册形态同构；消费方全查：对账 ：2700 与内存快照直读 :2733 行为中性偏改善，confirmed 任务键为 taskId 与 `write-next-{bookId}` 无碰撞）。
2. `pipeline.runWithAbortSignal(controller.signal, () => writeNextChapter(...))` 包裹——管线安全点与 provider 层立即生效（实例 pub 方法，零 core 面改动）。
3. catch 分支按 `signal.aborted` 判定用户停止 → 中性文案 **「写作已按您的要求停止。」**（与 Rust write_next_route:154 字面同形）落快照 error 终态 + `write:error` 广播；finishCheckpoint 双表注销（对齐 confirmed finally 双表形态）。
4. `activeWriteNextTasks` 退居对账面（重启对账的第二个存活来源，语义保留）；2640 注释改写。
5. abort 端点本身零改动（findRunningTaskController 查同表自然命中）。

## ③红绿可证伪

server.test.ts 新测试「aborts a direct write-next task through POST /abort」：直连端点 + sessionId 落检查点 → running 快照 → abort → **`aborted:true`（旧码 false=红验证断言点）** + `pipelineAbortSignals.at(-1).aborted===true`（signal 直达管线）→ mock 管线抛 AbortError → 快照 error 且 `error === "写作已按您的要求停止。"`（中性文案）→ 二次 abort 回 `aborted:false`（双表注销的诚实态）。stash 旧码红（`expected {aborted: false} to match {aborted: true}`）→ 恢复绿。MockPipelineRunner 补 `runWithAbortSignal` 记账桩（与 runWithAgentContext 同款）；既有 confirmed 路径 abort 测试（:5151）原样通过=旧通道零回归。

## ④插曲：并行会话撞号让号（本日第五实例）

开编号三查时 641 空闲；实现中途 stash 输出显示 HEAD 已移至 `a0d118af`——并行会话刚落 641 号（内容为 640 HEAD 二进制新鲜度补验：坐实 640 门禁活体腿跑的是陈旧 debug 二进制、重建后 smoke 双腿复跑全绿=640 无遗留债，反向证实 testgate `cargo test` 不编译 bin 目标）。按 534/536/637 先例让号改 **642**，收口前三查确认 642 空闲后写档。

## ⑤门禁

gate:ts 七步全绿（build 19.5/typecheck 18.4/test 80.5 含新测试/audit:npm 2.9/smoke 34.1/差分 40.1 61 端点 0 分歧/epub 34.2）。本轮 Rust 零触碰（纯 studio server.ts + server.test.ts），testgate/clippy/duel/bench 按 535 先例裁剪备案（Rust 面 = 8ece52bd 全绿 HEAD 同一份代码，且 641 号刚以重建后二进制补验活体腿全绿）；server.test.ts 183/183。

## ⑥备案

- 直连端点无 sessionId（checkpoint=null → controller=null → signal=undefined）行为零变化（runWithAbortSignal undefined 直通，与 confirmed 无 signal 路径同构）。
- Rust 侧 confirmed_write_next_abort 测试已锁死管线中止契约；直连端点注册同表共用同一管线实现——双端对齐的 Rust 验收面即既有测试，不重复建探针。
- StudioTaskExecutionStatus 无 cancelled 枚举：用户停止落 error 终态 + 中性文案（与 Rust 同形），枚举扩展（前端任务卡区分「已停止/失败」徽标）留待 UI 需求真实出现。

## ⑦教训

1. **「诚实承认停不掉」是半程解**：2640 旧注释把假停止问题用「不宣称」规避了 API 层谎言，但用户侧的「停止无效」依旧——正确终点是让停止真的生效（管线通道现成时，修复成本远低于维护双集合语义的成本）。
2. **双端行为分歧清单要有编号出口**：本分歧在勘察（640 号前期）中作为事实记录、640 备案「另案」——若非备案挂号，会随时间沉淀为「双端本来就不同」的默许漂移。
3. **并行会话让号纪律再次生效**（第五实例）：开编号三查只能保证当时空闲，stash/提交输出里的 HEAD 变化是撞号的实时信号——发现即让，不与其竞争写档。
