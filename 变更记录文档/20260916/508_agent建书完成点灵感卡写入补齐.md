# 508 号：agent 建书完成点灵感卡写入补齐——回退端全路径覆盖

日期：2026-09-16　分支：develop　基线：4662a1ff（507 号）

## 选题与实现

498 号把导演灵感卡写入接在了 `POST /books/create` 的完成回调（REST 路径）；活体排查发现 ChatPage 确认建书的现役路径走 **POST /api/v1/agent**（agent 链内执行 createBook 工具），其完成点另有 **两处**（同名完成逻辑、不同分支），灵感卡写入缺失 → 回退端 agent 建书依旧不出灵感卡。

本号在两处 agent 建书完成点（`resolveCreatedBookIdFromDetails` 确认后、`book:created` 广播前）补同一写入：`writeDirectorInspirationCard(root, createdBookId, premise)`，premise 取 `book.title`（book config 读取失败时回退 createdBookId），try/catch 不阻断。至此回退端灵感卡写入**三完成点全覆盖**（REST create + agent 两分支）。

## 验证

- studio tsc 0 错；studio 生产构建通过；全量 `pnpm -r test`：core 2113 + studio 886 + cli 218 = 3217 全绿；
- Rust 侧 500 号已在建书链镜像同语义（引擎直建路径），agent 链内建书（Rust 引擎）沿用 500 号之前的语义——两引擎在各自现役路径上行为一致。

## 改动

- `packages/studio/src/api/server.ts`：agent 建书两处完成点补灵感卡写入。

## 后续

- Xcode 许可解除后：481 号全量 cargo test（含 489/500 号 4 个新单测）、duel、bench:gate、resync Rust 侧残余评估；
- 三库种子 canonical 决策已在 505 号证实可撤销；npm 余 2 条上游钉死项滚动跟踪。
