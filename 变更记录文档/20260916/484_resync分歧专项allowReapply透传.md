# 484 号：resync 分歧专项首批——allowReapply 透传，回退端全链打通

日期：2026-09-16　分支：develop　基线：026f4411（483 号）

## 选题与定位

483 号立案的 resync 架构分歧专项首批：483 号遗留的 `duplicate summary row for chapter 1` 失败，活体抓栈定位到精确应用点——

```
WriterAgent.saveChapter (writer.ts:673)
→ resolveRuntimeStateArtifactsForOutput → buildRuntimeStateArtifacts（无 allowReapply）
→ applyRuntimeStateDelta → state-reducer 防重放守卫拒绝
```

**定性**：resync 语义 = 幂等重推导（同章 delta 对已含该章摘要的真相文件重放），但 saveChapter 链未透传 `allowReapply`，重放被 state-reducer 防重放守卫拒绝。settle 阶段已传 `allowReapply: true`（resync 调用点 2779 在位），唯持久化阶段的二次应用断链——非架构不可平替，是**一处参数断链**。483 号"架构分歧需专项"的备案范围据此收窄：结构化状态机本体与 Rust 直写语义在补齐透传后行为一致（幂等重放全链可过）。

## 修复

- `packages/core/src/agents/writer.ts`：`saveChapter` 增第 5 参 `allowReapply`（默认 false，其余 4 个调用方行为不变）；`resolveRuntimeStateArtifactsForOutput` 同步透传至 `buildRuntimeStateArtifacts`；
- `packages/core/src/pipeline/runner.ts`：resync 持久化调用传 `true`（注明 484 号语义依据）。

## 活体验证（生产形态冒烟全链打通）

fixture 套件对 TS 服务器**完整通过**：resync/1 → `{"status":"ready-for-review"}`（此前 duplicate 崩点）→ write-next 真跑第 2 章（run-log 6→14，planner/writer/settler/审稿全链）→ 第 3 章压缩留痕工件 → promises timeline 4 条 kinds 齐全（worldview/suspense/emotion/artifact）→ run-log 断言过。**Node 回退端自 444 号后首次端到端全链可用，walkthrough 一致性套件双端通用。**

## 门禁

- 全量 `pnpm -r test`：**core 2113 + studio 906 + cli 218 = 3237 全绿**；core build 0 错；studio tsc 0 错；
- cargo 链接仍被 Xcode 许可阻断（**481 号欠账待用户 `sudo xcodebuild -license accept`**）。

## 后续

- resync 备案收窄后的残余：Rust 侧 resync 是否引入结构化状态机（对齐 TS 精度）——需 Rust 门禁恢复后评估，暂维持双端行为等价即可；
- npm 接受风险余 8 条滚动跟踪；cargo test 欠账待许可解除。
