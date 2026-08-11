# 21 — Phase 3 prompts 域：short-fiction（prompts 域完整 Rust 化）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`20_Phase3_prompts域_prompt-pack.md`
> 性质：**prompts 域 100% Rust 化**

## 移植内容

### `prompts/short_fiction.rs`
移植 `short-fiction.ts`（568 行）——13 个 build 函数 + craft helper：

- **system/user prompt 对**：outline（system+user）/ outline-review / writer / draft-review / package
- **followup**：outline-revision / draft-revision
- **continuation**：draft-continuation（只补缺失章节）
- **craft helper**：内嵌写法提醒段（中英）
- 9 个输入类型（OutlinePromptInput / OutlineReviewPromptInput / DraftPromptInput / DraftContinuation / DraftReview / DraftRevision / OutlineRevision / Package / Reference）

## 关键技术点

- **机械模板搬运的忠实性**：568 行中英 prompt 字符串逐字移植，无逻辑变化。`WritingLanguage` 枚举替代
  TS 的 `ShortFictionLanguage = "zh" | "en"`（复用 crate 已有枚举，语义等价）。
- **`.filter(Boolean).join("\n")` 等价**：TS 用过滤空段后 join；Rust [`join_nonempty`] 收 `Vec<Option<String>>`
  过滤 None + 空串。reference 块用 `Option` 表达「可选」语义。
- **章节块循环**：TS `Array.from({length: n}, (_, i) => ...)` → Rust `(1..=n).map(chapter_blocks).join("\n")`。
  continuation 的 `missing_chapters.map` 同理。
- **内嵌 craft 段**：writer/continuation 内嵌 `build_craft_prompt`（写法提醒），对齐 TS 内联调用。
- **trim 对齐**：review/outline/draft 的 trim 处理（`input.review.trim()` 等）逐一对齐。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **10 项**（system/user/review/revision/continuation/package 各语言 + reference 块有/无 + 章节块计数 + craft）全绿 |
| 全量 lib 测试 | **481 passed**（上轮 471 + 本次 10） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## prompts 域完整 Rust 化（里程碑）

- ✅ builtin 12 prompt + 3 pack + 查询函数
- ✅ prompt_pack 覆盖加载（project/user/builtin 三级）
- ✅ **short_fiction 全部 13 build 函数**（本次）

**prompts 域 100% Rust 化达成**。该域从 builtin prompt 数据 → 覆盖加载 → 短篇 prompt 构造全链路打通。

## 会话累计（17 个里程碑）

本会话（06–21 号）累计：
- state 域全量 Rust 化（I/O 编排主链贯通）
- utils 域 hook 子链全链路 + story-markdown 解析全函数
- Phase 3 agents 域 6 个叶子 + **prompts 域 100%**（prompt-pack + short-fiction）

lib 测试 278 → **481**（+203 测试，0 回归），22 golden 全绿，clippy 双模式零警告。
