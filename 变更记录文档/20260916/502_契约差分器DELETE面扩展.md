# 502 号：契约差分器 DELETE 面扩展——写后删后读双端等价

日期：2026-09-16　分支：develop　基线：9c54a702（498 号）

## 选题与实现

契约差分器从写后读对照扩展至 **DELETE 面**（双引擎 DELETE 路由此前只有路由存在性核对，无行为对照）：

1. **experience 单条删除**：PUT 两条经验条目（exp_del/exp_keep）→ 双端 DELETE exp_del → 断言状态码一致且 200；
2. **asset-library 单资产删除**：PUT 一个 world-sample 探针资产 → 双端 DELETE → 断言一致 200；

删除后的持久化等价由随后执行的 26 端点 GET 归一化深比对面自动覆盖（experience 仅剩 exp_keep、world-sample 回到种子态、nextChapter 等在豁免/易变键规则下判定）。

## 验证

- 差分器：**37 断言（8 写入 + 2 DELETE + 1 错误面 + 26 读取）0 分歧，exit 0**；
- 全量 `pnpm -r test`：core 2113 + studio 886 + cli 218 = 3217 全绿（本号零产品代码改动）；
- cargo 链接仍被 Xcode 许可阻断（481 号欠账 + 489/500 号 4 个新单测持续排队）。

## 改动

- `scripts/engine-contract-diff.mjs`：DELETE 面对照阶段（experience 单条 + asset-library 单资产）。

## 后续

- Xcode 许可解除后：481 号全量 cargo test（含 489/500 号 4 个新单测）、duel、bench:gate、resync Rust 侧残余评估；
- 三库种子 canonical 化待产品拍板；npm 余 2 条上游钉死项滚动跟踪。
