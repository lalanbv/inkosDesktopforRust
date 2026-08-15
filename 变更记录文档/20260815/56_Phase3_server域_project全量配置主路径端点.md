# 56 号变更记录：Phase3 server 域——GET/PUT /project 全量配置主路径端点

- 日期：2026-08-15
- 范围：engine-rs（扩展 `server/project_config_routes.rs`、`server/mod.rs`）
- 契约源：`packages/studio/src/api/server.ts` L4181-L4207（GET）、L4324-L4346（PUT）；`packages/core/src/models/project.ts`（ProjectConfigSchema/LLMConfigSchema 默认值面）
- 验证：`cargo test --lib`（925）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（48）+ `cargo test --features export-bindings --lib`（1084）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

GET /project 在 TS 侧经 `loadCurrentProjectConfig` → core `loadProjectConfig` → `resolveEffectiveLLMConfig`（550 行：inkos.json + 环境层 + services 预设 + CLI 覆盖合并）。契约测试（server.test.ts L792/L816）的可观察面仅覆盖 raw 配置路径（PUT 后回读、损坏 JSON 结构化 500）——env 层合并场景无测试。据此 56 号按**主路径**移植：raw inkos.json 的 ProjectConfigSchema 语义提取 + schema 默认值；env 层/services 合并暂缓（偏差备案）。PUT 侧本身即是轻量合并器（三层浅合并，无 schema 校验）。

## 二、交付内容

### 1. `GET /api/v1/project`（server.ts L4181）

- 读 raw inkos.json：读失败/JSON 损坏 → ApiError 500 `{"error":{"code":"PROJECT_CONFIG_INVALID","message":"Failed to load inkos.json: ..."}}`（message 含 "inkos.json"，契约测试断言面）
- **schema 校验（响应所需字段面）**：name 非空、language ∈ zh|en（默认 zh）、llm 存在且 provider ∈ anthropic|openai|custom、baseUrl 合法 URL、model 非空——任一失败同样 500 `PROJECT_CONFIG_INVALID`
- 响应 8 字段：`{name, language, languageExplicit, model, provider, baseUrl, stream（默认 true）, temperature（默认 0.7）}`
- `languageExplicit = "language" in raw && raw.language !== ""`（显式设置且非空串——区别于 schema 默认 zh）

### 2. `PUT /api/v1/project`（server.ts L4324）

合并语义逐字：
- `temperature`/`stream` 给出（!== undefined）→ **直写 `existing.llm.x`（值透传不校验）**——`existing.llm` 缺失时 TS 解引用 TypeError → 端点 catch → **500 平铺** `{"error": ...}`（怪癖固化：inkos.json 无 llm 键且给出任一字段必 500；仅改 language 则不解引用、200）
- `language === "zh" | "en"` 精确命中才写入（"jp" 等静默忽略）
- 成功 `{ok: true}`；读写失败 500 平铺

### 3. E2E（config56_e2e，3 例）

- GET 默认值（zh/0.7/true/languageExplicit=false）→ PUT {language:en, temperature:0.2, stream:true} 回读（server.test.ts L792 同款场景）→ 非法 language 不写入
- 损坏 JSON / name 缺失 / model 空串 → 结构化 500 PROJECT_CONFIG_INVALID（code + message 含 inkos.json）
- 无 llm 键：temperature → 500 平铺；仅 language → 200

## 三、parity 要点

1. PUT 的 llm 解引用怪癖：`existing.llm.temperature = v` 在 llm 缺失时先于一切校验失败（TS 无 schema 校验，纯直写）
2. languageExplicit 与 language 的区别：schema 默认填充的 zh 不算显式（响应仍报 zh 但 explicit=false）
3. 500 形状双态：GET 结构化 ApiError（code/message 嵌套）vs PUT 平铺 error 串（端点自带 catch）

## 四、偏差备案

1. **env 层合并暂缓**：TS GET 返回 `resolveEffectiveLLMConfig` 的有效配置（INKOS_LLM_* 环境变量与 services 预设可覆盖 model/baseUrl/temperature 等）；Rust 返回 raw inkos.json 值 + schema 默认。桌面单仓场景（配置全在 inkos.json）行为一致；多环境变量部署场景有差异——随 LLM provider 配置域大件（40+ 预设表）移植后补齐
2. schema 校验错误文案：Rust 英文近似（"Invalid url" 等），code 与状态码一致
3. version 字面量（"0.1.0"）与 notify/foundation/writing/daemon 子 schema 未校验（响应不消费这些字段）

## 五、暂缓件

- `resolveEffectiveLLMConfig` 全链（env 层 + services 预设 + studio secrets + CLI 覆盖）——与 54 号 syncTopLevelLlmMirror 同属 LLM provider 配置域大件
- `GET /project/files|artifacts` 文件浏览面（L4270-L4322）
- skills / prompt-packs 域（L4209-L4268，独立轻域）

## 六、下一步（57 号候选）

1. architect 域大件（books/create + import/chapters + fanfic/init + spinoff/init 共同依赖：ArchitectAgent 基础设定生成——迁移主线剩余最大块）
2. skills / prompt-packs 轻域端点
3. Node sidecar 下线核对：累计 65+2=**67 端点**已可切换

## 七、影响面

- Rust 业务端点累计：65（55 号后）+ 2 = **67 个**
- 新增测试：e2e +3（config56_e2e）；lib 无新增
- 无破坏性变更；`/api/v1/project` 路由与子路径路由族（/project/detection 等）共存无冲突
