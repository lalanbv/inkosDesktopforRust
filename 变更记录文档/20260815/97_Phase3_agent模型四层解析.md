# 97 号变更记录：/agent 模型四层解析（P1-1 收官）

## 一、背景

96 号变更记录"下一步"首选：/agent 模型四层解析装配——94 号差分扫描 P1 清单的最后一件（P1-2 attachments 已于 95 号闭合）。域本体（resolveServiceModel 语义/listModelsForService/loadSecrets）经 63/96 号已全备，本轮纯接线。

## 二、交付

### 1. `agent_production.rs`：`resolve_agent_model_override`（四层逐层）

- **层 1（前端显式 service+model）**：`resolve_configured_service_base_url`（inline → preset → inkos.json services custom 条目）+ secrets/env key 解析。无 baseUrl → 500（TS 逐字错误）；**无 key 且非本地端点 → 400 双语逐字**（`请先为 {service} 配置 API Key` + `请先在模型配置中为 {service} 填写 API Key，然后再试。` / en 对应）；本地端点（localhost/127.0.0.1/::1）无 key 放行（Ollama 语义）。
- **层 2（新配置 defaultModel）**：inkos.json `llm.defaultModel`（文本模型校验）+ `services[0]` 条目 → baseUrl + key 全备则命中；任一缺失静默下落。
- **层 3（secrets 首个有 key 服务）**：遍历 secrets（按服务名排序——HashMap 无插入序，见偏差备案）→ `list_models_for_service`（live probe + bank 交叉）→ 首个文本模型 → baseUrl 可解析则命中；静默下落。
- **层 4**：None → 项目配置端点（现状兜底）。

### 2. `agent_route.rs`：per-request router 覆盖

- 解析点在 model 校验之后、attachments 之前（对齐 TS 顺序：config/client 之后、session 装配之前——**确认面与聊天回环双路径生效**）。
- 命中 → 构造覆盖 `AgentRouter`（base_url/api_key/model 来自解析产物）并重绑 `runtime`（hub/state/builtin_genres_dir/revision_gate 同引用共享）——本次请求的**全部代理**（studio-agent 聊天回环、sub_agent 域链、write_next、确认面生产）走前端选定模型，对齐 TS `pipelineClient` 注释语义（"so sub_agent tools use the frontend-selected model"）。
- 兼容修正：`root` 由 `&Path` 借用改 owned `PathBuf`（消除 runtime 重绑的借用冲突）。

### 3. 提权与测试

- `service_routes`：`resolve_configured_service_base_url`/`service_config_key` 提 `pub(crate)`（`load_raw_config` 本已 pub(crate)）。
- 单测 +2：层 1 无 key 400 双语（zh/en 两态 + `Cannot resolve model` 500 面）、层 1 命中（custom 条目 + secrets）与层 4 下落（层 3 探测不可达静默）。
- E2E +2（`mod sub97_e2e`）：**层 1 命中全链**——inkos.json custom 服务指向捕获 mock + secrets 有 key，`/agent {service, model}` 请求经覆盖端点发出且**代理请求 model 恒为前端模型**（项目端点指向不可达地址以反证）；层 1 无 key 400 双语（deepseek 非本地 preset，解析阶段即返回不打网）。

## 三、parity 要点

- 四层级序、层 1 无 key 双语文案与 `{error, response}` 双键 400 形态、本地端点无 key 放行、层 2/3 静默下落语义逐字/逐层对齐。
- 覆盖范围与 TS 一致：前端选型驱动本次请求全代理面（聊天 + 确认面 + 子代理）。

## 四、偏差备案

1. **层 3 服务迭代序**：TS `Object.entries(secrets.services)` 为 JSON 插入序；Rust HashMap 无序，按服务名排序迭代（确定性优先；多服务都有 key 时选择可能与 TS 不同）。
2. **覆盖 router 的 max_tokens/extra_headers**：固定 8192/空（项目配置端点的 per-agent 覆盖在覆盖态下不参与——TS pipelineClient 同样不带旧 service 的配置）。
3. **pi-ai 模型卡**：TS ResolvedModel 携带 contextWindow/compat 等 pi-ai 模型卡字段；Rust 覆盖端点仅 (base_url, key, model)——上下文窗等元数据不参与 Rust 流式层（无该消费面）。

## 五、暂缓件（滚动）

- **strangler 切换 P1 清单全闭合**（94 号：P1-1 本轮 + P1-2 95 号）。剩余 P2：doctor transport 回退细节、PDF 抽取、单章截断。
- P3：层 3 迭代序（secrets 保序需改存储形态）、provider 特判族残余。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1138** 过（+2） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **162** 过（+2：sub97 两路） |
| `cargo test --features export-bindings --lib` | **1297** 过（+2） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（98 号候选）

**strangler 切换的 P1 阻断项全部闭合**——前端模型直选、带图聊天、全部创作链与管理面端点在 Rust 端可用且有 E2E。下一轮候选：

1. **首选：strangler 切换演练轮**（端到端流量对跑清单 + 94 号基线复核更新——P1 闭合后的切换前演练：以既有 48 个 E2E 模块为流量脚本，输出切换 runbook 与回滚路径；纯勘测+文档轮）。
2. 其次：P2 清单逐件（doctor 回退 / PDF 选型 / 单章截断）。
3. 或：层 3 secrets 保序存储改造（P3 精修）。
