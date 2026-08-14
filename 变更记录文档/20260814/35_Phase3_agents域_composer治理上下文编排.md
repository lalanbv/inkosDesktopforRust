# 35 号｜Phase 3 agents 域：composer 治理上下文编排（plan → compose 链合龙）

> 里程碑：write-next 三段链（planner → composer → writer）的中游合龙。
> composer.ts（1083 行）+ context-assembly（148 行）+ runtime-writer（41 行）+
> persisted-governed-plan（275 行）四模块一次移植。至此 `resolveGovernedPlan` →
> `composeGovernedChapter` → `writeChapter` 的 Rust 侧数据面全部就绪。

## 背景

34 号收官 planner 后，write-next 核心链（runner.ts `createGovernedArtifacts`）只剩
composer 缺口：plan 产物 → 治理上下文包/规则栈/追踪链 → writer 入参。另含两枚
配套件：plan 的持久化复用（无新上下文时跳过 planner LLM 调用）与工件落盘。

## 改动

### 新建 `engine-rs/src/agents/composer.rs`（~1300 行，含测试）

**编排入口**
- `compose_governed_chapter`：上下文收集 → 预算控制 → 规则栈/追踪链 → 三件套落盘
- `ComposeChapterInput/Output` / `ContextBudget` / `ComposeChapterError`
  （ProtectedOverBudget / NoCompiler / EmptyCompiledContext / Compiler / Io）

**端口注入（async trait）**
- `CompressibleContextCompiler`（语义压缩）+ `OutlineSectionSelector`（大纲选段）
  + `CompressionCallback`（同步进度回调）
- `ComposerChat`（温度 + maxTokens 选项）+ `LlmOutlineSelector` / `LlmContextCompiler`
  端口适配（TS ComposerAgent 两个 LLM 方法的等价物）
- `build_outline_selector_messages` / `build_context_compiler_messages`（zh/en prompt
  装配，Rust 侧 golden 候选）

**上下文收集（`collect_selected_context`）**
顺序 load-bearing：memo → current_focus/author_intent/audit_drift/current_state（
legacy 路径透明回退）→ story_frame/volume_map 选段（确定性相关性 + 可选 LLM 语义
选择器，失败静默回退）→ parent_canon/fanfic_canon → 近章轨迹（标题史/情绪节奏/
结尾形态）→ 记忆选集（事实/摘要/卷摘要/hook，复用 33 号 retrieve_memory_selection）
→ hook 债简报（种子摘要 + 最新推进双拍）

**预算控制（`apply_context_budget_if_needed`）**
保护源不可压（超限 ProtectedOverBudget 硬错误）；可压源经编译器产出
`runtime/compiled-compressible-context` 聚合条目；start/end/error 三相压缩事件上报；
TraceCompression 记录保护/压缩来源与 token 预算

**纯函数**：`render_context_entries` / `parse_selected_sources`（fence 剥离 + 对象
截取 + 数组校验）/ `split_markdown_sections` / 相关性判定（硬标题信号 + hint 命中
≥min(2, terms)）/ `fallback_outline_sections` / `extract_match_terms` /
`heading_mentions_chapter` / `slugify_anchor` / `dedupe_by_source` /
`extract_last_meaningful_sentence` / `find_hook_summary`（seed/latest 双模式）/
`render_hook_debt_beat`

### 新建 `engine-rs/src/utils/context_assembly.rs`（~280 行，含测试）

- `build_governed_rule_stack`：mustAvoid/styleEmphasis 派生 L4→L3 逐章覆盖
  （reason 80 码元截断 + `…`）；四层栈 + 三分节 + 三覆盖边
- `build_governed_trace`：保护/可压分层 + token 预算
- `is_protected_context_source`：15 条保护源判定（含前缀族）

### 新建 `engine-rs/src/utils/runtime_writer.rs`（~150 行，含测试）

`write_governed_runtime_artifacts`：`chapter-NNNN.{context.json, rule-stack.yaml,
trace.json}` 三件套并行落盘。**已知分歧**：rule-stack.yaml 用 serde_yaml 而非
js-yaml（语义等价、行折叠/引号风格不同；非 golden 面，此处备案）。

### 新建 `engine-rs/src/pipeline/persisted_governed_plan.rs`（~470 行，含测试）

