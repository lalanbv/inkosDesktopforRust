# 674b 号：Rust 腿审改循环日志管道对齐修复（on_log 管道零新依赖）+ 三分支镜像活体

日期：2026-10-06。类型：fix(engine-rs)。前置：674 号备案「Rust 管线日志全盲（tracing-subscriber 依赖因 bench +45~242% 副作用回退定案）」+「三分支未逐一注入」。

## 一、日志管道对齐修复（真实缺陷修复）

**根因精确化**：write_next.rs 900-902 的审改循环 callbacks 把 log_stage/log_warn **硬编码为 tracing:: 宏**，未接 `config.on_log` 管道——而 TS 同构位（runner.ts 2217）是 `config.logger?.info` 注入。bin 无 subscriber → tracing:: 宏 no-op → **Rust 腿审改循环日志双通道全盲**（SSE log 事件 + inkos.log 落盘都缺）。

**修复**（write_next.rs，零新依赖）：审改循环 callbacks 接 `config.on_log.clone()`（books_routes/agent_production 既有 SSE+log_file 管道）——warn 级 log_warn/info 级 log_stage 双双经 on_log 广播+落盘；tracing:: 宏保留（双通道：有 subscriber 的部署面[如未来 src-tauri]仍有 tracing 输出）。

## 二、Rust 腿三分支镜像活体（叠加 673 patch-only+达标退出）

| 分支 | 注入 | Rust 行为实录 |
| --- | --- | --- |
| **未产出** | 45,91+PASSTHROUGH | `修复轮次 1 未产出新内容，退出循环`（WARN）——直通回传原文→reviser 判全等 |
| **未净提升** | 45,47,91+retries=2 | `修复轮次 1 未净提升（45 → 47），退出循环`——修订被丢弃落初稿 2405 |
| （对照）达标退出 | 45,91 | 673 已证：ready-for-review/2407=PATCH1 应用版 |

三分支日志全部经 on_log 管道可见（inkos.log 落盘+SSE 广播）——**Rust 管线日志可见性轻量方案实证成功：零新依赖，复用既有 on_log 管道**（674 回退实验的正面解）。

## 三、验证

- testgate 1926+584 全绿（含 engine-rs bin/callbacks 改动的 45 目标）；clippy 双 0。
- 走查活体：inkos.log 含审改循环日志（on_log→log_file 落盘链）；第 2 章 PATCH 应用版落盘。
- Rust 源码改动面=write_next.rs callbacks 一处+新依赖零。

## 四、教训

1. **callbacks 硬编码 tracing:: 而非接 config 管道=TS/Rust 日志对齐缺口**：TS 侧 logStage 走 config.logger?.info（SSE+落盘），Rust 侧同构位硬编码 tracing:: 宏——bin 无 subscriber 时全盲。对齐审计要覆盖「日志出口」而不只是行为。
2. 674 回退实验的正面解就此落地：**config.on_log 管道是审改循环日志的正确出口**（SSE+落盘双通道既有、零新依赖、bench 零影响）。
3. 闭包捕获链（Arc clone per-closure vs 共享借用）的编译迭代——Arc<dyn Fn> 'static 约束要求 move 捕获克隆，两闭包各持一份 Arc 克隆是标准形态。
