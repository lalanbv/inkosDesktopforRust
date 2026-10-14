# 700 号：全量 gate:ts 八步端到端认证（esbuild override 根因修复 + smoke 契约源自适应）

日期：2026-10-15。类型：fix(deps)+fix(scripts)。前置：699 号（8552b528）。

## 一、承接

689-699 号的验证都是分散实跑（typecheck/vitest/cargo/contract-diff 单项），本号用仓库自有 **gate:ts 八步全量**做端到端认证——build/typecheck/test/audit:npm/rust-bin/node-fallback-smoke/engine-contract-diff/export-epub-smoke，其中后六步的活体腿是合并后首次全跑。

## 二、首轮门禁抓获四红与根因链

首轮 4 红：build / node-fallback-smoke / engine-contract-diff / export-epub-smoke。

**根因一（build ✗，级联源）**：696 号 audit 修复的 37 键 `pnpm.overrides` 中 `esbuild@<0.28: ^0.28.1` 选择器**匹配了 vite 6.4.1 自己的 esbuild ^0.25 依赖**——强提至 0.28 后 `vite:esbuild-transpile` 对 streamdown chunk 报「Transforming destructuring … not supported yet」（esbuild 0.28 与 vite 6.4 的 transpile 管线不兼容；版本对照实证 streamdown/shiki/esbuild 皆无漂移、排除数据面）。**二分定位**：37 键→组A(11)绿→组B(25)红→13 红→8 绿→5 红→esbuild 单键红=定案。**修复**：撤 esbuild 键（其余 36 键保留），esbuild low advisory（Windows dev server 面）入白名单并记录配对理由（升级随 vite bump，到时移除即恢复拦截）。

**根因二（node-fallback-smoke ✗）**：fork 契约断言打在 2.0 TS 腿上（director/task-routing/genre/branch 树=已声明退役面；transcript seq 形状差=异常中断）——**契约源软失败模式**（与 strangler_duel/contract-diff 同语义）：腿级探测 /prompt-packs，2.0 腿的 fork 契约断言失败与异常中断转入软失败记账（照打 ✗ 不计判定），Rust 腿恒严格，TS 恢复 fork 契约后自动严格化。

**根因三（export-epub-smoke ✗）**：epub 在 2.0 腿**功能完好**（下载+214 项结构校验全过），仅响应体形状断言是 fork 契约——契约源自适应分支：2.0 腿放宽形状断言，落盘+结构校验照常。

## 三、终态门禁（本号认证结果）

**gate:ts 八步全绿（exit 0）**：build 14.2s / typecheck 12.7s / test 56.4s（core 881+studio 646+cli 81）/ audit:npm 3.1s（6 接受 0 拦截）/ rust-bin 0.2s / node-fallback-smoke 18.4s / engine-contract-diff 17.7s（三域报告：共享面 8/8 全绿）/ export-epub-smoke 19.4s。

叠加既有实证：cargo testgate 双零漂移（1933+584）、bindings 186、contract-diff 三域报告（fork-only 52/52、上游新增 9/9+2）。

## 四、教训

1. **override 选择器的杀伤半径是「所有匹配依赖」不是「目标包」**：`esbuild@<0.28` 意图修工具链 esbuild，实际命中 vite 的运行时配对依赖——override 必须用精确版本选择器（`esbuild@0.27.x`）或事后活体验证全消费面；「审计修复」本身需要审计。
2. **分散单项绿 ≠ 端到端绿**：11 个提交每号都有门禁实证，但八步从未整体同跑——活体腿（smoke）与构建链的级联关系只有全量门禁能暴露；大变更后先跑全量门禁再继续增量。
3. **契约源自适应要下沉到每个活体消费方**（smoke/gate/fixture 各自探测），调用方 env 传递是脆弱补丁——本号把 fixture 的 2.0 路径改为 `!forkContract` 自适应（与调用方解耦），env 仅保留报告模式对称性控制。
