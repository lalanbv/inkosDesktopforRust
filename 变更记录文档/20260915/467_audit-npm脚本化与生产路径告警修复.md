# 467 号：audit:npm 脚本化——坐实 8 条生产路径告警 + uuid 等修复 + 白名单语义

- **日期**：2026-09-15
- **类型**：chore(security) + fix(deps) —— 454 备案脚本化落地并顺带坐实生产路径告警
- **关联**：454 号（audit 复核发现 --dev 过滤盲区）、429/434 号（override 先例与 pi-ai 钉）、451 号（jsdom/undici 镜像坑）
- **提交**：`scripts/audit-npm.mjs`（新）+ package.json + pnpm-workspace.yaml + pnpm-lock.yaml + 本记录

## 1. 脚本化（454 备案落地）

`scripts/audit-npm.mjs`：`pnpm audit --json --registry=https://registry.npmjs.org/`（npmmirror 无 audit 端点须显式指定）→ advisory 解析 → **白名单语义**：已定性接受风险（每条注理由+来源循环号）只汇总不拦截；白名单外任何告警非零退出——新漏洞不静默绿灯。registry 不可达退出码 3（对齐 audit-rust「审计缺失不静默」哲学）。package.json 注册 `audit:npm`。

## 2. 顺带坐实：454 的 `--dev` 过滤漏掉 8 条生产路径告警

`--dev` 只审 dev 依赖，本次全量审计暴露 8 条生产路径告警（uuid moderate / brace-expansion moderate+high×2 / fast-xml-parser moderate / postcss-selector-parser low / @ai-sdk/provider-utils low）。

**修复 2 条**（override 同 major 内）：
- `uuid: ^11.1.1`——mermaid 链（范围 `^11.1.0 || …` 允许），补丁 11.1.1 就位；
- `undici@8: 8.10.0`——451 引入的 jsdom 依赖 undici@8.10.2 npmmirror 缺版致 install 全挂（即 465 号发现的 install 失败根因），钉镜像可用版。

**接受 5 条（逐条理由入白名单）**：
- `brace-expansion`（moderate+high×2）：DoS 需攻击者可控 glob 模式；运行链（epub 导出 minimatch@5→2.1.4）模式为内部静态串无攻击面，补丁仅在 5.x（CJS 链跨 major 断裂风险大于收益）；dev 链（ts-morph→minimatch@10）已 update 到 5.0.9+；
- `fast-xml-parser`（moderate）：pi-ai 0.67.1 精确钉（434 号安全先例）→ 补丁需跨 major 5.x 且 Bedrock 默认未启用；
- `postcss-selector-parser`（low）：shadcn dev-only；
- `@ai-sdk/provider-utils`（low）：ai@6.0.159 精确钉 4.0.23，补丁 4.0.33+ 需 @ai-sdk/provider 3.0.16（镜像缺）且破钉。

**过程坐实两处环境坑（已解）**：①451 的 jsdom→undici@8.10.2 npmmirror 缺版致 pnpm install 自 451 起实际全挂（解释 451/452/454 首跑 vitest 抖动）——undici@8 钉 8.10.0 修复；②`--dev` 审计盲区——audit:npm 为全量语义。

## 3. 门禁

- `node scripts/audit-npm.mjs` exit 0（11 advisory：放行 11 拦截 0；uuid advisory 因修复消失）。
- 依赖变更回归：studio 109 文件 871 用例全绿 + core 245 文件 2112 用例全绿 + tsc 净。
- Rust 零改动。

## 4. 遗留

- **待推送：433–467 共 35 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 下一个编号自 **468** 起。
