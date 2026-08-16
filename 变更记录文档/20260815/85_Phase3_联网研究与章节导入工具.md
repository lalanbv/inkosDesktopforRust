# 85 号变更记录：联网研究与章节导入工具（research_web / import_chapters）

- 日期：2026-08-14
- 阶段：Phase3 strangler 迁移（engine-rs）
- 契约源：`packages/core/src/agents/researcher.ts`（206 行，runResearchReport 确定性链——**不调 LLM**）、`packages/core/src/utils/web-search.ts`（96 行，Tavily searchWeb + fetchUrl）、`packages/core/src/agent/agent-tools.ts` L962-1042（ResearchWebParams / createResearchWebTool / slugResearchTopic / readResearchSearchConfig）、L1214-1307（ImportChaptersParams / createImportChaptersTool）、`packages/core/src/agent/chapter-import-source.ts`（loadChaptersFromPath）、`packages/core/src/models/project.ts` L120-131（ResearchSearchConfigSchema）、`packages/core/src/agent/agent-session.ts`（注册矩阵：chat 分支带 research+import；play 两分支均无 research；其余分支有 research 无 import）

## 一、背景

agent-session 注册面收尾：research_web（联网研究 → `.inkos/research/` 可追溯报告）与 import_chapters（本地小说文件 → 真实章节 + 反向工程地基 + 逐章回放重建状态）。研究链在 TS 是**确定性资料收集**（搜索 + 抓取 + 固定拼接，无 LLM 参与），完全可移植；导入链复用 59 号已交付的 `import_chapters_chain`（Step1 地基 + Step2 逐章回放）。至此 TS agent-session 注册的工具清单中，Rust 聊天面已覆盖：read/ls/grep、material 双件、propose_action、research_web、import_chapters、play 三件（合计聊天工具面 10 件，按会话类型条件注册）。

## 二、交付

1. **`engine-rs/src/utils/web_search.rs`**（新建，2 单测）：
   - `search_web`：Tavily POST（Bearer 头 + body `api_key/query/max_results/search_depth:"basic"`、15s 超时、`.no_proxy()`）；无 key → 固定错误文案（调用方降级不炸轮次）
   - `fetch_url`：UA/Accept + 15s；HTML → script/style/标签剥离 + `\s+` 折叠 + UTF-16 截断（TS `slice` 语义）
2. **`engine-rs/src/agents/researcher.rs`**（新建，2 单测）：`run_research_report` 逐字——depth 配置（quick 1×3/2、standard 2×4/4、deep 3×5/6）→ purpose 拼音 hint 查询 → URL 去重 → 逐源抓 1800 字符摘录（`first_sentences` 前三句 + 700 码元）→ 确定性 claims（摘录→medium / 仅快照→low）→ confidence（≥3 源无失败 high / ≥1 medium / low）→ unknowns/creative implications（六 purpose 分支文案）→ Markdown 报告（Summary/Claims/Conflicts/Unknowns/Creative implications/Sources/Query log/Partial failures 八节）；搜索/抓取经 `ResearchTransport` trait 注入
3. **`engine-rs/src/interaction/research_tool.rs`**（新建，2 单测）：`read_research_search_config`（inkos.json researchSearch 节，缺失/损坏回退禁用）、`TavilyTransport`（enabled 才带凭据——**禁用 ≠ 不调**，无 key 查询失败进 partialFailures、报告仍产出，TS 语义）、`tool_research_web`（topic/purpose/depth 枚举校验 → 报告落 `.inkos/research/{iso-slug}.md` → 三行文本 + details `research_report`）、`slug_research_topic`（NFKC + 60 码元 + "research" 回退）、schema 逐字
4. **`engine-rs/src/interaction/import_chapters_tool.rs`**（新建，3 单测）：
   - `resolve_tool_book_id`：缺失 → "import_chapters requires bookId when there is no active book."；显式与 active 不一致 → "must match the active book."；safe id 校验
   - `load_chapters_from_path`：目录模式（.md/.txt 按名排序、标题去扩展名 + 前导数字前缀 `03_风暴`→`风暴`）；单文件模式（`split_chapters` + 自定义正则；无章 → 固定提示文案含"第X章/第X回"）
   - `tool_import_chapters`：既有章节守卫（TS 逐字 "Book \"X\" already has N chapter(s). Pass resumeFrom=<n> …"）→ resumeFrom>1 / series 模式 → 暂缓错误文案 → 复用 `import_chapters_chain`（签名改为直收 `&[SplitChapter]`，endpoint 侧切分）→ 四段结果文本 + details `chapters_imported`（importMode 恒 continuation）
   - schema 逐字（resumeFrom/importMode 描述保留，Rust 链暂只支持全量重建）
