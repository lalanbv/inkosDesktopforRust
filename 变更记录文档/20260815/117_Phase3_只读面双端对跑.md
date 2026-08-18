# 117 号变更记录：strangler 只读面双端对跑（runbook 第一步自动化实跑——LLM 指本地 mock 即不依赖用户资源）

## 一、背景

verifier 确认实切阻塞待用户，按需轮方向可选。重新审视 runbook 五步后判定：**第一步（只读面对跑）的全部先决条件均可自备**——Rust engine 进程内全量路由、TS sidecar 真实进程（tsx 直跑 `server.ts`，依赖已装）、LLM 指向进程内 mock（对跑不消费真实端点）。94 号盘点是"文档对账 + E2E 单侧"，**双端真实进程同 fixture 对跑 diff 从未实跑**——本轮补上。

## 二、交付

### `tests/strangler_duel.rs`（新测试目标，`INKOS_DUEL=1` 门控——不进基线六项）

- **双端起跑**：同 fixture 根（书 b1 全真相面 + inkos.json 服务项 custom:Duel → mock + secrets + `version:"0.1.0"` 字面量——sidecar `ProjectConfigSchema` 严格校验，踩坑记录）下：
  - Rust：进程内 `router_books` 全量路由（bin 同款装配；write-next runner 为桩——只读面不触写链）绑定临时端口；
  - TS：`packages/studio/node_modules/.bin/tsx src/api/index.ts <root>` 真实进程（预置 `dist/index.html` 跳过前端自动构建；stdout/stderr 落 `/tmp/duel-tsx-*.log` 便于排障；线程 kill+wait 收割）。
- **只读桶对跑**（98 号 runbook 第一步端点族）：`/books`、`/books/b1`、`/books/b1/chapters/1`、`/books/b1/truth`、`/books/b1/analytics`、`/skills`、`/project`、`/prompt-packs`、`/sessions`、`/logs` 共 10 端点——**状态码 + 结构化 JSON 等价**（serde_json 对象等价天然键序无关；递归剥除波动键 startedAt/completedAt/timestamp/createdAt/updatedAt/version）。
- 断言差异清零（守门形态：后续回归任何一侧契约改动即红）。

### 实跑结果

**10/10 端点全一致**（`INKOS_DUEL=1 cargo test --test strangler_duel`，0.87s）。排障过程另收获两条 runbook 勘误：① sidecar 启动对 inkos.json 做严格 schema 校验（`version` 字面量必填）；② 前端 dist 缺失会触发 vite 自动构建（对跑场景需预置跳过）。

## 三、意义与边界

- **意义**：strangler 切换第一步从"文档清单 + 单侧 E2E"升级为**可重复执行的双进程契约对跑**——CI/本地一键复验，且不依赖任何用户资源；98 号 runbook 第一步自此有了自动化验收门。
- **边界**：LLM 面走 mock（对跑校验的是 HTTP 契约与磁盘读面，不是模型行为）；第二至五步（创作链/聊天面/模型配置/全量）仍需真实端点与前端切换（用户条件）。

## 四、验证基线（117 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1157 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | 181 过 |
| `cargo test --features export-bindings --lib` | 1316 过 |
| `cargo clippy --lib --tests --bins` | 零警告（含新测试目标） |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **10/10 只读端点双端一致** |

## 五、下一步（118 号候选）

1. **首选：runbook 第二步预演（创作链对跑扩面）**——在对跑框架上加写路径场景（同一 fixture 双端先后执行同操作 + 只读面复跑 diff 磁盘投影）——写面双端并发写同根有互斥风险，需串行+快照仲裁形态设计，可先做单侧写后另侧读的一致性验证（双端同磁盘格式已由 114 号保证）。
2. 或：实切条件到位后直接按 runbook 全量五步执行（用户三项条件）。
3. 或：按需轮（用户指定缺口/缺陷）。
