# 50 号变更记录：Phase3 server 域——交互运行时子集（export-save 落盘 + inspiration LLM 灵感卡）

- 日期：2026-08-15
- 范围：engine-rs（扩展 `server/books_state_routes.rs`、`server/mod.rs`）
- 契约源：`packages/studio/src/api/server.ts` L5667-L5698（export-save）、L3206-L3274（inspiration）；core 侧 `interaction/runtime.ts` export_book intent（可观察行为 = writeExportArtifact + details 映射）与 `interaction/project-tools.ts` exportBookToPath
- 验证：`cargo test --lib`（921）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（27）+ `cargo test --features export-bindings --lib`（1080）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

49 号完成编辑事务域后，books 域剩两个交互运行时触点：export-save（导出落盘到项目目录，走 TS `processProjectInteractionRequest` 的 export_book intent）与 workspace/inspiration（LLM 灵感卡）。经勘测，export_book intent 在 HTTP 面的可观察行为完全等价于 47 号已移植的 `build_export_artifact` + 落盘 + details 三字段映射，无需移植完整交互运行时会话机；inspiration 则是单次默认端点 chat（0.9/600）。本轮将两端点挂载到 Rust。

## 二、交付内容

### 1. `POST /api/v1/books/:id/export-save`（server.ts L5667）

- 正文解析：无效 JSON → 默认 `{format: "txt", approvedOnly: false}`（TS `.catch` 同款）；`format ?? "txt"`；`approvedOnly` 仅 bool true 生效
- 输出路径 `books/{id}.{fmt}`（fmt 原样拼名；内容格式经 `ExportFormat::parse` 未知回退 txt）
- 复用 47 号 `build_export_artifact`（内存负载）+ **本轮补落盘**（`fs::write(payload)`——47 号 GET /export 直接回流负载不落盘，落盘语义归本端点）
- 成功 `{ok, path, format, chapters}`；失败 500 `{"error": String(e)}`（如 `No chapters to export.`）

### 2. `POST /api/v1/books/:id/chapters/:num/workspace/inspiration`（server.ts L3206）

- 校验序：章节号非法 400 → brief 存在且非 string（含 null）400 → 章节文件缺 404 `"Chapter not found"`（文件定位与 read_chapter 同款：`NNNN` 前缀无下划线要求）
- 装配：章节正文 + 持久 brief（chapter_workspace）+ plan 文档 + book 配置（title/language）；`language === "en"` 判 en 其余 zh
- LLM：`AgentRouter::chat("inspiration", …, 0.9, Some(600))`——agent 键无 override → 默认端点（对齐 TS pipelineConfig.client/model）；中英系统提示与用户提示五行结构（书名/章节/当前用户提示/系统章节计划/当前章节）逐字对齐，空段过滤、`\n\n` 连接
- 空响应 → 500 `"The model returned an empty inspiration card"`；成功 `{chapterNumber, card}`（trim 后）
- 非变更性保证：不写任何文件（E2E 断言章节文件前后一致）

### 3. E2E（books50_e2e，3 例）

- export-save：md 全量（2 章落盘 b1.md）+ approvedOnly 过滤（1 章 b1.txt）+ 无效 JSON 默认回退
- export-save 空选集 → 500 `No chapters to export.`
- inspiration：mock LLM（spawn_mock_llm SSE）出卡 + 章节原样 + 400/404 分支

## 三、parity 要点

1. export-save 的 `format ?? "txt"`：缺失/null → txt；响应 `format` 字段回显**原始 fmt 串**（非解析后格式）
2. inspiration 的 brief null 也 400（TS `brief !== undefined && typeof !== "string"`，JS null ≠ undefined）
3. inspiration 章节定位前缀无下划线（`0002x.md` 也命中），与 chapter-replace 的 `0002_` 带下划线定位**不同源不同规**（server.ts 原文如此）

## 四、偏差备案

1. export-save format 非字符串垃圾输入（如数字 5）：TS `?? "txt"` 不兜底 → 文件名 `b1.5`、响应 format=5；Rust → txt（垃圾输入路径，不复刻）
2. inspiration 400/404/500 的错误体文案：TS 网络层错误为英文原生长文案；Rust 为简洁中文/英文短文案（状态码一致）

## 五、暂缓件

- 完整交互运行时会话机（session 状态机 + task.* 事件流 + 多 intent 路由）——桌面端当前无 HTTP 面直接消费，仅 export_book intent 经本端点落地
- `rewrite/:chapter`（rework 模式）与 `resync/:chapter` 端点（依赖 revise 链的 rework 变体与 resyncChapterArtifacts）
- acquireBookLock 跨进程文件锁（沿既有策略）

## 六、下一步（51 号候选）

1. `POST /books/:id/rewrite/:chapter`（reviseDraft rework 变体）+ `POST /books/:id/resync/:chapter`（章节工件重建）
2. books/create + create-status、detect/import/fanfic 域端点
3. sessions/state 配置域端点（Phase 2 清单）
4. Node sidecar 下线核对：books 域累计 32+2=34 端点已可切换

## 七、影响面

- Rust 业务端点累计：32（49 号后）+ 2 = **34 个**
- 新增测试：e2e +3（books50_e2e）；lib 无新增（纯端点装配，逻辑复用 47 号已测本体）
- 无破坏性变更
