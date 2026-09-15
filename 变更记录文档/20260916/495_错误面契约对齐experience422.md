# 495 号：错误面契约对齐——experience 非法载荷 500→422（对齐 Rust typed-extractor）

日期：2026-09-16　分支：develop　基线：ade1edbe（490 号）

## 选题与定性

490 号写入面对照遗留的备案清偿：experience 端点非法载荷 node=500 vs rust=422。定性：Rust 端 `PutExperienceBody` 为 typed Json extractor——条目 zod 校验失败由 axum 以 **422 Unprocessable Entity** 整体拒绝；TS handler 将 entries 透传给 `core.mergeExperienceEntries`（内部 zod 抛错）未捕获 → 冒泡成 **500**。双端拒绝语义一致、状态码不同。

## 修复

- `packages/studio/src/api/server.ts`：experience PUT 捕获 `mergeExperienceEntries` 校验错 → **422** `{error}`（对齐 Rust 状态码；响应体沿用 TS 错误约定 `{error}`）。

## 差分器扩展

- `scripts/engine-contract-diff.mjs`：新增**错误面探针**——非法载荷双端断言状态码一致且均为 422（此前该面零覆盖；红绿：修复前 node=500 rust=422 分歧，修复后双 422）。

## 验证

- 差分器：**35 断言（8 写入 + 1 错误面 + 26 读取）0 分歧，exit 0**；
- 全量 `pnpm -r test`：core 2113 + studio 906 + cli 218 = 3237 全绿；studio tsc 0 错；
- cargo 链接仍被 Xcode 许可阻断（481 号欠账 + 489 号 2 新单测持续排队）。

## 改动

- `packages/studio/src/api/server.ts`：experience PUT 校验错 500→422；
- `scripts/engine-contract-diff.mjs`：错误面探针。

## 后续

- Xcode 许可解除后：481 号全量 cargo test（含 489 号 2 新单测）、duel、bench:gate、skills/genres 移植评估；
- 三库种子 canonical 化待产品拍板；npm 接受风险余 2 条上游钉死项滚动跟踪。
