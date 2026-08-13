# 26 — Phase 3 agents 域：rules-reader（规则读取链）

> 日期：2026-08-13
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`25_Phase3_agents域_writer-parser.md`

## 移植内容

25 号收尾指出 ContinuityAuditor 的阻塞项是 rules-reader / 规则解析层未移植。本里程碑
补齐该读取链（ContinuityAuditor 的规则面数据入口）：

### `models/genre_profile.rs` — [`parse_genre_profile`]
移植 `genre-profile.ts`（36 行）的解析函数（原 TODO(serde_yaml)）：
- frontmatter 正则逐字移植（`^---\s*\n([\s\S]*?)\n---\s*\n([\s\S]*)$`）
- YAML → zod 语义校验（`GenreProfileRaw` 中间层：`name`/`id`/`chapterTypes`/
  `fatigueWords` 必填，缺失即错；`language` 枚举校验 zh/en，缺省 zh）
- 错误分类：`MissingFrontmatter` / `Yaml` / `InvalidField`

### `models/book_rules.rs` — [`parse_book_rules`] + [`try_parse_book_rules_frontmatter`]
移植 `book-rules.ts`（313 行）剩余全部解析逻辑（原 TODO(serde_yaml)）：
- `try_parse_book_rules_frontmatter`：严格变体，`NoFrontmatter` 与 `Invalid` 分类
  （对齐 TS 两类 null，供 rules-reader 决定回退路径）
- `parse_book_rules`：frontmatter 优先（失败静默落回退）→ shim 检测 → markdown 回退
- `parse_markdown_book_rules` + 8 个助手逐字移植：`extract_markdown_section`（行级
  状态机）、`read_labeled_value/list`（`(?im)` 多行大小写不敏感标签匹配）、
  `read_markdown_list`、`split_list`、`detect_narrative_person`、`normalize_fanfic_mode`、
  `clean_scalar`、`normalize_heading`
- 代码栅栏剥离（LLM 常裹 ```` ```md ```` ）

### `agents/rules_reader.rs`（新文件）
移植 `rules-reader.ts`（143 行）：
- `read_genre_profile`：项目级 → 内置 → other.md 三级查找（`try_read_file` 吞一切读错）
- `list_available_genres`：内置先入表 + 项目级同 id 覆盖 + 按 id 排序
- `read_book_rules`：book_rules.md → story_frame.md frontmatter（legacy 回退）→ None；
  两个 `console.warn` → `tracing::warn!`
- `read_book_language`：book.json 的 language（pick 语义——其余字段坏不影响）

## 关键技术点

- **serde_yaml 基座**：新增依赖 `serde_yaml = "0.9"`（TS js-yaml 对应物）。0.9 已停止
  维护但仍是事实标准；解析面仅受控 frontmatter，风险可控。
- **zod→serde 保真策略**：`BookRulesRaw` 中间层复刻 zod 语义——
  - `narrativePerson` 用 `Option<serde_yaml::Value>` 承载再手工归一，复刻
    `.optional().catch(undefined)`（非法值静默降级，不 fail 整个解析）
  - `fanficMode` enum 无 catch：非法值 → Err → frontmatter 失败 → markdown 回退（对齐
    zod throw 被 parseBookRules 的 catch 吞掉）
  - 嵌套对象（Protagonist.name / GenreLock.primary）必填语义：容器级不设 default
- **空 frontmatter 的 null 语义**（golden 差分逮到的真实分歧）：TS `yaml.load("") →
  undefined → zod throw → markdown 回退`；serde_yaml 会把空输入吞成全默认结构。显式
  先解析为 `serde_yaml::Value`，`is_null()` 即 Err，交给调用方回退。
- **markdown 回退的 zod default**：`BookRules::default()` 的 derive default 是空串，
  zod default 是 `"1.0"`——显式 `version: default_version()` 对齐。
- **JS `\b` vs Rust `\b`**：`detect_narrative_person` 的 `\bfirst\b` 用 `(?-u:\b)`
  （ASCII 词边界）对齐 JS `\w` 仅 ASCII 的语义（中文邻接处 Rust Unicode `\b` 行为不同）。
- **内置 genres 目录注入**：TS 经 `import.meta.url` 定死 `packages/core/genres`；Rust
  作参数 `builtin_genres_dir` 注入，由调用方（src-tauri / axum）决定路径（纯内核范式）。
- **listAvailableGenres 容错语义逐字保留**：单阶段任一文件解析失败中断该阶段剩余文件
  （TS 的 try/catch 包住整个循环），已入表条目保留。
- **已知接受偏差**：serde `Option` 把 YAML 显式 `null` 当缺失，zod 会 throw；受控
  frontmatter 无此写法。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **32 项**（genre 解析 7 + book-rules 解析 15 + rules-reader 10）全绿 |
| 全量 lib 测试 | **561 passed**（上轮 529 + 本次 32） |
| golden 差分测试 | **24 passed**（上轮 22 + 本次 2 域：parse_genre_profile 10 向量、parse_book_rules 12 向量） |
| clippy 双模式 | 零警告 |

golden 差分新工具：`norm_numbers`（TS JSON 整数 vs Rust f64 序列化的数字归一，
auditDimensions/hardCap 向量必需）。

## 解锁进度（ContinuityAuditor 路线）

- ✅ rules-reader（本次）——`readGenreProfile` / `readBookRules` / `readBookLanguage`
- ⬜ `utils/governed-context.ts`（101 行，纯逻辑；依赖 `ContextPackage` 类型未移植）
- ⬜ `models/input-governance.ts` 的 `ContextPackage` / `RuleStack` 类型层（29/124 行）
- ⬜ `agents/fanfic-dimensions.ts`（87 行，纯逻辑）
- ⬜ `utils/outline-paths.ts`（340 行，fs 读取：readVolumeMap / readCharacterContext /
  readCurrentStateWithFallback）
- ⬜ ContinuityAuditor 主体（continuity.ts 375-842 行：auditChapter 编排 + 维度注记 +
  审查结果解析）

## 会话累计（25 个里程碑）

lib 测试 278 → **561**（+283 测试，0 回归），24 golden 全绿，clippy 双模式零警告。
