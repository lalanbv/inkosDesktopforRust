# 33 号｜Phase 3 agents 域：planner 数据供给层（memory-retrieval + planning-materials）

> 里程碑：planner 链的前置数据层全量移植。`retrieveMemorySelection`（结构化
> state/*.json 优先 → markdown 兜底 → SQLite 加速）+ `loadPlanningSeedMaterials` /
> `gatherPlanningMaterials`（种子材料装配）+ `renderSummarySnapshot`。为 34 号
> planner 编排本体（planner-context / planner-prompts / planner.ts）铺平数据入口。

## 背景

32 号（writer 编排本体）就绪后，write-next 流程的上游缺口是 planner：writer 的
governed 入参（ChapterIntent / ChapterMemo / recyclableHooks）来自 planner 产物。
planner.ts 依赖五个未移植模块，其中数据层两个（memory-retrieval 530 行 /
planning-materials 185 行）是纯装配 + 检索逻辑，无 LLM 交互，先行独立收官。

## 改动

### 新建 `engine-rs/src/utils/memory_retrieval.rs`（~1100 行，含测试）

**检索入口**
- `MemorySelection` / `VolumeSummarySelection` / `RetrieveMemoryParams`
- `retrieve_memory_selection`：bootstrap 结构化状态 → 六路并行读
  （current_state 回退链 / pending_hooks.md / volume_summaries.md /
  state/{current_state,hooks,chapter_summaries}.json）→ 事实回退解析 →
  检索词双轨（narrative 不含 mustKeep / fact 含）→ hook 走权威路径 →
  SQLite 分支（空库回填摘要+事实、DB 行回退活跃集、dbPath 标注）

**纯函数（golden 守门）**
- `compute_recyclable_hooks`：回收阈值链（pressured/near_payoff 系 ≥5 /
  coreHook ≥8 / 其余 ≥10）+ 终态排除 + 未来计划排除 + 沉默 DESC/起点 ASC 排序
- `extract_query_terms`：goal+mustKeep 为主（≥2 词封顶）、不足并入 outlineNode、
  上限 12 词

**私有选择器（Rust 单测镜像 TS 行为）**
`select_relevant_summaries`（近 3 章保底 + 得分排序 + 截 4 还原 ASC）、
`select_relevant_hooks`（主选 ≤6 + 陈旧补位 ≤2）、`select_relevant_facts`
（优先谓词基分 + 截 4）、`select_relevant_volume_summaries`（末位保底 + 截 2）、
`parse_volume_summaries_markdown` + `slugify_anchor`、中文焦点词提取
（前缀重复组剥离 + 2..4 字尾部窗口）、`strip_negative_guidance`。

TS 死代码 `buildLegacyQueryTerms`（定义未引用）不移植，此处备案。

### 新建 `engine-rs/src/utils/planning_materials.rs`（~260 行，含测试）

- `PlanningSeedMaterials` / `PlanningMaterials`
- `read_previous_ending_excerpt`：chapters/ 下 `NNNN*.md` 首匹配 → 去首行 →
  尾部 320 UTF-16 单元窗口（`utf16_tail`，代理对不截半）
- `load_planning_seed_materials`：九路并行读（author_intent / current_focus /
  story_frame / volume_map / 摘要 / book_rules / current_state 回退链 /
  前章末屏 / brief），摘要 DESC 截 4 还原 ASC，`previousEndingHook` 空串门控
- `gather_planning_materials`：种子 + 记忆选集 + planner_inputs 清单
  （dbPath 条件追加）

### 类型层改造（status_raw parity 基建）

- `models/runtime_state.rs`：`HookRecord` 新增 `status_raw: String`
  （`#[serde(default, skip_serializing_if = "String::is_empty")]`，序列化形状
  对规范记录不变）；**手写 `Deserialize`**——status 原文归一化进枚举的同时保留
  原文（显式 statusRaw 优先 / 规范名留空 / 非规范原文保留）
- `HookStatus` 枚举补 `#[serde(other)]`（Open 兜底，供枚举直用场景）
- `utils/hook_lifecycle.rs`：+`hook_status_text`（原文优先、空串回退规范名）、
  +`hook_status_canonical` / `hook_payoff_timing_canonical`
- `state/memory_db.rs`：`StoredSummary` 补 Serialize/Deserialize（golden 输入）
- 13 处 `HookRecord` 构造点补 `status_raw`（reducer/arbiter 走规范枚举 → 空串）

### 渲染补齐

- `utils/story_markdown.rs`：+`render_summary_snapshot`（8 列 zh/en 双语表头 +
  `escapeTableCell`）；`render_hook_snapshot` 状态列改用 `hook_status_text`
  （原文透传，对齐 TS `hook.status` 直读）

### 顺手修复（32 号遗留）

`--features export-bindings` 编译破损：`RuntimeStateDelta` /
`RuntimeStateSnapshot` / `PostWriteViolation` / `ViolationSeverity` 补 TS derive，
ts-rs 启用 `serde-json-impl`（`subplot_ops` 等自由 JSON 列 → `any`）。
bindings 再生成恢复可用（HookRecord.ts 含 statusRaw）。

### golden 双侧

- TS dump（`golden-leaf-dump.test.ts`）：+3 域 12 例——
  `compute_recyclable_hooks`（期望投影为 hookId 列表，规避两侧 HookRecord
  序列化形状差异）、`extract_query_terms`、`render_summary_snapshot`（zh/en/空）
- Rust `tests/golden_leaf.rs`：+3 域消费测试（58 域全绿）

## parity 要点（移植难点）

1. **原始状态串语义**：TS `StoredHook.status` 是 string，`recycleThreshold`
   （"pressured"/"near_payoff" → 阈值 5）与 `isRecycleTerminalStatus`
   （"已解决"/"paused"/"hold" 等）直接吃原文；Rust 4 值枚举会丢信息 →
   `status_raw` + `hook_status_text` 判定链还原。golden `threshold-matrix`
   例固化（H04 core 8 章 / H02 near_payoff 7 章 / H01 pressured 5 章）
2. **前缀剥离正则的 `^` 锚**：TS `/^(本章|继续|...)+/` 有锚，首版漏锚导致
   `replace_all` 把串中「推进」一并剥掉（golden `chinese-focus-suffixes`
   差分抓出：期望 ["推进","起推进","崛起推进"] vs 实得 ["林动崛起",...]）
3. **JS `\b` 是 ASCII 词边界**（\w 不含 CJK），Rust 默认 `\b` 是 Unicode 词边界
   ——CJK 紧邻 ASCII 词首处行为不同 → `(?-u:\b)` 显式关 unicode
4. **`slice(-n)` UTF-16 语义**：前章末屏截尾 320 单元、中文焦点词尾部窗口，
   均 `encode_utf16` 窗口实现（CJK BMP 与 chars 等价，代理对不截半）
5. **zod 严格枚举 vs Rust 宽容超集**：TS `HookStatusSchema` 收到 "pressured"
   会让整个 hooks.json 解析失败回退 markdown；Rust 手写 Deserialize 归一化 +
   原文保留继续走结构化路径（更稳，行为超集在变更记录备案）
6. **`promoted=否` 不算活跃**：`parseBooleanCell("否")` → false → TS/Rust 均将
   其视作架构师种子排除出活跃集（集成测试数据踩坑后固化）
7. **`extractChineseFocusTerms` 前缀词全剥后回退原段**：如「保持」是前缀组词，
   剥空后 `target = segment` 保留（TS `stripped.length >= 2 ? stripped : segment`）
8. **`filter` 回调的 index 语义**：卷摘要 `entry.matched || index === all.length - 1`
   中 index 是映射后全数组的下标（末位恒保留）——Rust enumerate 于 ranked 再过滤
9. **稳定排序**：JS `Array.prototype.sort`（ES2019+ 稳定）→ `sort_by` 稳定排序，
   平局保原始序
10. **Set 插入序**：焦点词 Set → Vec + HashSet 保序去重

## 验证

- **lib 单测**：740 passed / 0 failed（+16 新单测：回收阈值矩阵/原始状态变体/
  排序、检索词中英/停用词/回退、摘要-钩子-事实-卷摘要选择器、卷摘要解析+slug、
  markdown 路径集成测试（tempdir 真实落盘+SQLite 分支））
- **golden 差分**：58 域全绿（+3 新域）。差分实战抓出 2 个真实 bug（前缀正则
  漏锚、serde 丢原文）后修正——golden 机制有效性再次验证
- **export-bindings**：898 passed（补 TS derive 后恢复编译，bindings 再生成）
- **TS 全量**：185 文件 / 1798 测试全绿（leaf dump 重生成）
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **34 号**：planner 编排本体——planner-context.ts（297 行，上下文提取器
  纯函数群）+ planner-prompts.ts（404 行，系统提示词 zh/en + 用户模板 +
  黄金三章指引）+ planner.ts（879 行，`PlannerChat` trait + planChapter 编排 +
  memo 重试链 + fallback memo + intent markdown 渲染落盘）。数据供给层已就绪。
