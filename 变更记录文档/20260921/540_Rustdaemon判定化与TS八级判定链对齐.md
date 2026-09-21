# 540 号：Rust daemon 判定化——八级判定+记债链对齐 TS scheduler

日期：2026-09-21
类型：fix(engine)+fix(studio)——行为对齐（537 号立案专项①清偿）
提交：本文件同批路径限定提交

## 背景与立案

537 号零引用扫描把 `QualityVerdict::creates_debt/continues_pipeline`、`resolve_quality_verdict`、
`resolve_governance_policy` 定性为「TS 活对齐面、Rust daemon 裁剪面」，立案 daemon verdict 化。
本轮把 72 号「精简生命周期」的裸计数暂停（`failures >= pauseAfterConsecutiveFailures` 两态、
失败不记债）升级为 TS `Scheduler.handleAuditFailure`（G3/337 号）同构的八级判定链。

## 双端对照结论（实现前考古）

TS 活体链（scheduler.ts + studio server daemon/start）：

1. `writeOneChapter`：成功 → 清零计数+日计数+检测环（detection 在 onChapterComplete 之前）；
   审计未过 → `handleAuditFailure` 后仍回调 onChapterComplete；异常 → onError+判定链(章号 0、无维度)。
2. `handleAuditFailure`：计数先自增 → 维度聚类（≥3 → diagnostic-alert）→
   `failures <= maxAuditRetries` 直接重试（不判定）→ 出窗八级判定
   （输入全零，`consecutiveDebts = failures-1`）→ `createsDebt` 记债入账
   （`MemoryDB(join(root,"books",bookId))`，replan-required→open 其余→deferred，
   debtId=`debt-{book}-{chapter||failures}-{Date.now().toString(36)}`）→
   local-patch/patchable-gap 返 false（下轮降温重试）→
   `continuesPipeline`（defer-and-continue）清零计数返 true 续写下章 →
   否则暂停+pipeline-error webhook（data 带 reason/consecutiveFailures/verdict）。
3. **暂停阈值 = `governance.maxConsecutiveDebts`**（book.governance 覆盖缺省 3）；
   `qualityGates.pauseAfterConsecutiveFailures` 在 TS scheduler 活体**零消费**（schema 残留面）。
4. TS server daemon/start **不传 governance**（ProjectConfigSchema 无此段）——活体=book 级
   governance+硬编码默认 gates {2,3,0.1}；processBook 重试读的是自增后计数，重试本身
   走完整判定链（重试失败也计数）。

Rust 旧态缺陷清单（本轮全部清偿）：

| # | 缺陷 | 影响 |
| --- | --- | --- |
| 1 | 无判定链：裸计数暂停、失败不记债 | quality_debts 账本 daemon 路径永不入账（读取端点 1666 行形同虚设） |
| 2 | quality-first 书无早停；defer-and-continue 缺席 | 治理方案（book.governance）完全未消费 |
| 3 | 暂停阈值用 `qualityGates.pauseAfterConsecutiveFailures` | 与 TS 活体（governance.maxConsecutiveDebts）异源 |
| 4 | 内联重试失败不计数 | 默认 gates 下 TS 第 3 次失败暂停、Rust 第 6 次尝试才暂停（时序漂移） |
| 5 | 重试成功不记日计数 | maxChaptersPerDay 可被击穿 |
| 6 | detection 在审计未过章也跑 | TS 仅成功审计后跑（111 号注释本就说"成功审计后"） |
| 7 | webhook data 缺 verdict 字段 | 订阅方无法按判定分级 |
| 8 | **qualityGates 读顶层键**（from_raw） | TS schema 嵌 `daemon.qualityGates`——用户配置被静默忽略 |
| 9 | **`QualityGates` derive 全零 Default** | 缺配置 daemon 落 {0,0,0.0}：首败即暂停、永不重试（测试红灯坐实） |

## 改动

- `engine-rs/src/server/ops_routes.rs`：
  - `process_book` 重构为 TS 三函数同构：循环体只做检查/冷却/重试编排；
    `write_one_chapter_governed`（TS writeOneChapter：温度步进、成功清零+日计数+检测环、
    审计未过先判定链后广播、异常走判定链章号 0）；`handle_audit_failure`（八级判定链全量）。
  - `unix_millis_base36()`（TS `Date.now().toString(36)` debtId 时间段等价）。
  - `from_raw` qualityGates 读取位纠偏顶层键 → `daemon.qualityGates`；模块 doc 头
    72 号「暂缓」过期清单更新（111 号已补环、540 号补判定链）。
  - 新增 4 判定链测试（重试窗口/replan-required 记 open 债+暂停/defer-and-continue 记
    deferred 债+清零续写/quality-first 暂停不记债）+daemon_config 嵌位断言。
  - `DaemonConfig.quality_gates` 字段 doc 备案：`pauseAfterConsecutiveFailures` 为 schema
    残留面（TS 同构零消费），真实阈值=governance.maxConsecutiveDebts。
- `engine-rs/src/models/project.rs`：`QualityGates` 摘除 derive `Default`，手动 impl
  返回 serde 字段默认 {2,3,0.1}（与 TS `QualityGatesSchema.default` 同源），缺陷 9 根治。
- `packages/studio/src/api/server.ts`：daemon/start 接线
  `qualityGates: currentConfig.daemon.qualityGates`——schema 既有面此前未传
  （Scheduler 落硬编码默认）；Rust 侧同位读取后双引擎门控配置面一致。

## 门禁（全绿）

1. engine-rs cargo test 全量 **1811**（1807+4）exit=0（落盘统计）
2. clippy:gate 双 crate 0 告警（拦截 1 处 `get().is_none()`→`!contains` 已改）
3. verify:engine-bindings **186** 导出全绿
4. duel 真跑（INKOS_DUEL=1）**10/10** @42.00s
5. gate:ts 7 步：typecheck ✓ / 三包 test **3236** ✓（core 2124+studio 894+cli 218；
   首跑 studio 挂=531 号已知高负载时序噪声，单跑与 `pnpm -r test` 复跑全绿）/ audit:npm ✓ /
   build ✓ / node-fallback-smoke 双腿 ✓ / engine-contract-diff ✓ / export-epub-smoke ✓
6. bench:gate 豁免备案：daemon 写循环非 bench 热路径（bench 面为聊天/prompt 链，本轮零改动）

## 教训

1. **derive Default 与 serde 字段默认是两条默认值管道**——TS `.default()` 移植时只配
   `serde(default=)` 而留 derive `Default`，缺配置路径走的是全零而非 schema 默认；
   守门的是「缺配置行为」测试而非「带配置解析」测试。
2. **暂停阈值类参数要核对活体消费位**：`pauseAfterConsecutiveFailures` 在 TS 是 schema
   残留（零消费），真实阈值在 governance——按 schema 直译移植会造出语义等价假象
   （默认值恰好同为 3）而配置源分叉。
3. **重试是否走完整判定链决定失败计数口径**：TS 重试=完整 writeOneChapter（重试失败
   也计数），Rust 旧内联重试不计数——默认值下表现为暂停时序相差一倍（3 失败 vs 6 尝试）。
4. 写档前撞号复核再次生效：开号时 539 空闲，写档前并行会话已落 00199dce 占 539
   （规划文档），本号全程改用 540，代码注释 16 处同步改号（534/536 号同类缺陷预防）。

## 编号警戒（更新）

- **539 已被并行会话占用**（00199dce，Pi 对标专项 v6 规划）；下一号自 **541** 起。
