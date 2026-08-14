# 34 号｜Phase 3 agents 域：planner 编排本体（context 提取器 + 提示词 + planChapter）

> 里程碑：planner 链全量收官。planner-context（上下文提取器纯函数群）+
> planner-prompts（系统提示词 zh/en 双语逐字 + 用户模板 + 黄金三章指引）+
> planner.ts 编排本体（`PlannerChat` trait + planChapter + memo 3 次重试链 +
> 降级 fallback + intent markdown 落盘）。33 号数据供给层在此汇流。

## 背景

33 号就绪了 memory-retrieval / planning-materials 数据入口。planner.ts（879 行）
的依赖至此全部可满足：ChapterIntent/ChapterMemo（1 号类型层）、parseMemo
（chapter-memo-parser，早前已移植）、renderHookSnapshot/renderSummarySnapshot
（32/33 号）、gatherPlanningMaterials（33 号）。

## 改动

### 新建 `engine-rs/src/agents/planner_context.rs`（~600 行，含测试）

**读取器**：`read_character_matrix`（roles/ 优先，storyDir.parent() 反推 bookDir）、
`read_subplot_board` / `read_emotional_arcs` / `read_pending_hooks` / `read_brief`、
`read_book_rules_block`（Phase 5 权威加载 → 主角人设锁/行为约束/禁忌/题材锁/
同人模式/正文指引的紧凑 markdown 块）

**纯提取器（golden 守门）**：`format_recent_summaries`（末 N 行 + 表头重渲染）、
`compose_current_arc_prose`（活跃支线 + 近期情感线拼装，表格/bullet 双形态）、
`extract_protagonist_row`（显式主角关系 → 首个非头行回退）、
`extract_opponent_rows` / `extract_collaborator_rows`（关系正则 + 主角行排除）、
`extract_relevant_threads`（pending_hooks + subplot_board 活跃行，头行/陈旧行过滤）、
`format_recyclable_hooks`（zh/en，状态原文经 `hook_status_text` 透传）

### 新建 `engine-rs/src/agents/planner_prompts.rs`（~500 行，含测试）

- `PLANNER_MEMO_SYSTEM_PROMPT` / `_EN`：**zh/en 系统提示词逐字**（15 条工作原则 +
  严格输出格式 + hook 账硬规则），raw string 三重 `#` 定界（正文含 `"##` 序列）
- `PLANNER_MEMO_USER_TEMPLATE` / `_EN`：13 占位符用户模板
- `get_planner_memo_system_prompt` / `get_planner_memo_user_template`（语言选择）
- `build_planner_user_message`：模板全量填充 + 黄金三章指引条件追加（≤3 章）
- `build_brief_block` / `build_chapter_context_block`（空串门控，非空时各自
  包装成最高优先级指令块）
- `build_golden_opening_guidance`（zh/en 散文指引，章节号内插）

### 新建 `engine-rs/src/agents/planner.rs`（~1500 行，含测试）

**编排入口**
- `PlanChapterInput` / `PlanChapterOutput` / `PlanChapterError`
- `PlannerChat` trait（`#[async_trait]`，复用 AuditorChat/WriterChat 注入模式）
- `plan_chapter`：种子材料 → find_outline_node → derive_goal → 权威规则 prohibitions
  → must_keep/must_avoid/style_emphasis → gather_planning_materials → 活跃 hook 计数
  → arc_context → ChapterIntent → plan_chapter_memo → **memo.goal 回写 intent.goal**
  → intent markdown 渲染 → `story/runtime/chapter-NNNN.intent.md` 落盘
- `plan_chapter_memo`：五路并行上下文读 → user message 装配 → 3 次重试
  （`## 上次输出的错误` 反馈块注入）→ 全败降级 fallback memo（zh/en 双语，可解析）

**确定性推导链（golden 守门）**
- `derive_goal`：本章指令首行 → current_focus 局部覆盖 → 大纲节点 → focus 聚焦 →
  author_intent → 默认句
- `find_outline_node` 四级匹配：精确章行（内联→下一行）→ 范围行（内联→节拍编号→
  小节→下一行）→ 首锚点行 → 全文首指令
- `extract_section`（标题层级状态机：同级截断/更深层目标标题重置 buffer）、
  `collect_must_keep/avoid/style_emphasis`、`build_arc_context`（占位文件守卫）、
  `is_golden_opening_chapter`（zh ≤3 / en ≤5）、`render_hook_budget`（容量 12，
  ≥10 告警）、`render_intent_markdown`

## parity 要点（移植难点）

1. **outline 正则的负向先行**：TS `(?!\d|\s*[-~–—]\s*\d)` 防「12 误配 123」「精确
   行误配范围行」，Rust regex 无 lookahead → 捕获数字 + `tail_is_exact_safe`
   手动校验（首字符非数字 + 非「空白*破折号空白*数字」形态），golden
   `find-outline-tricky-numbers` 例固化（123 与 12-15 均不误配 12）
2. **raw string 定界层级**：系统提示词正文含 `"## 当前任务"`（引号+双井号）→
   `r##"` 不够，须 `r###"..."###`；模板只需 `r##`
3. **范围行内联优先**：`第 1-5 章 试炼` 的「试炼」先于节拍编号返回（TS
   inline-first，首版测试预期写反被单测抓出）
4. **`isGoldenOpening` 的 activeHook 计数**：TS 对原始 status 串做大小写敏感
   全等（`!== "resolved"`），Rust 经 `hook_status_text` 同语义比较（不走枚举
   归一化，"Resolved"/"已解决" 仍计数——TS 怪癖固化）
5. **memo.goal 截断显示**：兜底 goal「Continue chapter 1 according to the current
   outline」51 码元 > 50 上限 → `make_display_goal` 截 47 码元 + `"..."`（golden
   未覆盖 display 层，单测固化）
6. **fallback memo 结构**：zh/en 各 15 行小节，每小节均满足 parse_memo 的 ≥20
   码元内容门槛（首版桩内容过短导致 fallback 测试连锁失败，追加真实长度内容）
7. **`extract_first_directive` 不剥「第 N 章」前缀**：真实链路中该串已被
   find_outline_node 剥过，derive_goal 只透传（单测固化语义）
8. **outline 节点不直接进 user prompt**：模板无 outlineNode 占位符——节点只经
   goal 推导链间接生效（externalContext 优先时节点不出现在 prompt）

## 验证

- **lib 单测**：762 passed / 0 failed（+22 新单测：goal 链/大纲四级匹配/arc 守卫/
  黄金窗口/预算阈值/intent markdown 形状/MockChat 全链路集成 ×3——成功流 / 重试后
  成功 / 耗尽降级 zh+en / fallback 可解析性）
- **golden 差分**：68 域全绿（+10 新域 41 例：system prompt zh/en 全文、user
  message zh/en、黄金指引、六个 context 提取器、recyclable hooks（含
  status="pressured" 原文）、planner 私有套件 16 例经 TS 实例括号访问取真值）。
  34 号代码**首次差分即零分歧**——33 号沉淀的 parity 纪律生效
- **export-bindings**：920 passed
- **TS 全量**：185 文件 / 1798 测试全绿（leaf dump 重生成）
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **35 号**：planner 与 writer 均已就绪——按《HTTP端点登记与迁移排期》挂
  `/api/v1/books/:id/plan` + `write-next` 路由（task-store 异步模型 + SSE 进度），
  或先移植 composer.ts（plan → compose 链的下一环：意图 → 结构化场景规划）。
  决策依据：write-next 端点的 Node 侧编排是否直接依赖 composer 产物。