5. **注册**：`ChatToolRouter` 扩展（propose → **research/import** → play → 文件四级分发）；payload 条件对齐 TS 矩阵——research：非 play 会话；import：仅 chat 分支
6. **E2E**（+2）：
   - `research85_e2e`：inkos.json 配置 mock Tavily（Bearer 断言 + purpose hint 查询捕获）+ HTML 抓取源 → 聊天 research_web → 三行文本（2 源 medium/无失败）+ 报告落盘（标题/来源节/摘录前三句不含第四句/Query log）
   - `import85_e2e`：聊天驱动全链导入（architect/analyzer mock）→ 2 章落盘 + index imported + 四段文本；二次导入 → "already has 2 chapter(s)" 守卫错误卡；无 bookId → "requires bookId" 守卫错误卡

## 三、parity 要点

- **研究链零 LLM**：TS researcher 不调模型——claims/confidence/creative implications 全部确定性拼接，Rust 逐字对齐（含 quick/standard/deep 三档查询数与抓取数）
- **禁用 ≠ 不调**：researchSearch.enabled=false 时仍发起搜索（无凭据必败）→ partialFailures 记录 → 0 源 low 报告照常产出——降级不炸轮次
- **导入守卫先于执行**：既有章节且无 resumeFrom → 固定错误（防误覆写）；bookId 解析三层（显式/active/缺失）
- **chain 复用不复制**：`import_chapters_chain` 签名从 text+regex 改为直收章节列表（endpoint 与聊天工具同一入口），目录模式章节标题原样保留（无章号注入）
- **注册矩阵对齐**：play 两分支（有/无世界）都不挂 research；import 只在 chat 分支——与 TS agent-session 完全一致

## 四、偏差备案

1. **resumeFrom>1 续放暂缓**：Rust 导入链为全量重建（等价 resumeFrom=1）；>1 时返回自拟错误文案——TS 保留已有地基从 N 章重放，待导入链支持增量后续轮补
2. **importMode=series 暂缓**：Rust 链恒 continuation（同时空新故事）；series 返回自拟错误
3. **Tavily 错误文案细节**：TS 失败响应附 body 文本；Rust 同（`Tavily search failed: {status} {body}`）；网络错误 TS 为原生 fetch 报错文案、Rust 为 reqwest 文案——前缀语义一致
4. **searchWeb 的 provider 字段**：配置 schema 的 provider 枚举（tavily/custom）Rust 未消费（TS 也仅存储不分支）——baseUrl 即自定义入口
5. **聊天卡 details 不外露**（84 号备案延续）：research/import 的 details（sources/claims/importedCount）待 66 号范围轮次

## 五、暂缓件

- resumeFrom 增量续放 + importMode=series（导入链增量能力）
- 聊天面卡 details 外露（66 号范围）
- sub_agent / generate_cover / 书会话写工具（book session 工具集，edit/book 分支）
- PDF 文本抽取（83 号备案延续）；zh 提示词逐字对齐（82 号备案延续）
- 散件：单章写作中途截断、/agent model 校验、resumeFrom（REST 面）、fetchWithProxy、attachments 归一化、模型四层解析

## 六、验证基线（2026-08-14）

- `cargo test --lib`：**1110**（+9：web_search 2 + researcher 2 + research_tool 2 + import_chapters_tool 3）
- `cargo test --test golden_leaf`：76
- `cargo test --test e2e_write_next_contract`：**144**（+2：research85/import85）
- `cargo test --features export-bindings --lib`：**1269**（+9）
- `cargo clippy --lib --tests --bins`：零警告
- TS：`packages/core` vitest 185 文件 / **1798** 测试全过

## 七、影响面与下一步

- 影响面：`book_create_routes.rs` 的 `import_chapters_chain` 签名变更（text→chapters；调用方 endpoint 已同步）；聊天工具面达 10 件（read/ls/grep + material 双件 + propose_action + research_web + import_chapters + play 三件），agent-session 注册矩阵在 Rust 聊天面全数落地
- 下一步（86 号候选）：**首选聊天面卡 details 外露**（66 号范围——propose/research/import 卡的结构化消费面，前端确认卡的直接依赖）；其次书会话工具集（sub_agent/generate_cover/写工具，edit/book 分支）；或散件收尾（单章截断/model 校验/resumeFrom REST/attachments/模型四层解析）
