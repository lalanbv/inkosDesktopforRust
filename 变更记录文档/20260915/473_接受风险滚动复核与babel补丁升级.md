# 473 号：467 接受风险滚动复核——@babel/core 补丁升级修复（ LOW 出清）

- **日期**：2026-09-15
- **类型**：chore(deps) + docs —— 接受风险滚动复核循环（1 条修复出清 + 4 条维持备案）
- **关联**：467 号（audit:npm 与 8 条生产告警）、454 号（--dev 盲区源头）
- **提交**：pnpm-workspace.yaml + scripts/audit-npm.mjs + pnpm-lock.yaml + 本记录

## 1. 滚动复核结论（5 条逐条）

| 模块 | 上游状态 | 处置 |
| --- | --- | --- |
| @babel/core (low) | 补丁 7.29.1 在 `^7.28.6` 去重范围内可达 | **override `^7.29.1` 修复**（实测落 7.29.7），移出白名单 |
| brace-expansion (moderate+high×2) | 补丁仅 5.x；运行链 minimatch@5 需 2.x | 维持接受（467 理由不变） |
| fast-xml-parser (moderate) | 补丁 5.7+；pi-ai 0.67.1 钉 4.x | 维持接受 |
| postcss-selector-parser (low) | 补丁 7.1.3+；shadcn dev-only 钉 | 维持接受 |
| @ai-sdk/provider-utils (low) | **ai 最新 6.0.283 仍钉 4.0.24**（<4.0.33）| 维持接受（上游阻塞，滚动跟踪） |

白名单同步：@babel/core 条目移除并注明「已修复，若回归会重新被拦截」——白名单保持与实际接受集精确一致。

## 2. 门禁

- `node scripts/audit-npm.mjs` exit 0：advisory 合计 10（babel 告警消失），白名单放行 10 拦截 0。
- 依赖变更回归：studio 110 文件 875 用例全绿 + core 245 文件 2112 用例全绿 + tsc 净 + studio dist/build 重建。
- Rust 零改动。

## 3. 遗留

- **待推送：433–473 共 41 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- vitest 4.x/5.x 大版本迁移（mocker moderate 唯一根治路径）评估为独立工程周期，不在安全滚动内强推。
- 下一个编号自 **474** 起。
