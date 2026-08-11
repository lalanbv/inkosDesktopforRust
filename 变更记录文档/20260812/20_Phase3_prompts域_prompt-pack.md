# 20 — Phase 3 prompts 域：prompt-pack（项目/用户/builtin 三级覆盖）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`19_Phase3_agents域_detector.md`

## 移植内容

### `prompts/prompt_pack.rs`
移植 `prompt-pack.ts`（110 行）——prompt pack 三级覆盖加载：

- [`load_prompt_pack_prompt`]：project 覆盖 → user 覆盖 → builtin 三级优先级
- [`append_prompt_pack_guidance`]：在 basePrompt 后追加 `## Prompt Pack Guidance (...)` 段（空覆盖原样返回）
- [`prompt_override_path`]：`{root}/prompt/{seg}/{last}.md`（`longform.writer` → `prompt/longform/writer.md`）
- `LoadedPromptPackPrompt` / `LoadPromptPackPromptInput` / `PromptPackPromptNotFoundError`

## 关键技术点

- **fs 经 StateStore trait 注入**：`load_prompt_pack_prompt(store: &dyn StateStore, ...)`——
  复用 state 域 async 编排的同一 fs 基座（StateStore），是 prompts 域首个 async 编排。
  `InMemoryStateStore` 单测三级优先级与覆盖语义。
- **promptOverridePath 的路径拼合**：TS `join(root, "prompt", ...parts.slice(0,-1), `${last}.md`)`。
  Rust 用 `join_path` 逐段拼合（`split('.')` 取 `dirs + last`）。无分隔符 id 退化为整体文件名。
- **normalize_prompt_id**：trim + lower（对齐 TS），三级查找前规范化。
- **append 的空覆盖语义**：覆盖文件内容 trim 后空 → 原样返回 base（对齐 TS `if (!content) return basePrompt`）。
- **PromptSource 复用**：mod.rs 的 PromptSource（4 值，含 external）；prompt-pack 用其中 project/user/builtin 三值。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **9 项**（override path 规范化/normalize/三级优先级/project>user>builtin/未找到/append 拼接/空覆盖）全绿 |
| 全量 lib 测试 | **471 passed**（上轮 462 + 本次 9） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## prompts 域进度

- ✅ builtin 12 prompt + 3 pack + 查询函数（此前）
- ✅ **prompt_pack 覆盖加载**（本次）
- ⬜ short-fiction.ts（568 行短篇模板数据，纯数据，按需移植）

prompts 域核心加载链完整。short-fiction 是大量模板字符串数据，可按需补。

## 会话总结（16 个里程碑）

本会话（06–20 号）累计交付：
- **state 域全量 Rust 化**（I/O 编排主链贯通：memory_db / store / state_bootstrap 全量 / runtime_state_store / chapter_workspace）
- **utils 域 hook 子链全链路 + story-markdown 解析全函数**
- **Phase 3 agents 域 6 个叶子**（detection_insights / settler_parser / settler_delta_parser / ai_tells / style_analyzer / detector）
- **prompts 域 prompt_pack 覆盖加载**

lib 测试 278 → **471**（+193 测试，0 回归），22 golden 全绿，clippy 双模式零警告。
变更记录 06–20 归档。剩余仍是多周/月量级（业务运行时 pipeline/edit-controller/runtime、
134 HTTP 端点、37 provider 数据文件、agent 编排群）。
