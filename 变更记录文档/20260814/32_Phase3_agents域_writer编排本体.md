# 32 号｜Phase 3 agents 域：writer 编排本体（章节创作 + 状态结算 + 原子落盘总装）

> 里程碑：writer.ts（1502 行）全量移植——agents 域最大编排体。Phase 1 创作 +
> Phase 2 结算（observer→settler，delta 优先/legacy 合并回退）+ 后写校验 +
> saveChapter 原子落盘全链路。`WriterChat` trait 注入（复用 AuditorChat 模式）。

## 背景

29 号（writer 三件套 prompt）、30 号（settler/observer prompt）、31 号
（post-write-validator）就绪后，writer 编排的最后缺口是四个前置模块：
`renderHookSnapshot`（story-markdown.ts 有 Rust 无）、`governed-working-set`
（全缺）、`atomic-file-set`（全缺）、`buildEnglishVarianceBrief` 编排层
（Rust 仅底层原语）。本次连缺口带本体一次收官。

## 改动

### 新建 `engine-rs/src/agents/writer.rs`（~1900 行，含测试）

**类型层**
- `WriteChapterInput` / `SettleChapterStateInput` / `WriteChapterOutput` /
  `TokenUsage` / `HookHealthIssue`（Serialize，camelCase 对齐 TS 契约）
- `WriterChat` trait（chat 端口，测试 mock / 生产实现注入）
- `WriterCtx { project_root, builtin_genres_dir, prompt_store, state_store }`
- `WriteChapterError`（Genre / PromptPack / Chat / Engine / Io / AtomicFileSet）

**编排入口（3 个）**
1. `write_chapter` — 13 路真相文件加载 + recent/fingerprint 章节 → genre/rules →
   prompt pack（longform.writer）→ governed/legacy 双路 user prompt → chat(0.7)
   → `parse_creative_output` → memo 软校验 → 结算工作集裁剪 → `settle` →
   runtime-state artifacts → hook 健康 → 后写校验四件套 → TokenUsage 汇总
