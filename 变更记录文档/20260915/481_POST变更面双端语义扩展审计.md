# 481 号：POST 变更面双端语义扩展审计——61 端点全一致 + write-next 失真注释纠偏

日期：2026-09-15　分支：develop　基线：458ac7f3（480 号）

## 选题与审计

延续 480 号方法论，把双引擎 body 语义对照覆盖到 **POST 变更面**（TS server.ts 全部 61 个 POST 端点）：逐端点提取 TS handler 的 body 键读取，与 Rust 对应 handler 的 typed struct（含 serde rename_all）或显式 `get()` 读取逐键对照；两端路由面先求交确认镜像完整（418 号路由级对照的复核）。

## 审计结论：body 语义全一致（唯一分歧=失真注释，已纠偏）

覆盖：series-backfill extract/apply、deconstruct（topCharacters camelCase ✓）、director directions、hybrid-search、roster/confirm（targetName/chapter 可选 ✓）、asset-library import/preview、adopt（refs+language ✓）、style-profiles、workspace inspiration {brief}、版本 restore、write-next/plan/consolidate/draft/compose、repair-state（双端均无 body ✓）、foundation/revise（feedback 必填 400 ✓）、services test（apiKey/baseUrl/apiFormat/stream ✓）/import-env、sessions create（bookId/sessionKind/playMode/sessionId ✓）、agent、project/language、audit、revise {mode,brief}、export-save {format,approvedOnly}、genres create/copy、detect/detect-all、rewrite/resync {brief}、style analyze/import/import-chapters（text/sourceName/splitRegex ✓）、canon import 三件（fromBookId/filePath/filename/dataUrl ✓）、fanfic/spinoff/imitation init（键集与必填镜像，模块头注释在案）、radar 三件、translations 五件（batchSize/maxTokens/segmentMaxChars ✓）、story-graph delta {delta}、node image {size}、backup import（444 号已测）。

**唯一发现（已修）**：`write_next_route.rs` 两条陈旧失真注释——「context 非空时替换自动 plan」与「TS 回退端同名键收下但忽略」。实测两侧实现完全同构：`prepare_write_input`（Rust）与 `resolveGovernedPlan`（TS）均为**非空 context→planner 携 external_context 重跑 governed plan；空/缺→复用持久化 plan**（450 号节拍透传在双端语义一致，回退端无缺失）。注释失真会在维护时误导排查方向（本次审计即被误导一次）——注释级修复，零行为变更。

## 门禁与环境异常（须用户处理）

- `pnpm clippy:gate` 双 crate 0 告警 ✓；`cargo check --all-targets` 全绿 ✓；
- **全量 `cargo test` 被环境阻断**：链接器报 `exit status 69`——Xcode 许可未同意（机器级问题，疑 Xcode 自动更新所致；clippy/check 不链接故不受影响）。**请用户在终端执行 `sudo xcodebuild -license accept` 后告知，下一轮补跑全量 cargo test**。TS 侧本号零改动，workspace 3250 全绿状态延续 479 号。

## 改动

- `engine-rs/src/server/write_next_route.rs`：两条注释纠偏（对齐 resolveGovernedPlan 真实语义，注明 481 号）；
- 变更记录本篇。

## 后续

- 用户接受 Xcode 许可后补跑全量 cargo test（本号唯一欠账）；
- vitest 5.0.0 迁移评估、npm 接受风险 4 条滚动跟踪维持挂起。
