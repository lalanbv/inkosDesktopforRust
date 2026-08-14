# 31 号｜Phase 3 agents 域：post-write-validator（后写规则校验链）

> 里程碑：writer 编排的零 LLM 成本后写校验链——每章生成后跑确定性规则，捕获
> prompt-only 规则无法保证的违规。927 行 TS 全量移植，7 导出函数 + ~15 内部辅助。

## 背景

writer.ts 编排 Phase 3（后写校验）依赖 `validatePostWrite` / `detectCrossChapterRepetition` /
`detectParagraphLengthDrift` / `normalizePostWriteSurface` 四件套（writer.ts 直接 import），
外加 `detectDuplicateTitle` / `resolveDuplicateTitle` 标题治理系列。本次一次性补齐，
为 32 号 writer.ts 编排本体清空最后一项纯逻辑依赖。

## 改动

### 新建 `engine-rs/src/agents/post_write_validator.rs`（~870 行）

**类型层**
- `PostWriteViolation { rule, severity: ViolationSeverity, description, suggestion }`（derive Serialize，
  golden 差分序列化为 TS 同形 JSON）
- `ViolationSeverity { Error, Warning }`（`#[serde(rename_all = "lowercase")]` → "error"/"warning"）
- `ResolveDuplicateTitleResult { title, issues }`（derive Serialize）

**7 导出函数**（逐字移植 TS）
1. `normalize_post_write_surface(content, language)` — 剥 meta 备注行 + 破折号转逗号（非 en）+ trimEnd
2. `validate_post_write(content, genre_profile, book_rules, language)` — zh 12 项 + en 分支
3. `detect_cross_chapter_repetition(current, recent, language)` — zh 6-gram / en 3-word
4. `detect_paragraph_length_drift(current, recent, language)` — 段落密度漂移
5. `detect_paragraph_shape_warnings(content, language)` — 段落过碎/连续短段（导出 + 内部用）
6. `detect_duplicate_title(new_title, existing)` — exact + near-dup（stripPunct）
7. `resolve_duplicate_title(new_title, existing, language, content)` — 重生成 + counter fallback + collapse 治理

**内部辅助**：`detect_narrative_person_drift` / `detect_first_person_inner_state_slip` /
`validate_post_write_english` / `analyze_paragraph_shape` / `extract_paragraphs` /
`is_dialogue_paragraph` / `detect_title_collapse` / `regenerate_*` / `extract_*_qualifier` /
`extract_*_terms` / `capitalize` / `strip_punct` / `count_substring`

### 修改
- `engine-rs/src/agents/mod.rs`：挂载 `post_write_validator` + 文档头登记
- `packages/core/src/__tests__/golden-leaf-dump.test.ts`：+6 imports +6 section（normalize 3 /
  validate 4 / cross-chapter 2 / paragraph-drift 1 / duplicate-title 2 / resolve-duplicate 2）+6 断言
- `engine-rs/tests/golden_leaf.rs`：+6 struct 字段 + 6 消费测试（violations 数组整体 serde 比对）

## parity 要点（移植难点）

1. **UTF-16 长度对齐**：所有 TS `string.length` → `utf16_len`（markerLimit/3000、段落 >300/>500、
   shortThreshold 35/120、content.length >= 800、slice(0,39) 截断等）
2. **`split(needle).length - 1` → `matches(needle).count()`**：消除 JS split 首尾空串差异
   （woCount/nameCount/fatigue 计数）
3. **lookbehind 替代**：TS `split(/(?<=[。！？!?])|\n+/)`（Rust regex 不支持 lookbehind）→
   手动扫描 `split_sentences_for_inner_state`（句末标点后切分，标点归前段；换行处切分）
4. **`\w` ASCII-only**：JS `\w` = `[A-Za-z0-9_]`，Rust regex 默认 Unicode → 显式 `[A-Za-z0-9_]`
   （`en_non_word_re`）
5. **Unicode 属性**：`[\u4e00-\u9fff]` → `[\u{4e00}-\u{9fff}]`；`[^\p{L}\p{N}]` 原生支持
6. **regex escape**：TS `word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")` → `regex_escape`（疲劳词动态正则）
7. **paragraph shape 对话段过滤**：`is_dialogue_paragraph` 判定 `"` `"` `"` `「` `『` `'` `《` 或 `——` 开头
8. **标题 collapse**：依赖 `analyze_chapter_cadence` 的 `title_pressure`（high + repeatedToken 命中）
9. **临时值生命周期**：`replace_all(&content.to_lowercase(), ...)` 临时 String drop → 先 `let lower` 绑定

## 验证

- **lib 单测**：678 passed / 0 failed（+23 新单测覆盖 normalize/validate zh-en/cross-chapter/
  paragraph-drift/duplicate-title/resolve 全路径）
- **golden 差分**：39 域全绿（+6 新域）。关键：`validate_post_write` 的 **整个 violations 数组**
  （含 push 顺序、severity、中英文案、阈值数字）与 TS 字节级零分歧——这是最硬的 parity 证明
  （12 项中文规则 + 5 项英文规则 + 人称漂移两条路径全验证）
- **clippy**：`cargo clippy --lib --tests` 零警告（修 3 处 `manual_contains`）

## 下一步

- ⬜ **32 号**：writer.ts 编排本体（1503 行）——三件套 prompt（29 号）+ 四件套解析（既有）+
  settler/observer prompt（30 号）+ 后写校验（31 号）全部就绪，writer 编排可落地。
  `WriterChat` trait 注入（复用 AuditorChat 模式），Phase 1 创作 + Phase 2 结算 + 后写校验总装。
