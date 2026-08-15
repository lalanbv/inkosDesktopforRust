# 63 号变更记录：services + cover 域 15 端点与 provider 配置大件（Phase3 · server/llm 域）

- 日期：2026-08-14
- 范围：engine-rs（`src/llm/providers_bank.rs` 新建 ~1490 行、`src/llm/cover_providers.rs` 新建、
  `src/llm/probe.rs` 新建、`src/llm/service_presets.rs` 扩展、`src/utils/llm_endpoint_auth.rs` 新建、
  `src/utils/llm_env.rs` 扩展、`src/server/service_routes.rs` 新建 ~1560 行、
  `src/server/project_config_routes.rs` 接线、`src/server/mod.rs` 15 路由挂载、E2E）
- 契约源：`server.ts` L3705-L4177（services 11 端点）+ L3854-L3953（cover 4 端点）+
  L1995-L2280（配置合并件）+ `core/src/llm/`（providers bank / service-presets /
  cover-providers / probe / secrets）+ `core/src/utils/effective-llm-config.ts`（studio 主路径）

## 背景

strangler 迁移 63 号（62 号核对清单的最高优先级缺口域）。services/cover 是 Studio 的
LLM 服务配置面：连接状态、密钥管理、模型发现、连通性测试、封面生成配置。依赖此前
未移植的 provider bank（39 endpoint / 737 模型卡）与 effective 配置解析。同时补齐
54 号（syncTopLevelLlmMirror）与 56 号（GET /project env/services 合并层）两项偏差备案。

## 交付

### provider bank（`src/llm/providers_bank.rs`，数据层）

- 39 endpoint / 737 模型卡逐字移植——TS 44 个 endpoint 文件经 **vitest 导出 +
  Python 代码生成器**转换（零手抄；`/tmp/gen63/endpoints.json` 为中间产物）
- `get_all_endpoints`（OnceLock 惰性一次构造）/ `get_endpoint`（39 项线性查找，
  无 unsafe）
- 单测：数量断言（39/737）、id 唯一性、spot checks（deepseek 4 模型 + compat 标记、
  ollama 本地、custom 无分组）

### 核心域小件

- `llm/cover_providers.rs`：3 封面预设 + `normalize_cover_base_url`（http(s)、
  无凭据/查询/锚点、去尾斜杠）+ `cover_secret_key`（`cover:{service}`）
- `llm/probe.rs`：`probe_models_from_upstream`（reqwest GET /models，任何失败空数组）+
  `list_models_for_service`（live probe → bank 交叉（lookup_model 补卡）→
  legacy knownModels 三层）
- `llm/service_presets.rs` 扩展：`resolve_service_preset`（bank + legacy 合并语义逐字：
  provider 优先、api/family/baseUrl/label/modelsBaseUrl 链式回退）+
  `resolve_service_provider_family`
- `utils/llm_endpoint_auth.rs`：`is_api_key_optional_for_endpoint`（anthropic 必带 key；
  本地/回环/.local/私网 IPv4 免 key）
- `utils/llm_env.rs` 扩展：`global_env_path`（~/.inkos/.env）+ `read_env_config_values`
  （server.ts 手写 .env 解析：`^([A-Za-z_][A-Za-z0-9_]*)=(.*)$` + `#` 注释 + 引号剥离，
  失败全空态）
- 新依赖：`percent-encoding = "2"`（custom:%XX 名解码，61 号已引）

### 15 端点（`src/server/service_routes.rs`）

| 端点 | 契约要点 |
|---|---|
| GET /services | bank 38 项（custom 排除）+ connected（secrets key / 免 key+已配置）+ custom 追加 + 优先级稳定排序（kkaiapi/openrouter/newapi/siliconcloud 置首） |
| GET /services/config | services + service/defaultModel + configSource:"studio" + storedConfigSource + envConfig 双层摘要（project/global/effectiveSource/runtimeUsesEnv:false） |
| POST /services/config/import-env | .env 检测（project 优先 global）→ 无 key 400 平铺；entry 构造（custom 名 "Env LLM"）→ merge → secrets 写 → 镜像 → `{ok,source,service,defaultModel}` |
| PUT /services/config | services merge（custom:{name} 键）/ defaultModel / configSource=env 400 平铺不落盘 / service → 镜像 |
| DELETE /services/:service | 条目过滤 + 选中服务联动清（service+defaultModel）+ secrets 清 + 缓存清 |
| POST /services/:service/test | 未知服务 400 / 空 key 400（公网）→ probe 主路径：live /models（非空即成功，selectedModel 取首个文本模型，B12 形状 `{probe, chat:null}`）→ aggregator 组静态表 fallback（modelsSource:"fallback"） |
| GET/PUT /services/:service/secret | `{apiKey}` 回读 / trim + header-safe 校验（`^[\x21-\x7E]+$`）400 `{ok:false,error}` / 空 key 删除 |
| GET /services/models | bank 按已存 key 过滤 → 文本模型 + maxOutput/contextWindow 字段 |
| GET /services/models/custom | config custom 条目 + key → live probe → 文本过滤 |
| GET /services/:service/models | 无 key（非本地）`{models:[]}`；cache 10min（key= service::baseUrl::key尾8位）；listModelsForService + 文本过滤 |
| GET/PUT /cover/config | `{service,model,baseUrl,configured,providers[3]}` / preset 校验 400 + model 白名单回 default + baseUrl 校验 400 |
| GET/PUT /cover/secret/:service | preset 校验 + cover:{service} 键 roundtrip + header-safe 400 |