pipeline 域首个模块（pipeline.rs 从占位转正式 mod）：
- `save_persisted_plan` / `load_persisted_plan`：`chapter-NNNN.plan.md` 权威存取
  （MEMO 标记块 + 严格 parseMemo 重建；任何漂移 → None 重规划）；YAML frontmatter
  旧缓存直接拒绝
- `load_legacy_intent_plan`：plan.md 缺失时回读 intent.md（`## Goal` 等小节），
  memo 降级构造（goal 截 50 码元 / threadRefs 空）
- `relative_to_book_dir`：POSIX 相对路径

### golden 双侧

- TS dump +3 域 7 例：`build_governed_rule_stack`（覆盖派生 + 空 intent）、
  `build_governed_trace`（分层 + 预算）、`is_protected_context_source`（4 例矩阵）
- composer 模块级私有函数不可从 TS dump 访问（TS 无实例括号技巧可用），由 Rust
  单测镜像 TS 行为覆盖（parse_selected_sources 围栏/垃圾 JSON、slugify 回退、
  分段、末行提取 57 截断、相关性/兜底选段、章节命中变体、hook 债双拍）

## parity 要点（移植难点）

1. **regex crate 与 JS 的三处分歧**：①字符类内 `[` 必须转义（`[\[\]\\]`）；
   ②无 lookahead——persisted-plan 的段落截取改**消费式终止符**
   （`(?:\n#{2,3}\s|\z)` 等价还原 `(?=\n#{2,3}\s+|(?![\s\S]))`）；③replacement
   语法差异——escapeRegExp 的等价替换串是 `\$0`（字面反斜杠 + 整体引用）而非
   TS 的 `"\\$&"`
2. **TS `extractSection` 的单行怪癖**：`(?=\n## |\n### |$)` 配 m 标志时 `$` 于
   每个行尾成立——懒惰匹配恒停于首个行尾，实际只捕获标题后**第一行**。Rust 用
   `[^\n]*` 等价还原（legacy intent 回退依赖此语义）
3. **`includes` 子串匹配无数值边界**：`headingMentionsChapter` 里 "chapter 121"
   命中 "chapter 12"；`第${n}章` 无空格形态（"第 3 章" 不命中）——单测固化
4. **UTF-16 窗口**：末行提取 `length > 5` / `slice(0,57)`、legacy memo goal
   `slice(0,50)`、override reason 80 码元截断
5. **`parseInt(file.slice(0,4))` 前缀语义**：章节文件名解析取开头连续数字
   （"0012-x" → 12）
6. **选定条目顺序 load-bearing**：memo → 焦点/意图/漂移/状态 → 大纲 → 正典 →
   轨迹 → hook 债 → 事实 → 摘要 → 卷摘要 → hook（集成测试断言首条 + 含集）
7. **Set 保序去重**：threadRefs 目标 hook 去重、选段 terms 去重、dedupe_by_source
8. **保护源超预算是硬错误**：不做静默截断（InkOS 不压缩作者意图/焦点/硬状态/
   活跃 hook 证据），错误信息逐字移植
9. **压缩事件三相上报**：start（无 message）/end（无 message）/error（含
   message），字段 Option 化对齐 TS 可选字段

## 验证

- **lib 单测**：784 passed / 0 failed（+22 新单测：预算四态[通过/压缩记录/无编译器
  报错/编译失败传播]、compose 全链路 mock 集成[选择器失败回退确定性 + 规则栈派生
  + 三件套落盘]、plan 往返/legacy 回退/frontmatter 拒绝、上下文装配矩阵）
- **golden 差分**：71 域全绿（+3 新域，首次差分零分歧）
- **export-bindings**：943 passed（ContextBudget 补 TS derive）
- **TS 全量**：185 文件 / 1798 测试全绿（leaf dump 重生成）
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **36 号**：write-next 数据面已合龙——两条路线择一：
  (a) 移植 `runner.ts` 的 `_writeNextChapterLocked` 编排（plan → compose → write →
  settle → 审核环）+ task-store 异步模型，挂 `/api/v1/books/:id/write-next` 路由；
  (b) 先移植 reviser.ts（714 行，writeNextChapter 的 audit→revise 环依赖）。
  决策依据：审核环在 runner 内联（auto 模式默认走）——(a) 不含 reviser 则首版
  只支持 skip 审核模式，(b) 先补齐再上 (a) 更完整。倾向 (b)。
