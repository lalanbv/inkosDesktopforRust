# 503 号：pi-ai 0.73 落地后的双引擎活体回归——通过

日期：2026-09-16　分支：develop　基线：282fa080（502 号）

## 背景与执行

499 号 pi-ai 双 pin 0.67.1→0.73.1 落地时的验证为单元/编译级（core 2113 + build + tsc）；**经 pi-ai 的真实 LLM 链（fallback 服务器 write-next/agent 链）未做活体复验**。本号补齐：重跑双引擎活体套件。

## 结果

- `node scripts/node-fallback-smoke.mjs`：**exit 0**——node 腿（TS 服务器经 pi-ai 0.73 的 resync/write-next 全链）+ rust 腿全过；
- `node scripts/engine-contract-diff.mjs`：**exit 0**——37 断言（8 写入 + 2 DELETE + 1 错误面 + 26 读取）0 分歧；SSE 事件名集合双端一致（6=6）。

**结论：499 号 pi-ai 0.73.1 落地经真实 LLM 链活体验证闭环**——chatCompletion/流式消费/错误分类在 mock 全链下行为与 0.67 一致。本号零代码改动（纯回归验证，470 号门禁复验同型）。

## 门禁

- 全量 `pnpm -r test`：core 2113 + studio 886 + cli 218 = 3217 全绿（延续）；
- cargo 链接仍被 Xcode 许可阻断（481 号欠账 + 489/500 号 4 个新单测持续排队）。

## 后续

- Xcode 许可解除后：481 号全量 cargo test（含 489/500 号 4 个新单测）、duel、bench:gate、resync Rust 侧残余评估；
- 三库种子 canonical 化待产品拍板；npm 余 2 条上游钉死项（fast-xml-parser/provider-utils）随 major 专项滚动。
