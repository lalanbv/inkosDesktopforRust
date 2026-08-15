# 74 号：生图链接通与影游执行器

**日期**：2026-08-15
**阶段**：Phase3 深度收尾（73 号"下一步"首选：生图链一次解锁两处 503 + 三确认意图执行器）
**契约源**：`packages/core/src/pipeline/short-fiction-runner.ts` L587-L760 + L810-L910（resolveCoverGenerationRequest / generateImageFromPrompt / 三 API 提取函数）、`llm/cover-providers.ts`（preset 表 + normalizeCoverBaseUrl）、`agent/film-authoring-tools.ts` L275-L370（STRUCT 提示词 + createDraftStructureTool / createConnectChoiceTool / createRemoveNodeTool）、`interactive-film/film-context.ts`（作者上下文摘要）、`authoring-generate.ts`（buildStructureDeltaFromLLMText）

---

## 一、背景

两处遗留 503 兜底与三个 Unsupported 确认意图：69 号 node-image 与 71 号 play generate-image 都因生图基础设施（cover-generation）未移植而返回 503/needsCoverConfig；draft_structure/connect_choice/remove_node 三个确认意图在 67 号起为"Unsupported confirmed action"。74 号接通 cover 基础设施一次解锁两处，并把三执行器接到 69 号已交付的 interactive-film 域本体上——确认意图累计接通 **6/11**（write_next/create_book/play_start/draft_structure/connect_choice/remove_node）。

## 二、交付

### 1. 生图基础设施 `engine-rs/src/llm/cover.rs`（新，~620 行 + 12 单测）

- **preset 表**（kkaiapi/openai/google——api 形态 images/images/gemini + 默认模型逐字）+ `normalize_cover_base_url`（http(s)/无 userinfo-query-hash/去尾斜杠）+ `cover_secret_key`。
- **`resolve_cover_generation_request`**：env 优先（INKOS_COVER_ENDPOINT/BASE_URL/MODEL/API_KEY——endpoint 判 `/responses` 定 api 形态，base 从 endpoint 反推剥后缀）→ `inkos.json` 的 `llm.cover`（service → preset）+ **secrets 三级 key 链**（`cover:{service}` → `{service}` → `{SERVICE}_API_KEY` env）→ 明确错误（"cover endpoint is required…" / "Cover API key is required…"——端点转 needsCoverConfig 提示）。
- **`generate_image_from_prompt` 三 API**：
  - **images**：POST `{base}/images/generations`（model/prompt/n/size）；b64_json 优先，url 形态下载（401/403 带 Bearer 重试；content-type 定扩展名）；
  - **responses**：POST `{base}/responses` + `tools:[{type:"image_generation",size}]`；`output[].image_generation_call.result` 或 content[] 的 result/image_base64；
  - **gemini**：POST `{base}/models/{model}:generateContent?key=…`（模型名 URL 编码）；`candidates[].content.parts[].inlineData`（camel/snake 双键 + mime 定 jpg/png）。
  - 错误消息逐字（HTTP 状态 + 500 字符预览）；300s 客户端预算。
- 提取函数（responses/gemini/images）pub 可测。

### 2. 两处 503 解锁

- **69 号 `POST /projects/:id/nodes/:nodeId/image`**：prompt 取 imageSlot.prompt/sceneDesc（双空 → TS generateNodeImage throw 消息逐字 500）→ resolve（未配置 → 400 `{error, needsCoverConfig:true}`）→ 生成 → `node_image_rel_path` 落盘 → **setImageRef delta 回写图谱**（imageSlot 注入 + applyGraphDelta rev 前进）→ `{assetRef, rev}`。
- **71 号 `POST /play/runs/:worldId/:runId/generate-image`**：scene/entity 目标（69 号已做的 prompt 构建保留）→ resolve → 生成成功：`play_image_file_name` 落盘 + **manifest ready 条目**（`set_play_image_entry` 新增——play-image.ts setPlayImageEntry 语义）+ `{key, ok:true, status:"ready", file, url}`；**生成失败不抛**——manifest failed 条目 + `{key, ok:false, status:"failed", error}`（UI 可重试）；未配置 → 400 needsCoverConfig。
- 69 号老 E2E 的 503 期望更新为真实行为（无 prompt 源 → TS 逐字 throw 消息）。

### 3. 三确认意图执行器（agent_production.rs）

- **装配分支**（payload 校验 + params 透传）：
  - connect_choice：payload.node 必填（缺 → 502 中文"确认连接选择缺少节点数据"）；projectId 兜底链 payload.projectId ?? bookId；
  - remove_node：nodeId 必填（缺 → 502"确认删除节点缺少 nodeId"）；
  - draft_structure：projectId 兜底链 + instruction（payload ?? 请求指令）。
  - tool 名分流：connect_choice/remove_node/draft_structure（各自独立工具名，无 stages）。
