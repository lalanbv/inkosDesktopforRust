# 90 号变更记录：叙事预测域全链（narrative forecast 三件套）

## 一、背景

89 号把 agent-session 聊天工具注册矩阵对齐 TS 真值表后，book 会话仅剩 forecast 三件（create/get/select narrative forecast）未落地——TS 侧是独立域 `packages/core/src/forecast/`（RFC #342，约 1132 行），Rust 侧此前仅有 schema + 模型输出解析。本轮（90 号）移植域本体全链（store/context-builder/prompts/agent/render/runner）并接入聊天面，闭合 book 会话注册矩阵的最后缺口。

## 二、交付

### 1. forecast 域本体（六个新文件）

- **`store.rs`**：`story/runtime/narrative-forecasts/` 安全边界存储——`assert_safe_forecast_id`（`^[A-Za-z0-9][A-Za-z0-9_-]{0,79}$`）；id 派生（`fc-YYYYMMDD-HHMMSS` 时间戳基名 + 已存在追加 -2/-3…）；save（校验先行 + pretty JSON 尾换行 + comparison trimEnd 尾换行）；load（缺文件 → 可用清单逐字错误、JSON 损坏与 schema 失败两段错误面）；list（含 forecast.json 的 id 排序）；mark_stale；write_selected_plan。
- **`context_builder.rs`**：正史只读上下文（零副作用——不走会种默认值的控制文档面）。九段 sections（作者意图/当前聚焦/当前状态/伏笔/故事框架/卷映射/近期摘要 ≤8/人物/支线看板，读法复用 planner-context 与 outline-paths 助手）；**内容指纹**：sha256 over `{"baseChapter":n,"files":[[排序后的相对路径,内容]...]}`——固定 12 路径 + `story/state/*.json`（排序）+ roles 四层目录；`story/runtime/**` 刻意不在列（预测不会失效自己）；baseChapter = 磁盘最高章号；双语 markdown 渲染（空段滤除）。
- **`prompts.rs`**：system/user/repair 三构建器双语逐字（互斥分支/规划材料非正文/尊重正史/纯 JSON 输出；JSON 形状模板含起始章号插值）。
- **`agent.rs`**：`ForecastChat` 端口（chat + temperature + maxTokens）+ RoutedAgent 实现（agent 名 "forecast"）；单次调用 + 校验驱动一次重试（0.6 → 0.4 降温；分支数不符 `narrative forecast model returned N branches, expected exactly M.`）；maxTokens = `max(8192, branches*(horizon*220+1600))`。
- **`render.rs`**：comparison.md（表头 + 逃逸表格行 `|`→`\|`、换行→空格 + 每分支小节）与 selected-branch-plan.md（stale 警告行 + 分支小节复用）双语逐字；joinOrNone 全角分号连接（zh/en 同款）。
- **`runner.rs`**：三操作——create（守卫：空分歧/branchCount 2-5/horizon 1-10 越界逐字错误；book 存在性检查；上下文 → 投影 → branch-1..N 确定性派生 → 落盘）、get（stale 重检 + active 时持久化标记）、select（分支存在性逐字错误 + 只写计划文件）。
- **`schema.rs` 增**：`validate_narrative_forecast`（zod 约束手工等价：version=1/非空串集/horizon 界/分支数 2-5/branchId 正则与去重/beat 章号 ≥1/score ≤100 等）。

### 2. `interaction/forecast_tools.rs`（聊天三件壳）

- create/get/select 三执行器：守卫（resolve_tool_book_id 复用 + 必填 divergence/forecastId/branchId）、文本与 details 逐字（`Narrative forecast {id} created with N isolated branches.` + 分支行 `branch-1 "标题" — intent fit 88/100, 1 risk(s), premise: ...` + 工件路径行 + 非正史提示；get 的 `Forecast {id} (book b1) — status: active/stale.` + WARNING 行；select 的 `Selected branch-1 "..." from forecast ...` + 计划路径行）；details 键集对齐（narrative_forecast_created/narrative_forecast/narrative_branch_selected）。
- `forecast_tool_schemas()` 三 schema 逐字 + `execute_forecast_tool` 分发器。

### 3. `server/agent_route.rs`：注册与分发

- book/book-create 会话注册 forecast 三件（TS edit 过滤器剔除 forecast——edit 会话不注册）；`ChatToolRouter` 七级分发（propose → research → import → sub_agent → 编辑工具族 → **forecast 三件** → play → 文件工具）。