2. `settle_chapter_state` — 独立结算（复用 `settle` + artifacts）
3. `save_chapter` / `save_new_truth_files` — 原子集落盘（章节 + 真相 +
   state/*.json + superseded 清理）/ 摘要追加去重

**私有辅助（12 个导出纯函数，golden 守门）**
`build_user_prompt` / `build_governed_user_prompt` / `build_chapter_context_block` /
`join_governed_evidence_blocks` / `build_settler_governed_control_block` /
`verify_pre_write_check_aligns_with_memo` / `build_length_requirement_block` /
`render_delta_summary_row` / `normalize_runtime_state_delta_chapter` /
`build_style_fingerprint` / `extract_dialogue_fingerprints` / `find_relevant_summaries` /
`sanitize_filename` + `load_recent_chapters` / `append_chapter_summary`

### 新建 `engine-rs/src/utils/governed_working_set.rs`（~600 行）

`build_governed_hook_working_set`（选中 ID ∪ agenda ∪ 窗口）、
`merge_table_markdown_by_key` / `merge_character_matrix_markdown`（legacy 回写
表格合并，防 LLM 丢行）、`build_governed_character_matrix_working_set`（活跃角色
过滤）+ 内部 `parse_single_table` / `parse_sections` / `collect_hook_agenda_ids` 等。

### 新建 `engine-rs/src/utils/atomic_file_set.rs`（~350 行）

`commit_atomic_file_set` — staged → backup → 就位三段式事务 + 失败回滚
（逆序删除已就位 → 逆序恢复 backup → 回滚错误汇总）+ 路径逃逸/重复/写删冲突
校验。TS 的 `renameFile` 测试缝隙参数不保留（文档注明）。

### 修改

- `utils/story_markdown.rs`：+`render_hook_snapshot`（无标题、保序、空表
  `- none`，区别于 projections 全量投影）+ 5 个私有 cell 渲染器
- `utils/long_span_fatigue.rs`：+`build_english_variance_brief` 编排层
  （读 chapters/ + 摘要表 → 高频三词短语/首尾句式/cadence 场景义务）；
  `analyze_long_span_fatigue` 留待 pipeline 域
- `state/reducer.rs`：`RuntimeStateSnapshot` 补 derive Serialize（四字段已各自
  实现，wrapper 缺）
- `agents/mod.rs` / `utils/mod.rs`：挂载 + 文档头
- `Cargo.toml`：tempfile 从 dev-dependencies 升为正式依赖（atomic-file-set 生产用）
- golden 双侧：TS dump +11 imports +16 域；Rust `tests/golden_leaf.rs`
  +16 struct 字段 +16 消费测试

## parity 要点（移植难点）

1. **对话正则引号类**：TS `dialogueRegex` 引号全部为**半角直引号 + 「」**
   （码点级核对），不含弯引号 U+201C/D——首版臆加 “ 导致 golden 差分失败
   （Rust 有产出 / TS 空），码点审计后修正
2. **贪婪说话人怪癖**：`(.{1,6})` + 动词备选含单字"道"→ JS 引擎回溯后捕获
   "林动冷声"（非"林动"），Rust regex 同语义，golden 向量固化该行为
3. **CJK 名字提取无 lookahead**：TS `/[\u4e00-\u9fff]{2,4}(?=[，、。：]|$)/g`
   （Rust regex 不支持 lookahead）→ 手动扫描：贪心 4→2 长度前缀、后随分隔标点
   或串尾、非重叠推进
4. **JS truthiness 门控**：`buildStyleFingerprint` 的 `if (profile.x)` →
   Rust `as_f64()` + `!= 0 && !is_nan`；块标题恒中文（ledgerBlock/summariesBlock
   等 buildUserPrompt 未分支语言）
5. **UTF-16 语义**：`sanitize_filename` slice(0,50)、对话 bigram slice(i,i+2)、
   line.length > 1 → `encode_utf16` 窗口；`Number.parseInt` 前缀解析 →
   `parse_int_prefix`（"12ab"→12，"ab12"→None）
6. **Map 插入序**：JS Map 迭代序（说话人指纹/高频 bigram 平局序）→ Vec +
   index HashMap 保序 + `sort_by` 稳定排序
7. **chapterSummary 行正则**：`\|\s*(\d+)\s*\|` 首匹配；`num < chapterNumber - 1`
   用 i64 防下溢
8. **observer 用量丢弃**：TS settle 只返回 settler 的 usage——creative+settler
   求和（集成测试断言 300 而非 450 固化）
9. **`^### ` 行首降级**：`(?m)` 多行标志 + `replace_all`
10. **normalize_delta 相等副本**：TS 对象同一性判断 → Rust `PartialEq` 等值
    （changed=false 时返回相等克隆）

## 验证

- **lib 单测**：724 passed / 0 failed（+46 新单测：writer 36 含 3 个 MockChat
  全链路集成测试——delta 结算落盘 / legacy 表格合并回退 / 独立结算；工作集 13、
  原子集 8、variance brief 15）
- **golden 差分**：55 域全绿（+16 新域）。关键：`build_user_prompt` zh/en 全块
  字节级零分歧（含预算裁剪 + 占位门控 + 双模板）；`extract_dialogue_fingerprints`
  的贪婪说话人怪癖逐字节一致；`normalize_runtime_state_delta_chapter` 整体
  JSON 形状一致
- **TS 全量**：185 文件 / 1798 测试全绿（leaf dump 重生成）
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **33 号**：writer 编排已就绪但未接 HTTP 端点——按《HTTP端点登记与迁移排期》
  挂 `writeChapter` / `settleChapterState` 路由（或先做 planner.ts 编排，
  writer 的 governed 入参来自 planner 产物）。
