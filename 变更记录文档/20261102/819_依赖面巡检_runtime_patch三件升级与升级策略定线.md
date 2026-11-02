# 819 号：依赖面巡检新维度——runtime patch 三件升级 + 升级策略定线

日期：2026-11-02。前置：817 号巡检（上游/安全/性能三面）；本轮补依赖面维度。

## 一、巡检快检

上游 `fetch --prune` 复跑仍 0 未吸收（无新推送）；RustSec 双 0、npm audit ✓（本轮全电池复核）。

## 二、依赖面盘点（新维度）

`pnpm outdated -r` 按 semver 级别×运行时分类：
- **MAJOR RUNTIME 13 件**：`@earendil-works/pi-*` 0.87→1.4（agent 框架）、`@hono/node-server` 1→2、`zod` 3→4、`undici` 6→8 等——**上游 2.0 基底管辖**：单方升 major 会脱钩上游依赖线、污染后续合并。策略定线：**跟随上游**（上游升则随合并带入），本仓不单方动。
- **MINOR/PATCH dev**：playwright/tailwindcss/vitest 等——不随包发布，滞后无害，择机批量。
- **RUNTIME patch 3 件**：`hono` 4.13.13→4.13.16（HTTP 服务面）、`jszip` 3.10.1→3.10.2（epub/导出）、`@sinclair/typebox` 0.34.49→0.34.52（核心 schema）——本轮升级。
- cargo `update --dry-run`：117 项 semver 兼容更新可刷——**登记择机独立轮**（锁文件大刷新需 bench 复跑护航）。

## 三、升级落地

`pnpm -r update hono jszip @sinclair/typebox`：
- lockfile 差异**精确限于三件** + `@hono/node-server` 的 hono peer 重钉；package.json 随升（typebox `^0.34.49→^0.34.52`、hono `^4.7.0→^4.13.16` 下限抬升；jszip 为精确钉 `3.10.1→3.10.2`）。
- `pnpm -r update` 输出的 fdir/picomatch unmet-peer 告警为**既有**（frozen-lockfile 对照 0 条；lockfile diff 无 fdir/picomatch）——非本次引入，另案登记。

## 四、验证（jszip=export 面 → export-epub-smoke 为关键验证）

gate:ts 八步全绿（build/typecheck/test/**audit:npm**/rust-bin/**node-fallback-smoke**/engine-contract-diff/**export-epub-smoke**）；权威 pnpm -r test core 180/988 + studio 137/725 + cli 23/81；cargo:testgate 47 目标 1980 + src-tauri 584 + RustSec 双 0（rust 零触碰照跑）；clippy 双 0。diff 71/1 维持。

## 五、登记（695 工项余项）

- **数值分协议扩展（上游面）**——唯一实质余项（overallScore/averageScore/scoredChapters 与 issueCount 值差同源）。
- cargo 锁文件 117 项兼容刷新：择机独立轮（testgate + bench:gate 双护航）。
- TS MAJOR 依赖跟随上游；dev minor/patch 批量择机。
- fdir/picomatch unmet-peer（既有）：上游基底传递依赖形态，跟随上游。

## 六、教训

1. **依赖升级的管辖权分层**：MAJOR 属上游基底（单方升级=脱钩依赖线）；patch/minor 内的本仓自有包才可单方动——「跟随上游」是 fork-and-own 的依赖策略。
2. `pnpm update <名>`（不带 --latest）只在声明范围内刷新——精确升级的正确姿势；`--latest` 会跳 major。
3. 升级前 `git stash` + `--frozen-lockfile` 对照可甄别「既有 vs 新引入」的 peer 告警。