### 4. 依赖与测试

- Cargo.toml 增 `sha2 = "0.10"`（纯 Rust、零系统依赖，与 rustls/bundled sqlite 的自包含哲学一致）。
- 单测 +8：agent 三件（首过无重试 + 无效输出重试一次降温 + 分支数不符错误逐字）、store 三件（save/load/list/stale 标记 + id 后缀碰撞 + 时间戳格式与安全 id）、runner 一件（create→get→select 全链含守卫错误、工件断言、新章后 stale 持久化、stale 后选择带警告）、forecast_tools 一件（schema 形状）。
- E2E `mod sub90_e2e`（2 测试）：book 会话经 `/api/v1/agent` 的 create（mock 投影代理 2 分支 → 卡片文本/分支描述行/details + forecast.json 与 comparison.md 落盘 + 表格行断言）→ get（active；正史加章后 stale 持久化 + WARNING 行）→ select（计划文件 + 标题/分支行）；edit 会话不注册 forecast 三件。

## 三、parity 要点

- 三件工具 schema、守卫文案、成功文本、details 键集逐字。
- 指纹算法逐字（canonical JSON 键序 = 字母序、数组条目 [path, content]、sha256 hex）；过期语义逐字（状态为 stale 直接真，否则指纹比对；get 持久化标记，select 只读判定）。
- store 错误文案逐字（`Narrative forecast "X" not found. Available forecasts: ...` / `has corrupted forecast.json` / `failed schema validation`）。
- 模型输出容错链（代码栅栏剥离/JSON 对象切片/控制字符与尾逗号清理）沿用既有 schema.rs 解析器。

## 四、偏差备案

1. **确定性注入省略**：TS store 的 `ForecastStoreOptions.now/idFactory` 仅供测试注入时钟；Rust 用真实时钟（utc_now_iso），测试以行为断言（前缀/后缀碰撞）替代固定 id。
2. **zod 错误文本**：`validate_narrative_forecast` 报"哪条约束不过"，非 zod 展开格式（两侧都是内部错误面，最终用户可见文案由 store 包装的固定前缀承担）。
3. **chapters 派发**：create 后 baseChapter 断言依赖磁盘最高章号（resolveBaseChapter 正则 `^(\d+)[_-]?.*\.md$` 与 TS 一致）。
4. **abort 信号**：TS create 透传 AbortSignal 给 agent 上下文；Rust 聊天回环的取消面在外层（会话中止即断流），代理层无独立信号（与 reviser/writer 等域代理同 reductions）。

## 五、暂缓件（滚动）

- forecast 的 REST 面（TS 侧无独立 forecast REST 端点——工具面即唯一入口，无缺口）。
- 确认单工具面收敛（short_run/script_create 等聊天注册 + architectCreateOnly 变体）。
- 既有备案落地：PDF 文本抽取（83 号）、play zh 提示词逐字（82 号）、resumeFrom 增量续放与 importMode=series（85 号）、generate_cover 端点覆盖参数（89 号）。
- 散件收尾：单章写作中途截断、/agent model 校验、resumeFrom REST、fetchWithProxy、attachments 归一化、模型四层解析。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1125** 过（+8） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **154** 过（+2：sub90 两件） |
| `cargo test --features export-bindings --lib` | **1284** 过（+8） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（91 号候选）

agent-session 聊天工具面**全数对齐且无已知缺口**（book 16 件、chat 8 件、edit 9 件、play 5 件——按 TS 真值表）。下一轮候选：

1. **首选：确认单工具面收敛**（TS confirmed 分支的整组替换工具集——short_run/script_create/storyboard_create/interactive_film_create/translation_create 聊天注册 + sub_agent architectCreateOnly 变体；这是 agent-session 工具矩阵的最后一块"确认态"拼图）。
2. 其次：既有备案落地（PDF 抽取/play zh 逐字/resumeFrom 增量/cover 端点参数）。
3. 散件收尾：单章写作中途截断、/agent model 校验、resumeFrom REST、fetchWithProxy、attachments 归一化、模型四层解析。
4. 或转向 sidecar 对齐差分：以 TS 集成测试清单为基线，对已移植端点做一轮契约差分扫描（strangler 切换前的系统对齐）。
