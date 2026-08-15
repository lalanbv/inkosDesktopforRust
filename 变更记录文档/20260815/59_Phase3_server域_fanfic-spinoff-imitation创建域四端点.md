# 59 号变更记录：Phase3 server 域——fanfic/spinoff/imitation 创建域四端点

- 日期：2026-08-15
- 范围：engine-rs（新增 `agents/fanfic_canon_importer.rs`、`server/fanfic_routes.rs`；扩展 `agents/architect.rs`、`agents/mod.rs`、`server/book_create_routes.rs`、`server/style_routes.rs`、`server/mod.rs`、`llm/agent_router.rs`）
- 契约源：`packages/studio/src/api/server.ts` L6259-L6329（fanfic/init、fanfic/refresh）、L6333-L6383（spinoff/init）、L6387-L6420（imitation/init）；`packages/core/src/pipeline/runner.ts` L952-L1028（importFanficCanon + initFanficBook）、L1037-L1073（initSpinoffBook）、L1083-L1093（initImitationBook）、L194-L215（buildSpinoffFoundationContext）；`packages/core/src/agents/fanfic-canon-importer.ts`（201 行）；`packages/core/src/agents/architect.ts` L1098-L1153（generateFanficFoundation）
- 验证：`cargo test --lib`（941，+2）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（52，+2）+ `cargo test --features export-bindings --lib`（1100）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

58 号后创建链仅剩衍生形态：同人（canon/au/ooc/cp 四模式）、番外（复用正传正典）、仿写（原创作 + 强制风格向导）。本轮补齐 FanficCanonImporter 与 generateFanficFoundation 两个核心件并装配四端点——创建域全形态闭环。

## 二、交付内容

### 1. `agents/fanfic_canon_importer.rs`（新建，~240 行）

- `import_from_text`（temp 0.3）：五段 SECTION 提取提示逐字（世界规则/角色档案 8 列表/关键事件时间线/力量体系/写作风格 7 项）→ `=== SECTION: {tag} ===` 提取 → `fanfic_canon.md` 全文档拼装（五节 + 缺失占位文案 + meta 块 sourceFile/fanficMode/generatedAt）
- **超长源文本编译**：>50k UTF-16 码元先分块（UTF-16 语义）逐段编译（temp 0.2，「同人正典资料编译器」提示逐字）为「语义资料包」——非截断；系统提示追加编译说明段
- 单测 2 例：五段提取与缺失段/UTF-16 分块

### 2. `architect.rs` 增 `generate_fanfic_foundation`（~60 行）

- temp 0.7；四模式指令逐字（canon 不可改原作事实 / au 标注分歧点 / ooc 偏离需驱动 / cp 关系线主线）；「新时空要求」四条（分岔点/独立冲突/5 章引爆/场景新鲜度 ≥50%）+ 原作正典块 + 输出契约（主角色须来自原作、原创配角标注）
- reviewFeedbackBlock 语言 `book.language ?? "zh"`（TS 原文如此——与其他方法不同）

### 3. `server/fanfic_routes.rs`（新建，~560 行，四端点）

| 端点 | 行为 |
|---|---|
| `POST /fanfic/init` | title/sourceText 必填 400；端点自带配置（other 题材/100 章/3000 字/fanficMode/书名派生 id）；**同步执行**（TS await）；SSE `fanfic:start/complete/error`；`{ok, bookId}` |
| `POST /books/:id/fanfic/refresh` | sourceText trim 空 400；按书 fanficMode（缺省 canon）重导 `fanfic_canon.md`；SSE `fanfic:refresh:*`；`{ok}` |
| `POST /spinoff/init` | title/parentBookId 必填 400；parent 缺失 404 逐字；配置回退 parent（题材/平台/章数/章长/语言）；409；**后台**创建（复用 bookCreateStatus 状态机）；SSE `spinoff:start/complete/error` + `book:created`；`{status:"creating", bookId}` |
| `POST /imitation/init` | title/referenceText/storyIdea 必填 400；409；**后台**：initBook（storyIdea 为外部指令 → brief.md）+ **强制风格向导**（失败上抛——区别于其他链的吞错）；SSE `imitation:*` + `book:created` |

