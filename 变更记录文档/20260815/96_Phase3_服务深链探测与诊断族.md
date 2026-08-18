# 96 号变更记录：服务深链探测与诊断族（studio 模型配置域收尾·上）

## 一、背景

95 号变更记录"下一步"首选：studio 模型配置域（P1-1）拆分第一轮。**勘测修正了范围**——63 号 services 域轮已交付大量本体，指令所列四件特判中三件已有结构等价物：

- `load_secrets`/`save_secrets`/`resolve_service_api_key`（`llm/secrets.rs`，含 legacy id 迁移）✓
- `list_models_for_service`（live /models 探测 + bank 交叉 + **无 key 直通判定** `is_api_key_optional_for_endpoint`——Ollama/LM Studio 本地端点已覆盖）✓
- `resolve_service_model`（default/current 归属 → **bank check model 优先** → 首个启用模型 → fallback 链）✓
- 模型列表缓存：`service::baseUrl::key后8位` 键 + 10 分钟 + `refresh=1` 绕过 ✓（与 TS `modelListCache` 一致）

**真实缺口**：`/services/:service/test` 的 **chat 深链探测**与 **`formatServiceProbeError` 服务专属诊断族**——live /models 不可达且非 aggregator 时，TS 回退到"模型候选 × 协议计划"的最小 chat 探测，失败时按服务给出专属诊断（google 四步清单等）；Rust 此前止步于通用 400 文案（"chatCompletion 深链探测暂缓"备案）。本轮补齐。

## 二、交付

### 1. 深链探测（`service_routes.rs::test_service` 尾部）

- **候选**：preferred（body.model）+ discovered 前 2（`MAX_DISCOVERED_MODELS_TO_PING`；TS test 路径 configModel/generic fallback 均禁用）。空候选 → 通用文案（原行为保留）。
- **计划**：preferred 带流式 → 先流式后非流式；否则单非流式。**responses 协议传输未移植**——一律以 chat completions 探测（见偏差备案）。
- **最小 chat**（`minimal_chat_probe`）：`"Reply with OK only."`、max_tokens 16、temperature 0.7、无重试、**8s 超时**（`SERVICE_CHAT_PROBE_TIMEOUT_MS`；tokio timeout 包裹）。任一 (model, plan) 通过 → 200 `{ok, modelCount, models, selectedModel, detected{apiFormat, stream, baseUrl, modelsSource:"api"}, probe, chat:null}`。
- 全部失败 → 400 `error=format_service_probe_error(...)`。

### 2. `formatServiceProbeError` 诊断族（逐字双语）

- **google 分支**：`Google Gemini 测试连接失败。` + 上下文块 + 四步检查清单（AI Studio Gemini key 非 OAuth/Vertex、项目启用且未限制、地区/账号允许、泄露后重生成）。
- **moonshot/kimiCodingPlan/kimicode 分支**：kimi-k2.x 可能需要 temperature=1 的检查提示。
- **通用分支**：API Key/模型可用性/账号额度/协议匹配四项。
- **上下文块**：服务商（label ?? service）/测试模型/协议（Chat / Completions|Responses + 流式/非流式后缀）/Base URL。
- **上游详情**：剥 `(baseUrl:…)$` 行尾尾注（多行锚定）→ 含 `上游详情：` 时以 `\n上游返回：` 前缀整段保留。

### 3. 测试

- 单测 +2：google 分支（清单四步 + 上下文 + 尾注剥离 + 上游前缀）、moonshot/en 通用分支（协议后缀、label 回落、无上游详情省略）。
- E2E +2（`mod sub96_e2e`）：深链成功路（/models 404 → chat SSE OK → 200 selectedModel + modelsSource/api）；google 深链失败路（/models 404 + chat 400 → 400 四步清单诊断 + probe.ok=false）——mock 上游双路由（inline baseUrl 注入，不打真网）。

## 三、parity 要点

- 诊断文案、清单步骤、上下文块字段序、上游详情前缀逐字。
- 深链触发条件（live 不可达 + 非 aggregator 静态信任 + 候选非空）与候选构成对齐 TS test 路径。

## 四、偏差备案

1. **responses 协议计划**：TS `buildProbePlans` 含 `responses` 传输（preferred=responses 时仅试 responses）；Rust 流式层仅 chat completions，深链一律 chat 探测（对 preferred=responses 是行为超集——chat 通则判通）。Responses API 传输随协议层扩展轮评估。
2. **models 失败详情**：TS `fetchModelsFromServiceBaseUrl` 携带 `/models` 错误（`服务商返回 {status}` + 401/403 authFailed 短路）；Rust probe 主路径失败静默入深链（成功路径不依赖该详情，诊断以 chat 错误为准）。
3. **超时实现**：TS `withTimeout` + AbortSignal；Rust tokio timeout 包裹（错误文案 "service connection test timed out"）。

## 五、暂缓件（滚动）

- P1-1 剩余：/agent 模型**四层解析**（前端 service+model → defaultModel → secrets 首个有 key 服务探测 → legacy）——域本体（resolveServiceModel/listModels/secrets）经 63/96 号已全备，剩 agent_route 装配（97 号候选）。
- P2：doctor transport 回退细节、PDF 抽取、单章截断。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1136** 过（+2） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **160** 过（+2：sub96 两路） |
| `cargo test --features export-bindings --lib` | **1295** 过（+2） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（97 号候选）

1. **首选：/agent 模型四层解析**（P1-1 收官——agent_route 装配四层：前端 service+model 显式（无 key 400 双语）→ defaultModel → secrets 首个有 key 服务的首个文本模型 → 项目配置端点；依赖域本体已全备，纯接线轮）。
2. 其次：P2 清单逐件（doctor 回退 / PDF 选型 / 单章截断）。
3. 完成后 strangler 切换的 P1 清单全闭合，可做切换演练轮（端到端流量对跑 + 94 号基线复核）。
