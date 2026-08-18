# 93 号变更记录：散件收尾批（/agent model 校验 + attachments 归一化；三件勘测备案）

## 一、背景

92 号变更记录"下一步"首选散件收尾批五件（resumeFrom REST 面 / /agent model 校验 / attachments 归一化 / 模型四层解析 / fetchWithProxy）。逐件勘测 TS 契约后，两件为可移植缺口（本轮交付），三件为勘测结论备案（无缺口或不适合本轮）：

- **resumeFrom REST 面——备案关闭（表述修正）**：TS `/api/v1/books/:id/import/chapters` 端点只收 `text` + `splitRegex`，调用 `importChapters({bookId, chapters})` **不带** resumeFrom/importMode（该参数面仅存在于聊天工具）。Rust REST 端点已与 TS 一致，无需改动。
- **模型四层解析——继续备案**：TS /agent 的模型解析四层（前端 service+model 显式 → 新配置 defaultModel+services[0] → secrets 首个有 key 服务探测 models → legacy client）依赖 studio 配置域（`resolveServiceModel` / `listModelsForService` / `loadSecrets`），Rust 未移植该域；当前 Rust 模型面由 inkos.json 端点配置承担（≈ 层 2/4 的项目配置语义）。属 studio 模型配置域的移植范围，非散件。
- **fetchWithProxy——备案：天然等价**：TS 为 undici ProxyAgent 包装（`INKOS_LLM_PROXY_URL → HTTPS_PROXY → https_proxy → HTTP_PROXY → http_proxy`），用于 studio /models 探测。Rust reqwest **默认**读取系统代理环境变量（HTTP_PROXY/HTTPS_PROXY/ALL_PROXY），网络面已天然等价；材料/研究面的 `.no_proxy()` 对应 TS 裸 fetch（材料获取不走代理）语义，此前已对齐。唯一差集是 `INKOS_LLM_PROXY_URL` 自定义变量（studio 探测专用），随模型配置域轮次评估。

## 二、交付

### 1. `/agent` model 校验（TS `isTextChatModelId` 逐字）

- `agent_production.rs`：`NON_TEXT_MODEL_ID_PARTS` 八片段表（image/embedding/embed/rerank/tts/speech/audio/moderation，lower+trim 子串匹配）；`is_text_chat_model_id`（非空且不含任何片段）；`non_text_model_message` 双语逐字。
- `agent_route.rs`：`payload.model` 非空且非文本模型 → 400 `{error: message, response: message}`（TS 形态与双语按项目语言）；空串跳过（TS falsy 语义）。

### 2. `/agent` attachments 归一化（TS `normalizeAgentAttachments` 逐字语义）

- `agent_route.rs` 新增归一化链：
  - 校验面（ApiError 形态 `{error:{code,message}}` 逐字）：非数组 → 400 `INVALID_ATTACHMENTS`；>8 件 → 413 `TOO_MANY_ATTACHMENTS`；非对象 → 400 `INVALID_ATTACHMENT`；缺 dataUrl → 400（`{filename} is missing dataUrl`）；非 base64 dataUrl → 400 `INVALID_ATTACHMENT_DATA_URL`；>4MB → 413 `ATTACHMENT_TOO_LARGE`；文本附件 >120k UTF-16 码元 → 413 `ATTACHMENT_TEXT_TOO_LARGE`。
  - 落盘：`.inkos/uploads/{safeSessionId}/{ts}-{i}-{safeFilename}`（时间戳防重名）；相对路径 posix 化。
  - 三类分支：image（mime `image/*`）/ 文本（`text/*` 或 9 个文本扩展名）/ 其余仅存。
  - 助手逐字：`safe_upload_file_name`（路径字符折叠 `_` + 非 `\p{L}\p{N}._ -` 连续折叠单 `_` + 120 截断 + 空 → "upload"）、`is_text_attachment`、`parse_data_url`（`data:{mime}?;{params}?;base64,{payload}`，mime 缺省 octet-stream）。
  - broadcast `agent:start` 的 `attachments` 计数从硬编码 0 改为实际件数。
- **注入面备案**：TS 侧 attachments 最终经 buildAgentSession 进入多模态消息（image base64 内联 / text 注入）；Rust `LLMMessage` 无 image 内容形态——归一化产物本轮用于校验/落盘/计数，多模态注入随 LLM 层扩展轮次接入。

### 3. 测试

- 单测 +5：`attachment_tests`（safe_upload_file_name 折叠/截断/回退、is_text_attachment、parse_data_url 四变体含非法 base64）与 `model_guard_tests`（八片段检测 + 双语文案）。
- E2E +1：`sub93_e2e::agent_model_guard_and_attachment_normalization`——非文本模型 400 双语（error=response 同文案）+ 文本模型放行；attachments text+image 两件落盘（文件名含序号与安全化中文名）+ 非数组/缺 dataUrl 错误面（code 与 message 逐字）。

## 三、parity 要点

- model 校验的八片段、双语文案、`{error, response}` 双键 400 形态逐字。
- attachments 的常量（8 件 / 4MB / 120k 码元）、错误 code/message、上传目录结构与文件名格式（`{ts}-{i}-{filename}`）、三类分支判定逐字。

## 四、偏差备案

1. **base64 严格性**：TS `Buffer.from(…, "base64")` 宽松（忽略非法字符）；Rust 标准 engine 解码失败 → 400 `INVALID_ATTACHMENT_DATA_URL`。dataUrl 来自前端（标准 base64），取严格侧。
2. **attachments 注入面**：见交付 2——多模态消息形态未支持，归一化已为注入做好数据准备。
3. **120 截断/文本长度以 chars/UTF-16 计数的 BMP 近似**：safe filename 截断用 chars（此前各轮同款近似）。

## 五、暂缓件（滚动）

- 模型四层解析（studio 模型配置域：resolveServiceModel/listModelsForService/secrets + INKOS_LLM_PROXY_URL）。
- attachments 多模态注入（LLMMessage 扩展 image 内容形态）。
- PDF 文本抽取（83 号）——独立依赖选型轮。
- 单章写作中途截断。
- sidecar 契约差分扫描（迁移收尾的系统对齐基线）。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1131** 过（+5） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **157** 过（+1：sub93） |
| `cargo test --features export-bindings --lib` | **1290** 过（+5） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（94 号候选）

/agent 参数面（actionSource/requestedIntent/actionPayload/skills/model/attachments）至此全部归一化对齐。下一轮候选：

1. **首选：sidecar 契约差分扫描**（以 TS 集成测试清单为基线，对已移植端点做系统对齐盘点——输出缺口清单并按风险排序，作为 strangler 切换前的收尾基线；纯勘测+文档轮，为后续实施轮定向）。
2. 其次：studio 模型配置域（模型四层解析 + /models 探测 + INKOS_LLM_PROXY_URL——闭合 93 号两件备案）。
3. PDF 文本抽取选型轮（pdf-extract vs lopdf 文本层自实现）。