- **执行器**：
  - `execute_connect_choice`：StoryNode 严格反序列化（StoryNodeSchema.parse 等价——非法字段 Err）→ upsert delta → applyGraphDelta → `"Choices updated on node {id} (rev {rev})."` + `{kind:"graph_updated", rev}`；
  - `execute_remove_node`：remove delta → `"Node {id} removed (rev {rev})."`；
  - `execute_draft_structure`：图谱上下文（`summarize_story_graph_for_authoring`——film-context.ts 逐字：世界锚一行/变量列表/节点拓扑 + 角色档案带口吻）→ **编剧系统提示词逐字**（"恰好 1 个 start、至少 2 个 branch、至少 2 个差异化 ending"）→ LLM（film-authoring 端口 0.7）→ extractJson（fence + 首尾大括号）→ nodes 逐个 StoryNode 严格解析（空 → "draft_structure: LLM returned no nodes"）→ upsert delta → `"Structure drafted: {n} nodes (rev {rev})."`。

### 测试

- **lib 单测（12 个）**：preset/secret-key、normalize 七正反例、responses 双路径提取（直接 result + content 嵌套 image_base64）、gemini 双键 mime 判扩展、images b64 优先于 url、**resolve 三段**（无配置明确错/inkos.json 无 key 错/secrets cover:service 键完整请求）、**三 API 生成端到端 mock**（b64 下载/绝对 url 下载定 jpg/responses/gemini）、429 错误消息带 HTTP 状态。
- **E2E `image74_e2e`（3 个）**：**node-image 全链**（cover 配置经 inkos.json+secrets → 生成 → 图片落盘 → 图谱 imageSlot 注入回写 + rev → 未配置 needsCoverConfig）；**play generate-image 全链**（scene 投影兜底文本 → manifest ready + URL → entity 生图 → 71 号 GET run 实体 imageUrl 注入互证）；**三执行器确认流**（draft_structure 建骨架 rev 1 → connect_choice 改线 rev 2 → remove_node 删除 rev 3 + 节点消失 + 缺 nodeId 502 中文文案）。

## 三、parity 要点

1. **cover 请求解析优先级**：env → 项目配置 → 错误（两端点把"未配置"统一转 needsCoverConfig 400，与 TS catch 分支同形）。
2. **secrets 三级 key 链**：`cover:{service}` → `{service}` → env 大写替换——studio 配置面与 env 面双入口都命中。
3. **生成失败与未配置分离**（play 面）：失败记 manifest failed 可重试；未配置 400 引导配置——TS 语义。
4. **无 prompt 源的 node-image**：TS generateNodeImage 直接 throw（500），消息逐字——E2E 固化。
5. **draft_structure 的 LLM 产物防线**：节点逐个 StoryNodeSchema.parse（一条坏节点掀翻而非静默丢弃——TS map parse 同语义）。

## 四、偏差备案

1. **appendPromptPackGuidance 未接**：TS draft_structure/mutator 等提示词会拼项目 prompt-pack 指引；Rust 直连基础提示词（prompt-pack 覆盖面随后续号统一接）。
2. **draft_structure 的 phase 标记**：TS applyGraphDelta 传 `phase: "structure"`（authoring-state phase 前进）；Rust apply_graph_delta 保持现 phase（69 号实现的 phase 透传面未暴露）。
3. **play generate-image 的 size 默认 1024x1024**：TS 端 env INKOS_FILM_IMAGE_SIZE 默认 1024x1536（node-image.ts）；Rust 未读该 env（69 号 node-image 用 1024x1536 硬编码 ✓，play 面差异）。
4. **gemini URL 编码**：模型名/api key 经 encodeURIComponent 严格集（js_encode_uri_component 复用）。

## 五、暂缓件（沿 73 号清单滚动）

- 剩余 5 个确认意图执行器（short_run/generate_cover/script_create/storyboard_create/interactive_film_create/translation_create——各自依赖 short-fiction/script-storyboard/translation runner 域本体）
- sceneReconciler + regenerate 变体/checkpoint 面、play en 提示词、play_step 聊天工具
- 单章写作中途截断、actionPayload strict、/agent model 校验、resumeFrom、fetchWithProxy、attachments、模型四层解析

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **1036** 通过（+9 净） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **122** 通过（+3） |
| `cargo test --features export-bindings --lib` | 1195 通过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：互动影游节点配图与互动世界插图两处生成面从 503 兜底转全功能；影游创作三确认意图（骨架起草/连线/删点）接通——**确认意图 6/11**；生图基础设施为后续 generate_cover/short_run 等意图复用就绪。
- **下一步（75 号候选）**：translation_create 确认意图（70 号 translation 域本体全齐，纯接线 + POST /translations 端点已有 → 半天量级）；或 script_create/storyboard_create（script-storyboard-runner 588 行域本体移植 + 接线）；或散件收尾（单章截断/actionPayload strict/model 校验）。