### 两项功能性缺口补齐

1. **syncTopLevelLlmMirror**（54 号备案）：`llm.provider/baseUrl/model/temperature/
   apiFormat/stream` 镜像随选中服务 + defaultModel——接入 PUT /project/default-model、
   POST import-env、PUT /services/config 三处
2. **GET /project 有效配置合并**（56 号备案）：`resolve_effective_llm_studio`（
   resolveEffectiveLLMConfig 的 studio-project 主路径）——selectServiceEntry（configured →
   find/synthesize；无 → services[0]）→ applyServiceEntry（family/baseUrl/传输默认）→
   resolveServiceModel（defaultModel/currentModel 属服务优先 → checkModel → bank 首可用；
   custom → 透传）→ secrets apiKey → fillNoopLLMDefaults（provider→openai、
   baseUrl→example.invalid、model→noop-model）→ 动态模型服务（ollama/openrouter 等 7 个）
   白名单外 id 直接透传

## parity 要点

- 错误形态：本域 400 多为**平铺** `{error}` 或 `{ok:false,error}`（非 ApiError 结构），
  逐端点对齐；`/test` 为 B12 两步形状（`chat:null` 固定——chat hello 是 Studio 后续调用）
- `custom:{name}` 键语义贯穿（serviceConfigKey / normalizeServiceEntry 的 percent-decode）
- GET /services 的 group 为 serde 序列化（overseas/china/aggregator/local/codingPlan）
- `/services/:service/models` 缓存指纹取 key 尾 8 位（`apiKey.slice(-8)`）

## 偏差备案

- **/test 的 chatCompletion 深链暂缓**：live /models 不可达且非 aggregator 组时的
  逐模型 chat hello 探测环（依赖完整 openai-responses/anthropic-messages 多协议客户端栈
  与 formatServiceProbeError 服务特定长文案链）——本论实现 probe 主路径 + 静态表
  fallback，探测失败统一 400"无法自动确定模型"消息；E2E 以 mock /models 覆盖主路径
- **proxyUrl 探针代理暂缓**：TS probe 走 fetchWithProxy（llm.proxyUrl）；Rust 直连
  （reqwest）——桌面场景代理配置罕见
- **56 号行为修正**：GET /project 对 `model:""` 的 config 由旧实现的 500 改为 TS 真实
  语义（fillNoopLLMDefaults → 200 noop-model）——56 号 e2e 断言随之更新（属修复非回归）
- **provider bank 生成链**：TS 源 → vitest 导出 JSON → Python 生成器 → Rust；再生成时
  需重跑导出脚本（工具非持久化，bank 数据以 TS 为唯一真源）

## 暂缓件

- `resolveEffectiveLLMConfig` 的 cli/daemon 消费者分支（applyCliProjectConfig /
  applyLegacyEnvConfig + warnings 面板）——Studio 消费面恒为 studio-project，CLI 层
  随 CLI 域需求移植
- `/test` chat hello 后续调用（Studio 前端在 probe 成功后单独触发）
- fetchWithProxy 代理层

## 下一步（64 号候选）

- sessions + agent 域（9 条，交互运行时会话机大件——62 号清单次优先级）
- foundation/revise + resumeFrom + 同步钩子（books 域补全）
- interactive-films/projects 域（10 条）

## 影响面

- Rust 业务端点 82 → 97 个；lib 测试 953 → 970；e2e 65 → 77；
  export-bindings 1112 → 1129；TS 基线不变
- 62 号核对清单缺口 52 → 37 条（services 11 + cover 4 全清）；共同端点 80 → 95 条
- 全量基线：`cargo test --lib`（970）/ `golden_leaf`（76）/
  `e2e_write_next_contract`（77）/ `--features export-bindings --lib`（1129）/
  `clippy --lib --tests --bins`（零警告）/ TS vitest（185 文件 1798 测试）
