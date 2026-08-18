# 114 号变更记录：导入结构态回写 + 聊天面 tool:end 结构化（113 号"穷尽"结论的复核勘误与闭合）

## 一、背景

对 113 号"会话内可开发项穷尽"结论复核，两项"维持备案"依据不成立：

1. **同步钩子族（47/51 号备案，103 审计 #10）**：勘测发现 TS `importChapters` 在 analyzer 输出无 runtime delta 时逐章调用 `syncLegacyStructuredStateFromMarkdown → rewriteStructuredStateFromMarkdown`——**markdown 真相面强制回写 `story/state/{manifest,current_state,hooks,chapter_summaries}.json` 四件套**。Rust 写作链经 settler 快照写四件套 ✓，但**导入链不写**——同书导入后双端 state/*.json 状态分歧，直接威胁 strangler 回滚仲裁（"双端同磁盘格式"前提）。真缺口。
2. **聊天卡 details / SSE tool:end 结构化（66/86 号备案，103 审计 #9）**：复核 TS 契约——聊天**响应卡已带 details**（loop 保留 + `tool_execution_cards` 序列化）；真缺口仅 **SSE `tool:end` 实时事件**：TS 载荷为 `result.content: [{type:"text",text}]` 数组 + **顶层 `details`**（studio 前端实时任务卡消费源），Rust 为纯文本 content 且无 details。真缺口（且无需"前端契约确认"——TS 即契约）。

## 二、交付

### 1. `state_bootstrap.rs`：`rewrite_structured_state_from_markdown`（TS 逐字）

- 既有模块（41-44 号移植的 load-or-bootstrap 变体 + 全部解析/归一助手）上补**强制回写变体**：markdown 三面（current_state/pending_hooks/chapter_summaries.md）→ 四件套整组重写；进度 = max(显式 fallback, 章节工件连续前缀)（index.json 条目 ∪ `NNNN_*.md` 文件名，缺 1 即止——不信 markdown 章号）；语言 = 既有 manifest 合法值 ?? book.json（仅 "zh" 认定，缺省 en）；projection_version/migration_warnings 保留。
- **import 链接入**：Step 2 每章 `save_new_truth_files` 后回写（TS syncLegacy 同位）。

### 2. 聊天面 `tool:end` 结构化（86 号备案闭合）

- `LoopEvents::on_tool_end` 增 `details: Option<&Value>`（成功时传 ToolResult.details，错误 None——TS exec.details 同语义）；SseBridge 载荷对齐 TS：`result.content` 为 `[{type:"text",text}]` 数组 + 顶层 `details`（None 时键缺省）。
- 聊天响应卡 details 已有（复核确认，无需改动）。

### 3. 测试

- **单测 1**：强制回写——既有 manifest（en/进度 9/projection 7）被 markdown 派生值覆盖（进度=工件前缀 2）而语言/projection 保留。
- **E2E 2（sub114）**：① 续放导入第 2 章 → 四件套落盘（manifest schemaVersion 2/language zh/lastAppliedChapter 2、current_state.chapter 2、chapter_summaries 含第 2 章、hooks 数组）；② 聊天 ingest_material → SSE `tool:end` 断言 `result.content[0].type=="text"` + 文本前缀 + `details.kind=="material_ingested"`。

## 三、parity 要点

- 四件套写入形态（pretty JSON）与 TS `JSON.stringify(x, null, 2)` 同构（尾换行为 Rust 侧既有惯例）；进度推导/语言判定/保留字段语义逐字；tool:end 载荷与 TS L5370 逐字段对齐。

## 四、偏差备案

1. **回写时机**：TS 在 `saveNewTruthFiles` 与索引更新之间；Rust 在 `save_new_truth_files` 后立即——同章内序等价。
2. **manifest 尾换行**：TS 无尾换行，Rust `write_string` 落盘带（state 文件族既有形态，双端 JSON.parse 无感）。

## 五、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1157** 过（+1：强制回写） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **180** 过（+2：sub114） |
| `cargo test --features export-bindings --lib` | **1316** 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 六、影响面与下一步（115 号候选）

103 审计 #9（SSE 结构化——聊天响应卡 details 复核确认本就齐备）与 #10（同步钩子族——markdown 回写面闭合；SQLite 记忆索引两钩子为 TS node:sqlite 索引重建、markdown 即权威源且 Node 无 sqlite 时同样跳过）闭合。P3 仅余：pi-ai 模型卡（无消费面）、authoring 边角（无端点消费）、revisionGate 热配置（bin env 维持）。候选：

1. **首选：strangler 实切演练**——待用户环境（Rust engine 起动 + 真实 LLM 端点配置 + 前端 API base 切换许可）。
2. 其次：迁移总结终版 v3（114 号勘误并入——磁盘格式面现已完全对齐）。
3. 或：按需轮。
