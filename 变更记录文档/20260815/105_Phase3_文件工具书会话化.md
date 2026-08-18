# 105 号变更记录：文件工具书会话化（read/ls/grep TS 逐字——books/ 作用域 + 注册矩阵对齐）

## 一、背景

104 号候选"schema 中文简述英文化"（103 审计 #8）。勘测发现差距比文案深：TS 的文件三件是**书会话专属 + books/ 作用域**（`createReadTool`/`createLsTool`/`createGrepTool`，agent-session bookTools），而 66 号 Rust 版是**全部会话注册 + 项目根作用域**——作用域、参数面、注册矩阵三层差异。本轮整体对齐（66 号备案 #6/#7 的实质收口）。

TS 矩阵勘测（agent-session.ts）：book/edit 会话有文件三件；**chat（无书）/play（有世界）/book-create（无书）均无**；`allowSystemFileRead` 缺省 false（studio 不传，`INKOS_AGENT_ALLOW_SYSTEM_READ` env 开启）。

## 二、交付

### 1. `interaction/project_tools.rs`：书域三件（TS 逐字）

- **read**：path 相对 books/（`safe_child_path` 逃逸拒绝）；系统读开启时绝对路径直读；失败为**非错误文本结果** `Failed to read "{path}": {message}`（TS textResult 形态）。
- **ls**：bookId + 可选 subdir；目录项 `/` 后缀或 `{name} ({N} bytes)`；空目录 `Directory is empty: {bookId}/{subdir}`；失败 `Failed to list "{bookId}/{subdir}": {message}`；readdir 自然序（TS 不排序）。
- **grep**：bookId + pattern（不区分大小写正则）；只搜 story/ 与 chapters/（md/txt/json）；输出 `{前缀}{文件}:{行号}: {原行}`（行不 trim）；>100 截断 `... [N more matches]`；零命中 `No matches for "{pattern}" in book "{bookId}".`；stat/read 失败整环失败 `Grep failed: {message}`（TS 同构）；迭代 worklist 复刻 readdir 深度优先。
- `env_flag_enabled`（agent-session.ts 逐字：未设默认/"1"/"true"/"0"/"false"）+ `INKOS_AGENT_ALLOW_SYSTEM_READ` → read 描述两分支逐字。
- `book_file_tool_schemas()`：三件 OpenAI function schema TS 逐字（参数描述全英文原文）。

### 2. `server/agent_route.rs`：注册矩阵 + 分发

- 工具装配：`book_edit_session`（book/edit）才注册文件三件——chat/play/book-create 不再注册（TS 矩阵）。
- 路由分发：read/ls/grep 且 book/edit 会话 → 书域执行器；未注册会话落未知工具文本（模型幻觉调用与 TS pi-agent 同后果）。
- 项目作用域旧三件保留为 loop 单测的通用执行器（agent 面不再注册）。

### 3. E2E 调整 + 新增

- **66 号 `tool_loop_roundtrip_with_execution_cards` 改造**：聊天会话读项目根 → 书会话读 `books/b1/note.md`（夹具 + mock 参数同步）。
- **79 号 play / 83 号 material 两处注册断言反转**：`contains(read)` → `!contains(read)`（play/chat 会话不注册——TS 矩阵）。
- **新 `sub105_e2e`**：书会话 grep 全语义（books/ 外不参与、story/chapters 前缀、大小写不敏感、行号原行）+ 注册矩阵双面断言（书会话批次含三件 + sub_agent；chat 批次含 propose_action/material、无 read/ls/grep/sub_agent）。

## 三、parity 要点

- 作用域（books/ vs 项目根）、参数面（bookId/pattern/subdir）、输出形态（bytes 后缀/前缀行号/截断文案/空命中文案）、注册矩阵（book/edit 独占）、系统读开关（env 语义逐字）、错误呈现（textResult 非错误）全部对齐 TS。

## 四、偏差备案

1. **错误 detail 文案近似**：`Failed to read/list/Grep failed` 后接 Rust io 错误 Display（TS 为 Node 原生 errno 文案）——48 号错误文案族。
2. **`Grep failed:` 含 "failed:" 触发 66 号 loop 错误启发式**（TS textResult 非错误）——66 号既有备案，本轮沿用；read/ls 失败文案不触发启发式（非错误保持）。
3. **grep 遍历序**：迭代 worklist 近似 readdir 深度优先（两侧 readdir 序均未定义；文件内行序精确一致）。
4. **schema 缺参防御文案自拟**（"read requires a path argument" 等）——TS zod 在参数层拦截不进执行器。

## 五、暂缓件（滚动）

103 审计 #8 闭合（含矩阵深化）。余：prompt-pack（#1）、env 请求级合并（#2）、responses 传输 + provider 特判族（#3/#5）、pi-ai 模型卡（#6 维持）、Scheduler 余量（#7）、聊天卡 details/SSE 结构化（#9）、同步钩子族（#10）、回放治理输入（#11）、评审轮数配置位（#12）、authoring 边角（#14）。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1145 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **170** 过（+1：sub105；66/79/83 三处随契约更新） |
| `cargo test --features export-bindings --lib` | 1304 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（106 号候选）

1. **首选：strangler 实切演练**——98 号 runbook 只读面起跑（103 号就绪度结论的实战验证；本轮后聊天面工具矩阵亦与 TS 全对齐）。
2. 其次：P3 余量（responses 传输 + provider 特判族同轮 / 聊天卡 details 外露）。
3. 或：环境变量面审计（INKOS_AGENT_ALLOW_SYSTEM_READ 本轮引入 Rust——全 env 族与 TS 对账一轮）。
