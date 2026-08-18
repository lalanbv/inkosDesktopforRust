# 109 号变更记录：非 agent 面运行时配置装配（inkos.json 服务项 → runtime router 热解析，62 号备案升级闭合）

## 一、背景

108 号候选之二。勘测确认的架构缺口：TS studio **每次用配置时**经 `loadProjectConfig(consumer:"studio")` 热解析（服务项选择 + secrets key + transport 链）；Rust 非 agent 面（write REST / revise / import / daemon 写循环 / radar / style / fanfic / translation / forecast / play / film / short）全部使用**启动态 router**（bin env 装配）——inkos.json 服务项配置对这些面不生效（62 号部署备案的"重启生效"限制）。/agent 面已由 97 层四层解析覆盖。

## 二、交付

### 1. `BooksRuntime::effective_router()`（`server/books_routes.rs`）

- 解析链：`resolve_effective_llm_studio`（63 号已备的 studio 主路径——服务项选择/镜像/secrets key/transport 默认链）→ 可用性判定（baseUrl + model + apiKey（或免 key 本地端点））→ `AgentRouter`（**apiFormat + stream 透传**——106/108 号两维度在此汇合）；不可用回退启动 router。
- 缓存：进程内 per-root 表，键 = (inkos.json mtime, secrets.json mtime, 启动 router 指纹)——**配置写入即失效**（mtime 变化）、测试间不同 root/启动态天然隔离（指纹键防串）。

### 2. 消费面清扫（36+7 位点，10 文件）

- 全部非 agent 面 `runtime.router` → `runtime.effective_router().await`：books_routes（write/revise/plan/import 链装配）、ops_routes（daemon 写循环 + radar）、style/fanfic/translation/audit、agent_production（short/play/film/style 执行器）、book_create_routes（architect 地基链）、forecast_tools、sub_agent_tool（writer 委托）。
- 装配点异步化：`build_write_next_agents` 与 `forecast_agent` 改 async（调用点 5 处补 await）；`AuditRuntime` 同款 `effective_router`（state/router 共享 Arc 经 BooksRuntime 委托，缓存复用）。
- **agent_route.rs 有意保留启动/覆盖态 router**——其 `runtime` 已是 97 层 per-request 覆盖后的实例，语义正确。

### 3. E2E（`sub109_e2e`，1 测试）

- `non_agent_face_resolves_router_from_inkos_json_hot`：启动 router 指向**死端点** + inkos.json 服务项（custom:Cfg + secrets key + defaultModel）指向 mock → radar 扫描 200 且 marketSummary 来自 mock（热装配生效）；随后**改写 inkos.json 指回死端点** → 再扫描失败（mtime 失效 → 热回落）——配置写入即时生效的完整行为证明。

## 三、parity 要点

- 与 TS `loadProjectConfig` 每调用热解析语义对齐（studio 模式 env 忽略/服务项镜像/transport 链复用 63 号逐字实现）；62 号"重启生效"备案**闭合**。

## 四、偏差备案

1. **max_tokens/extra_headers/温度不参与热装配**：effective router 端点为 (baseUrl, apiKey, model) + apiFormat/stream（TS studio 客户端同核心四件；max_tokens per-call 由各 agent 自带）。
2. **缓存粒度为 mtime**：同秒内多次写入可能命中旧缓存（fs mtime 分辨率限制；桌面单写者场景无实害，备案）。

## 五、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1152 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **174** 过（+1：sub109） |
| `cargo test --features export-bindings --lib` | 1311 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 六、影响面与下一步（110 号候选）

配置面三件（97 层 per-request / 106-108 传输维度 / 109 热装配）收口后，Rust engine 在 strangler 部署下的配置行为与 TS sidecar 全面对齐。候选：

1. **首选：strangler 实切演练**（98 号 runbook 只读面起跑）——功能/协议/配置三面无已知缺口；需真实运行环境与流量条件（请求用户提供）。
2. 其次：迁移总结文档终版（30-109 号全链回顾 + 切换 runbook 修订版——比 103 号审计新增 106-109 四轮）。
3. 或：聊天卡 details 外露 + SSE tool:end 结构化 result（前端契约确认前提）。
