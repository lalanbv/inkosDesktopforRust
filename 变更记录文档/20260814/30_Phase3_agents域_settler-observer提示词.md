# 30 号｜Phase 3 agents 域：settler / observer 提示词

> 里程碑：writer 编排 Phase 2（Observer 事实提取 + Settler 状态回写）的 prompt 构造层。
> 纯函数，与既有 [`settler_parser`] / [`settler_delta_parser`] 输出端配对，
> 构成 writer 编排「prompt 构造 → LLM chat → 解析」三段式的两端骨架。

## 背景

writer.ts 编排依赖四件套 prompt 构造器（settler-prompts 230 行 + observer-prompts 127 行
+ settler-parser 38 行 + settler-delta-parser 53 行）。后两者（解析器）已于早期里程碑移植，
本次补齐前两者（prompt 构造），为 32 号 writer.ts 编排本体清空 Phase 2 依赖缺口。

## 改动

### 新建 `engine-rs/src/agents/settler_prompts.rs`（~330 行）

- `build_settler_system_prompt(book, genre_profile, book_rules: Option, language: Option<WritingLanguage>) -> String`
  - 语言合并对齐 TS `language ?? genreProfile.language`（显式 `Some` 优先，`None` 才回退 genre 画像语言）
  - 数值块：`numerical_system` 条件（两份中文原文，逐字）
  - hookRules 段：固定中文原文（TS 源即不分语言，en 仅前置 LANGUAGE OVERRIDE 头）
  - 全员追踪块：`book_rules.enable_full_cast_tracking` 条件
  - 模板插值：`book.title` / `genre_profile.name` / `book.genre` / `book.platform`（`Platform::as_str()`）
  - `build_settler_output_format(gp)`：`chapter_types[0]` 缺省回退 `"主线推进"`
- `build_settler_user_prompt(&SettlerUserPromptInput) -> String`
  - 条件块语义：ledger / observations / evidence / feedback 为**非空字符串**才输出（TS truthy）
  - 四类 truth 文件以 `!== "(文件尚未创建)"` 判定
  - `governed_control_block` 存在时**卷纲块让位**（TS `controlBlock.length === 0` 互斥）
- 参数 struct 化：`SettlerUserPromptInput<'a>`（14 字段，`&str` 切片，0GC）

### 新建 `engine-rs/src/agents/observer_prompts.rs`（~210 行）

- `build_observer_system_prompt(_book, genre_profile, language: Option) -> String`
  - **parity 钉死**：TS 签名含 `book` 参数但函数体未使用——Rust 保留为 `_book` 对齐调用面
  - en/zh 两完整分支模板逐字搬运（en 首句句号仍是中文「。」——TS 原样保留）
- `build_observer_user_prompt(chapter, title, content, language: Option) -> String`
  - **parity 钉死**：TS `language === "en"` 判定**无 genre 回退**（`undefined` → zh），
    与 system prompt 的 `language ?? genreProfile.language` 不同——Rust `language == Some(En)`

### 修改

- `engine-rs/src/agents/mod.rs`：挂载 `observer_prompts` / `settler_prompts` + 文档头登记
- `packages/core/src/__tests__/golden-leaf-dump.test.ts`：
  - +3 imports（settler/observer 函数 + 3 type import）
  - +fixture（`settlerBook` / `settlerGp` 工厂 / `fullCastRules`，`as unknown as BookRules` 收敛噪音）
  - +4 section（settler_system ×5 向量 / settler_user ×3 / observer_system ×3 / observer_user ×3）+ 4 断言
- `engine-rs/tests/golden_leaf.rs`：+4 struct 字段 + `settler_book`/`settler_gp`/`lang_opt` helper + 4 消费测试

## parity 要点（golden 差分抓出的细节）

1. **引号字符不一致**（TS 源码历史遗留，golden 如实反映）：
   - settler-prompts.ts 用**弯引号** U+201C/U+201D（3 处）→ Rust `\u{201c}`/`\u{201d}`
   - observer-prompts.ts「具体化」行用 **ASCII 直引号** U+0022 → Rust `\"`
   - 首次差分 observer 失败，hex 验证后定位（U+201C 计数 observer=0 / settler=3）
2. **语言判定分歧**：observer_user 的 `language === "en"` 无 genre 回退（None→zh），
   与 observer_system / settler 的 `language ?? genreProfile.language`（None→genre）语义不同——逐分支对齐
3. **条件块 truthy**：`params.ledger ?` 是 TS truthy（非空字符串），Rust `!is_empty()`；
   `!== "(文件尚未创建)"` 精确比对占位文案
4. **governedControlBlock 互斥**：`controlBlock.length === 0` 才输出卷纲块（TS 空串=无控制块）

## 验证

- **lib 单测**：655 passed / 0 failed（+15 新单测：settler 9 + observer 6）
- **golden 差分**：33 域全绿（+4 新域：settler_system 5 向量 / settler_user 3 / observer_system 3 / observer_user 3，字节级零分歧）
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **31 号**：post-write-validator（926 行）——validatePostWrite / detectCrossChapterRepetition /
  detectParagraphLengthDrift / normalizePostWriteSurface + PostWriteViolation 类型，writer 编排的零 LLM 成本后写校验链
- ⬜ **32 号**：writer.ts 编排本体（1503 行）——Phase 1 创作 + Phase 2 结算 + 后写校验总装，
  WriterChat trait 注入（复用 AuditorChat 模式），三件套 prompt + 四件套解析 + 后写校验全部就绪后即可落地
