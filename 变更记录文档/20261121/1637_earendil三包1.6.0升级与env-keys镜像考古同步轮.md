# 1637 earendil 三包 1.6.0 升级与 env-keys 镜像考古同步轮（2026-11-21）

触发 = backlog 备案触发条件「earendil-mz 单包合并/上游升级」点火：`@earendil-works/pi-ai` 等 npm 三包于 2026-11-20 发布 1.6.0（本仓钉定 1.5.2）。性质 = 外部事件驱动的依赖刷新轮，1636 后首次**真实源码变更轮**（终结 1608-1636 二十九轮纯验证序列）。

## 变更面

1. **依赖升级**：`packages/core/package.json` 三包 `@earendil-works/pi-agent-core` / `pi-ai` / `pi-telemetry` 1.5.2 → 1.6.0；`pnpm install` 后传递依赖 `@earendil-works/pi-schema` 同步升至 1.6.0。
2. **镜像考古同步**：`packages/core/src/llm/pi-env-keys.ts`（554 号自持副本）同步至 1.6.0 上游表形态：
   - qwen token-plan 四项改 `string[]` fallback 链（TEAM/INDIVIDUAL 优先、旧区域键兜底）；
   - `azure-openai-responses` 键随上游改名 `azure`（考古实证：0.87.1 原始形态确为 `azure-openai-responses`，上游 0.87→1.5.2 间改名，非 554 号移植笔误）；
   - 补 `typesafe`（0.87 后上游新增）；
   - 补 `github-copilot` 分支（0.87.1 即有，554 号移植遗漏且未备案，本轮按文件头「全表镜像」声明补齐）；
   - 文件头源声明更新（0.87 移植基线 + 1.6.0 考古同步备案）。
3. **测试补强**：`pi-dispatch-env-keys.test.ts` 新增两用例——qwen fallback 链（per-plan 优先/旧键兜底/键名不串扰）+ 表改名与新增键（azure/typesafe/github-copilot），期望值从 1.6.0 上游源派生。

## 上游 1.5.2→1.6.0 变更面定性（npm 包内行为，随升级生效）

- `pi-agent-core`：agent-loop 移除循环内三处静默 abort `break`，改工具执行前统一检查并产 `isError` receipt（#9986「call 不落 receipt」修复）——**本仓零影响**：`@earendil-works/pi-agent-core` 仅 type-only import（AgentTool/AgentMessage），运行时 Agent 来自 `@mariozechner/pi-agent-core` 0.73.1（npm 最新，零工作）。
- `pi-ai`：anthropic `supportsMessageThreads` 判定去 provider 条件只看 baseUrl；工具 JSON schema 透传 `additionalProperties`（agent 工具 schema 正向增强）；openai-completions developer role 判定收紧为 isOpenAI（#10741 其他 host chat template 拒 developer role——对本仓自建 provider 为修复）；constrained-sampling strict mode 对无 type schema 抛错；provider 模型目录数据刷新；env 表上述四处。
- `pi-telemetry`：纯版本 bump 零行为变化。

## 验证证据（@本轮树，fmt 后终态）

1. **core vitest**：189 文件 1087 测试全绿（51.0s），含镜像测试 8/8（新增 2 用例）。
2. **全仓 test**：CLI 24 文件 91 测试全绿（studio 面随十腿 test 腿覆盖）。
3. **gate:ts 十腿全绿**：build 20.6s / typecheck 20.1s / test 106.1s / **audit:npm 3.5s（升级后依赖面复核 0 漏洞+白名单拦截如常）** / rust-bin 0.4s / node-fallback-smoke 35.9s / engine-contract-diff 36.1s / export-epub-smoke 35.0s / work-alternation 5.1s / check-routes 0.1s。
4. **clippy:gate / fmt:gate**：双 crate 0 告警 / 0 漂移；TS 面改动两文件 prettier 格式化后复跑 typecheck（全过）+ 镜像测试（8/8）钉终态。
5. **audit:rust**：本轮 Rust 源零变更，1635 号 testgate（2732 测试+audit 双 0+必红自检活体）证据转移成立。
6. **上游**：upstream/master 0 commits behind。

## 后续

- 下号自 1638 起；出口 = 下一巡检周期（2026-12 中旬，对照包以 1637 依赖刷新后为新基线）。
- 用户侧待办：① Fork 图形端推送（955-1637）② 真实 Tauri build 打包+安装包验证（按 docs/RELEASE_RUNBOOK.md，必须含 engine-rs release 重编 + core/dist 重烤——1637 升级面随打包进入壳）。
