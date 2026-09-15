# 509 号：统一 TS 门禁入口 gate:ts——验证资产运营化

日期：2026-09-16　分支：develop　基线：79dfe7ce（508 号）

## 实现

本会话积累的验证资产（typecheck/build/test/audit + 三个活体套件）此前需手工逐条执行。本号新增统一入口：

- `scripts/gate-ts.mjs`：顺序编排 **7 步门禁**（typecheck → build → 全量 test → audit:npm → node-fallback-smoke → engine-contract-diff → export-epub-smoke），逐步计时、失败截尾输出 stderr/stdout 尾段、汇总表 + 非零退出码；
- `pnpm gate:ts`（全量）与 `pnpm gate:ts:fast`（--fast 跳过 build 与活体套件，秒级冒烟）；
- `packages/studio/TESTING.md` 门禁清单顶部补 `pnpm gate:ts` 入口。

Rust 门禁（clippy:gate / audit:rust / cargo test / duel / bench）不在本脚本范围——Xcode 许可阻断期间互不影响。

## 验证

- `pnpm gate:ts:fast`：3 步绿（typecheck 15.9s / test 65.6s / audit 1.7s）；
- `pnpm gate:ts` 全量：**7/7 步绿，总耗时约 3.5 分钟**（typecheck 16.1 / test 65.9 / audit 1.9 / build 16.7 / smoke 33.4 / diff 35.0 / epub 33.5）。

## 门禁汇总（本号即门禁）

typecheck 三包双 tsconfig 0 错；build 全链（core/cli tsc + studio vite7+server tsc）通过；测试 **core 2113 + studio 886 + cli 218 = 3217 全绿**；audit 2 条白名单 exit 0；活体套件双引擎一致性全过。

## 后续

- Xcode 许可解除后：481 号全量 cargo test（含 489/500 号 4 个新单测）、duel、bench:gate、resync Rust 侧残余评估；
- 三库种子 canonical 决策已在 505 号证实可撤销；npm 余 2 条上游钉死项滚动跟踪。
