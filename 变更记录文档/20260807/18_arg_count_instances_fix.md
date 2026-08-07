# 变更记录：exec_command 参数数量上限 + instances 限制移除

## 日期
2026-08-07

## 变更类型
feat(security): exec_command MAX_ARG_COUNT=64 + WASM instances 上限修正

## 1. exec_command 参数数量上限
`const MAX_ARG_COUNT: usize = 64`；args.len() > 64 → ExecutionFailed。
防多个小参数耗尽 ARG_MAX（与 MAX_ARG_LEN=4096 互补：前者防总量，后者防单个）。

## 2. WASM instances 限制移除
`.instances(1)` 导致 Component Model 内部 WASI subcomponents 占用额外槽位，
第二次 execute 报 "instance count too high at 2"。
memory(64MiB) + table(1M) 双层守卫已充分；instances 无需额外限制（Store 生命
周期 = 一次 execute，天然有界）。

## 累计测试数
519 passed, 0 failed（全量）
