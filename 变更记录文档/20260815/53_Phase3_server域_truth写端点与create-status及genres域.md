# 53 号变更记录：Phase3 server 域——truth 写端点 + create-status + genres CRUD 域（七端点）

- 日期：2026-08-15
- 范围：engine-rs（新增 `server/genre_routes.rs`；扩展 `server/books_state_routes.rs`、`server/mod.rs`）
- 契约源：`packages/studio/src/api/server.ts` L5915-L5944（truth 写）、L3127-L3142（create-status）、L3036-L3053 / L5702-L5729 / L6086-L6180（genres 列表/详情/复制/创建/编辑/删除）
- 验证：`cargo test --lib`（925）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（36）+ `cargo test --features export-bindings --lib`（1084）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

52 号后剩余轻量域勘测：books/create 主体依赖未移植的 architect 长流程（create_book intent + 基础设定生成）——暂缓；create-status 的磁盘判定分支可独立交付。truth 写端点复用 48 号读面已移植的三重校验件。genres 域六端点为纯文件 CRUD（frontmatter 拼装 + 复制/删除），其底层 `list_available_genres`/`read_genre_profile` 已在 rules_reader.rs 移植完毕。本轮一次交付三组共 7 端点。

## 二、交付内容

### 1. `PUT /api/v1/books/:id/truth/:file`（server.ts L5915）

复用 48 号件（`resolve_truth_file_path` 白名单 / `LEGACY_SHIM_FILES` / `runtime_diagnostic_re` / `is_new_layout_book`），校验序逐字：
1. 白名单外 → 400 `"Invalid truth file"`
2. legacy shim（story_bible/book_rules）且新布局书 → 400 `"Legacy compat shim; edit outline/story_frame.md instead"`
3. runtime 诊断文件 → 400 `"Runtime diagnostic files are read-only"`
4. 无效 JSON / 缺 content / IO 失败 → 500 `{"error":{"code":"INTERNAL_ERROR","message":"Unexpected server error."}}`（TS 走 onError 的逐字形状）
5. 成功 → `{"ok": true}`（自动 mkdir 父目录，roles 嵌套路径可写）

### 2. `GET /api/v1/books/:id/create-status`（server.ts L3127）

内存 bookCreateStatus 无写入方（books/create 暂缓）——直接落磁盘判定分支：`is_book_foundation_complete`（五节：book.json + outline/story_frame + outline/volume_map + book_rules + pending_hooks + roles 任一 tier 卡或 legacy matrix）→ `{status:"ready"}` / 404 `{status:"missing"}`。内存分支待 books/create 移植后补。

### 3. `server/genre_routes.rs`（新建，六端点）

- `GET /genres`：`list_available_genres`（项目级覆盖内置、按 id 排序）+ 每条经三级查找 `read_genre_profile` 附着 language（失败回 "zh"；`?? "zh"` 仅对 null/undefined——空串保持）
- `GET /genres/:id`：`{profile, body}`；任何错误 404
- `POST /genres/create`：缺 id/name → 400 `"id and name are required"`；id 含 `/` `\` `\0` 或 `..` → ApiError 400 `{"error":{"code":"INVALID_GENRE_ID","message":"Invalid genre ID: \"...\""}}`（onError 形状）；frontmatter 12 键固定顺序拼装落盘 `genres/{id}.md`
- `PUT /genres/:id`：同 create，name/id 缺省回路径参数（`p.name ?? genreId`）；缺 profile → 500（TS undefined.name TypeError）
- `DELETE /genres/:id`：项目级删除；缺失 404 `"Genre \"id\" not found in project"`
- `POST /genres/:id/copy`：内置 → 项目复制 → `{"ok":true,"path":"genres/{id}.md"}`

**frontmatter 拼装逐字对齐**：`yamlScalar(v) = JSON.stringify(String(v ?? ""))`（JSON 字符串转义即 YAML 双引号标量）、数组字段 `JSON.stringify(v ?? [])`（serde_json 紧凑形态与 JS 一致：`["成长章"]` / `[1,6]`）、布尔 `?? false`、`language ?? "zh"` 与 `pacingRule ?? ""` 的字段级缺省差异。E2E 对产出文件做整串逐字断言。

### 4. E2E（books53_e2e，3 例）

- truth 写：outline 写入 + 读回 / roles 嵌套自动建目录 / 白名单外 400 / runtime 诊断 400 / 新布局 shim 400 / 无效 JSON 与缺 content 的 onError 形状 500
- create-status：五节缺二 → missing；补齐 + 角色卡 → ready
- genres 全链：列表（builtin + language 附着）→ 详情 → 创建（frontmatter 整串逐字断言 + 项目级覆盖 source=project）→ 编辑（缺省回路径参数）→ 内置复制 → 删除 + 二次 404 → 缺 name 400 / unsafe id ApiError 形状

## 三、parity 要点

1. ApiError 响应形状 `{"error":{"code":..., "message":...}}`（onError 处理器）与本号各 500 的 `INTERNAL_ERROR` 逐字对齐
2. genres/create 校验序：先缺 id/name（400）后 unsafe id（ApiError 400）——server.ts 原文顺序
3. genre frontmatter 的数组用 JSON 序列化内嵌 YAML（`chapterTypes: ["推进章"]`）——TS `JSON.stringify` 直接内嵌的怪癖原样保留
4. create-status 的 404 body 带 `{status:"missing"}`（非 error 形状）

## 四、偏差备案

1. create 的 id/name 非字符串真值（如数字 123）：TS truthy 通过并写 "123.md"；Rust as_str 失败按缺失 → 400（垃圾输入不复刻）
2. `GET /genres/:id` 404 的 error 文案：TS `String(e)` 带前缀 `Error: ...`；Rust 为裸 message（沿 48 号 truth_file 读面同款偏差）
3. update 缺 profile：TS TypeError → onError 500；Rust 500 `INTERNAL_ERROR` 形状一致、触发路径等价

## 五、暂缓件

- `POST /books/create`（create_book intent：architect 基础设定生成长流程 + bookCreateStatus 内存状态机 + SSE book:creating/created/error 三事件）——依赖 architect 域与交互运行时会话机，随后续大件移植；create-status 的内存分支届时补
- `POST /api/v1/style/analyze`（L6184，统计风格画像 + LLM 混合，随 import 域）

## 六、下一步（54 号候选）

1. import/fanfic 域端点（generateStyleGuide / importBook 等 L6184 起）
2. sessions/state 配置域端点（Phase 2 清单：project/detection GET/PUT、model-overrides、global default model 等）
3. Node sidecar 下线核对：books 域 39+3 与 genres 域 6 共 **48 端点**已可切换
4. books/create（architect 域大件）

## 七、影响面

- Rust 业务端点累计：39（52 号后）+ 7 = **46 个**（truth 写 1 + create-status 1 + genres 6；不含 48 号已计的 truth 读）
- 新增测试：e2e +3（books53_e2e）；lib 无新增（全部复用已测件：48 号校验链、rules_reader、outline_paths）
- 新模块 `server/genre_routes.rs`（~290 行）；`write_truth_file` 与既有 `truth_file` 读合并挂载于同一路由（get+put）