**两条 init 链**（逐段对齐 runner.ts）：
- `init_fanfic_book`：saveBookConfig（直接目标目录）→ fanfic_canon 导入 → 审核环（**fanfic 模式 + sourceCanon** 传入审核上下文）→ 落盘 → 控制文档 → 风格向导（≥500 吞错）→ chapters/ + 空索引 + 快照 0
- `init_spinoff_book`：saveBookConfig → **importCanon**（55 号件 pub 化复用，产 parent_canon.md）→ `build_spinoff_foundation_context`（双语逐字：番外独立侧篇 + 方向 + 正传正典复用块）→ 审核环（original 模式）→ 同尾；控制文档 author_intent 用 direction

### 4. E2E（fanfic59_e2e，2 例，mock 五分派）

- fanfic/init 全链：canon 五段提取 + meta + 地基 + 角色卡 + 快照 + 风格向导 + SSE 两事件 → 缺 sourceText 400 → refresh 重导 + 空 400
- spinoff/imitation：后台生命周期（creating 响应/parent_canon/地基/brief.md/风格向导/SSE 事件序）→ parent 404 / 必填 400 三例

## 三、parity 要点

1. fanfic/init 与 spinoff/init 的执行模型不同：**fanfic 同步等待**（TS L6289 await）、**spinoff/imitation 后台 void**（L6367/L6410）——响应语义随之不同（{ok} vs {status:"creating"}）
2. fanfic 端点配置不走 buildStudioBookConfig（默认 100 章/other 题材——TS L6272 独立构造）
3. 审核 fanfic 模式传入 **sourceCanon**（审核提示带「原作正典参照」块）——原创模式不带
4. imitation 的风格向导**失败上抛**（链失败 → 状态机 error），其余链吞错
5. 50k 阈值按 UTF-16 码元（TS length/slice 语义）

## 四、偏差备案

1. 后台链错误文案为链内短消息（TS String(e) 长文案；状态码/SSE 事件一致）
2. SECTION 提取正则的 lookahead 改消费式（Rust regex 限制；各段独立提取不受影响）
3. spinoff 完成事件 `book:created` 不带 book 摘要（TS loadStudioBookListSummary 摘要——省略，事件名与 bookId 一致）

## 五、暂缓件

- fanfic 端点的 `worldPremise/protagonist/...` 等创作草案字段（TS fanfic/init body 未接收——已对齐；交互运行时会话的 creationDraft 路径随会话机）
- `reviseFoundation`（架构稿修订）
- `waitForStudioBookReady`（studio 前端轮询辅助，非 HTTP 面）

## 六、下一步（60 号候选）

1. skills / prompt-packs 轻域端点（L4209-L4268）
2. `GET/PUT /project/files|artifacts` 文件浏览面
3. Node sidecar 下线核对：累计 69+4=**73 端点**已可切换；创建域全形态（原创/导入/同人/番外/仿写）闭环，可整体下线 Node 的 books/genres/project 配置面
4. resolveEffectiveLLMConfig 全链（env 层 + services 预设——LLM provider 配置域大件）

## 七、影响面

- Rust 业务端点累计：69（58 号后）+ 4 = **73 个**
- 新增测试：lib +2（fanfic_canon_importer）、e2e +2（fanfic59_e2e）
- 新端口装配：`FanficCanonImporterChat` 入 AgentRouter；55 号 `import_canon` 与 58 号 `init_book`/状态机件 pub 化复用
- 创建域五形态（原创 books/create、导入 import/chapters、同人 fanfic/init、番外 spinoff/init、仿写 imitation/init）全部在 Rust 侧闭环
