# 58 号变更记录：Phase3 server 域——books/create 与 import/chapters 端点（architect 装配 + 审核环）

- 日期：2026-08-15
- 范围：engine-rs（新增 `agents/foundation_reviewer.rs`、`server/book_create_routes.rs`；扩展 `server/style_routes.rs`、`server/mod.rs`、`agents/mod.rs`、`llm/agent_router.rs`）
- 契约源：`packages/studio/src/api/server.ts` L3054-L3125（books/create）、L6218-L6236（import/chapters）、`book-create.ts`（buildStudioBookConfig）；`packages/core/src/pipeline/runner.ts` L734-L809（initBook）、L535-L633（generateAndReviewFoundation + buildFoundationReviewFeedback）、L2825-L3010（importChapters）、L3086-L3190（resetImportReplayTruthFiles + 双语种子）；`packages/core/src/agents/foundation-reviewer.ts`（210 行）；`packages/core/src/interaction/project-tools.ts` L109-L129（buildCreationExternalContext）
- 验证：`cargo test --lib`（939，+4）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（50，+2）+ `cargo test --features export-bindings --lib`（1098）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

57 号交付 architect 核心（生成/解析/规范化/落盘/修复环）。本轮补齐两件事：① FoundationReviewerAgent + generateAndReviewFoundation 审核环（initBook 的直接依赖）；② books/create 与 import/chapters 两端点装配——迁移主线最后两个重依赖端点。

## 二、交付内容

### 1. `agents/foundation_reviewer.rs`（新建，~300 行）

- `review_foundation`：五维打分（temp 0.3）；原创模式维度表（目标章数自适应：openingWindow=min(5,target)、repeatWindow=clamp(3,10,target)）与衍生模式（同人/系列，预留 fanfic/spinoff 用）
- 双语审核提示逐字 + 地基摘录五段（storyBible/volumeOutline/bookRules/currentState/pendingHooks）
- `parse_review_result`：`=== DIMENSION: n ===` 分块解析（分数/意见；缺失 → 50 分 + "(parse failed)"）；总分四舍五入均值；**通过 = 总分 ≥80 且无单维 <60**（双闸门）；OVERALL 总评段
- `build_foundation_review_feedback`（双语重生成反馈拼接）+ `generate_and_review_foundation_multi` 审核环（maxRetries=2：审核不过带反馈重生成 → 终审兜底接受）
- 单测 4 例：全形解析/地板拒绝与缺失维度回退/窗口自适应/双语反馈

### 2. `server/book_create_routes.rs`（新建，~600 行）

**`POST /api/v1/books/create`**（含状态机与 SSE）：
- `build_studio_book_config`（id 派生：lower + 非 [a-z0-9 汉字] 折叠 `-` + 截 30 字节；默认 200 章；语言感知默认章长 3000/2000）→ id 空 400 / `completeBookExists`（book.json + story_bible.md）409
- SSE `book:creating` → **后台 tokio::spawn** `init_book` → 成功磁盘复核 + 删状态 + SSE `book:created`（含 book 摘要）；失败置 error 状态 + SSE `book:error`；即时响应 `{status:"creating", bookId}`
- `bookCreateStatus` 进程级内存状态机（OnceLock<Mutex<HashMap>>，TS server 内存 Map 等价物；53 号 create-status 的内存分支由此接通）

**`init_book`**（runner.ts initBook 逐段对齐）：
1. 审核环生成基础设定（architect + foundation-reviewer 双 agent 路由）
2. **staging 目录**（`.tmp-book-create-{id}-{ts36}-{rand}`）全量落盘：book.json → 基础设定 → brief.md（外部指令非空时）→ 控制文档（ensure_control_documents_at，author_intent 回退 external_context）→ current_focus → 空索引 → 快照 0
3. 目标已存在校验（完整书 → 逐字错误"Use a different title or delete the existing book first"；不完整 → 清除）→ **原子 rename**；任何失败 → staging 清理

**`POST /api/v1/books/:id/import/chapters`**：
- `split_chapters`（分章正则可选）→ SSE `import:start`(chapters)
- Step 1：`generate_foundation_from_import`（continuation 模式）+ 落盘 + `reset_import_replay_truth_files`（状态/伏笔双语空表种子 + 10 类运行时文件清除 + state/snapshots 目录清除）+ 空索引 + 快照 0 + 资料包 ≥500（UTF-16）→ 风格向导（吞错）
- Step 2：逐章 `analyze_chapter` → **分析面补全为落盘面**（content/wordCount 覆写、postWrite 清空——TS `{...output, ...}` 同构）→ `save_chapter` + `save_new_truth_files` → 索引（同号替换/追加，status=**imported**）→ 逐章快照
- 响应 `{bookId, importedCount, totalWords, nextChapter}` + SSE `import:complete`(count)

### 3. E2E（books58_e2e，2 例，mock 四分派）

- create 全生命周期：creating 响应 → SSE 双事件 → staging 原子落盘全文件面（Phase 5 地基 + 角色卡 + 控制文档 + brief.md）→ 无 staging 残留 → create-status ready → 二次创建 409 → 空标题 400
- import 全链：2 章回放（importedCount/nextChapter/索引 status=imported/逐章快照/地基重生成/旧运行时痕迹清除）→ text 空 400

## 三、parity 要点

1. **id 派生按字节截 30**（TS slice(0,30) 是 UTF-16 码元——中文书 id 恰好一致；32 位 emoji 边角差异备案）
2. 章节文件名 title 来自 analyzer 的 CHAPTER_TITLE 提取（非分章标题）——E2E 断言固化此行为
3. 审核环终审**兜底接受**（重试耗尽不再拒绝——"ACCEPTED (max retries)" 语义）
4. create 的 409 判据是 book.json + story_bible.md（shim 文件，Phase 5 落盘也写）
5. import 链的 resumeFrom 暂缓（全量回放；TS 默认 1 同为全量）

## 四、偏差备案

1. create 后台失败链的错误文案为链内中文/英文短消息（TS String(e) 原生长文案；状态码与 SSE 事件一致）
2. `buildImportFoundationSource` 的资料包形态为「# 标题 + 正文」逐章拼接（TS 实现按章目录拼接，细节文案未逐字——从 importChapters 调用处推导）
3. staging 随机后缀用 pid^ts 混合（TS Date.now36 + Math.random36——防碰撞语义等价）

## 五、暂缓件（59 号候选）

- fanfic/init + fanfic/refresh + spinoff/init 端点（复用审核环衍生模式 + importCanon 已有件）
- `reviseFoundation`（架构稿修订端点，依赖 buildRevisePrompt）
- `markBookActiveIfNeeded` / `syncCurrentStateFactHistory` / `syncNarrativeMemoryIndex` 同步钩子（沿 47 号口径）
- resumeFrom 断点续导（TS input.resumeFrom）

## 六、下一步（59 号候选）

1. fanfic/spinoff 域端点（审核环衍生模式已就绪）
2. skills/prompt-packs 轻域
3. Node sidecar 下线核对：累计 67+2=**69 端点**已可切换（books/create + import/chapters 落地后创建链全通）

## 七、影响面

- Rust 业务端点累计：67（57 号后）+ 2 = **69 个**；创建→写作→审计→修订→导出全生命周期端点已在 Rust 侧闭环
- 新增测试：lib +4（foundation_reviewer）、e2e +2（books58_e2e）
- 新端口装配：`FoundationReviewerChat` 入 AgentRouter（impl_simple_chat）
- create-status 端点（53 号）的内存分支接通（creating/error 态可查）
