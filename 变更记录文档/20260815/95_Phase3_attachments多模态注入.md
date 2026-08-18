# 95 号变更记录：attachments 多模态注入（P1-2 切换阻断项闭合）

## 一、背景

94 号差分扫描将 attachments 多模态注入列为 P1 切换阻断项（93/94 两轮备案：归一化/落盘/校验已就绪，image base64 内联与 text 注入未进 LLM 消息）。本轮（95 号）闭合该缺口——对齐 TS `agent-session.ts` 的双通道消费链（`buildAttachmentUserBlock` 文本块追加用户消息 + `attachmentImages` 传 pi-agent `prompt(message, images)`）。

## 二、交付

### 1. `llm/streaming_client.rs`：多模态序列化层

- `ChatImage { data /*base64*/, mime_type }`（对应 TS `ImageContent`）。
- `ChatCompletionParams` 增 `images: Option<&[ChatImage]>`：注入**最后一条 user 消息**为 OpenAI vision content 数组——`[{type:"text", text}, {type:"image_url", image_url:{url:"data:{mime};base64,{data}"}}…]`；其余消息（含早前 user 轮）保持纯字符串。空列表 = 不注入（全量兼容：既有 4 处构造点传 None，行为零变化）。
- 语义对齐：TS `agent.prompt(message, images)` 的图随当轮 user 消息进入历史并在后续轮次保留——Rust 由 agent_loop 每轮重发历史 + "最后一条 user"（即 instruction 消息）恒带图实现同等形态。

### 2. `server/agent_route.rs`：注入装配

- `AgentAttachment` 恢复数据面：`image: Option<(base64, mime)>`（归一化时编码保留）+ `text: Option<String>`（120k 码元校验通过后保留）。
- `build_attachment_user_block`（TS 逐字双语）：`## 用户上传文件（宿主已接收，用户授权本轮使用）` 清单块（`### {filename}` + id/mime/size/stored_path 行）+ 三分支（text → `内容：` + 代码块全文；image → `- 图片：已作为多模态输入附加`；其余 → `- 内容：已保存；当前 MIME 类型暂未配置文本抽取器`）。
- `attachment_images`：image 附件 → `ChatImage` 列表。
- 装配：instruction 拼接附件块（无附件零变化）→ `RouterLoopChat` 持图 → 每轮请求经 params 注入。

### 3. 测试

- 单测 +3：vision 序列化（最后一条 user → 数组、早前 user/其它角色纯字符串、空列表不注入、data URL 形态）；附件块双语三分支（清单行/内容代码块/图片标记/仅存标记/空清单）；`attachment_images` 提取。
- E2E +1：`sub95_e2e::agent_attachments_inject_text_block_and_vision_images`——mock 捕获 studio-agent 请求侧的 user 消息，断言**请求真实形态**：content 为 vision 数组、text 段 = 指令 + 附件清单块（文本附件全文代码块 + 图片标记行）、image_url 段 data URL、上传落盘两件（93 号行为保留）。

## 三、parity 要点

- 附件块文案、分支语义、代码块包裹逐字；vision 数组形态为 OpenAI 标准协议（TS pi-agent 底层同为 OpenAI 兼容形态）。
- 图片仅在"当轮 instruction 消息"带（历史保留）——与 TS prompt(message, images) 一致。

## 四、偏差备案

1. **provider 特判**：TS pi-agent 按模型家族适配多模态（如 Anthropic 原生 image 块）；Rust 统一走 OpenAI vision 形态（Rust LLM 层本就统一 OpenAI 兼容协议，与既有单形态决策一致）。
2. **图片大小上限**：沿用 4MB 单件（93 号归一化层），未加 base64 膨胀后的请求体二次校验（TS 同）。

## 五、暂缓件（滚动）

- studio 模型配置域（P1-1：四层解析 + 探测特判族 + INKOS_LLM_PROXY_URL——94 号清单唯一剩余 P1）。
- P2：doctor transport 回退细节、PDF 抽取、单章截断。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1134** 过（+3） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **158** 过（+1：sub95） |
| `cargo test --features export-bindings --lib` | **1293** 过（+3） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（96 号候选）

带图/带文聊天在 Rust 端完整可用，94 号 P1 清单仅剩模型配置域。下一轮候选：

1. **首选：studio 模型配置域之 secrets/models 轮**（P1-1 拆分第一轮：`loadSecrets`/`listModelsForService`/`resolveServiceModel` 域本体 + /models 探测特判族——Ollama/LM Studio 无 key、Google 400 诊断、按 baseUrl 缓存键、bank check model 优先级）。
2. 其次：/agent 四层解析收尾轮（依赖第一轮的域本体）。
3. P2 清单逐件。
