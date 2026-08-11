# 07 — Phase 2 utils 域：hook-arbiter（hook 仲裁器）+ story-markdown 叶子

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`06_Phase2_state域_memory-db持久化层.md`
> 性质：runtime-state-store 的纯逻辑前置依赖（hook 操作裁决）

## 移植内容

### 1. `utils/story_markdown.rs`（部分移植）
移植自 `packages/core/src/utils/story-markdown.ts`（346 行）的叶子函数 [`normalize_hook_id`]——
它是 hook-arbiter 的唯一外部依赖。story-markdown 的其余部分（hook 表格行解析、章节号解析、
depends_on 解析等）属 state-bootstrap 依赖域，后续阶段补齐。

- **迭代剥离 7 种 markdown 包装**（`[text](url)` / `**` / `__` / `*` / `_` / `` ` `` / `~~`），循环直到稳定
- dash 归一（`-{2,}`→`-`）+ 去首尾 dash
- 最终判定：含 `[a-zA-Z0-9一-鿿]`（字母/数字/CJK）则保留，否则返回空
- 8 个单测（含嵌套包装 `[**x**](url)` 两轮剥离）

### 2. `utils/hook_arbiter.rs`（~480 行）
移植自 `packages/core/src/utils/hook-arbiter.ts`（332 行）。[`arbitrate_runtime_state_delta_hooks`]
把 architect/consolidator 的原始 delta 裁决为可直接归约的 resolved delta：

- 已知 id 的 upsert 直接保留；未知 id 转 fallback 候选（带偏好 id）
- fallback + newHookCandidates 经 `evaluate_hook_admission` 准入：
  - **准入** → 建规范 hook（canonical id，冲突加 `-N` 后缀）
  - **DuplicateFamily** → 有新颖内容则**映射**回既有 hook（merge + mention 删除）；纯重述降级为 mention
  - **缺 type/payoff** → rejected
- 最终 mention/upsert/resolve/defer 互斥过滤（upsert > mention > resolve/defer）

完整移植辅助函数：`slugify_hook_stem` / `extract_terms` / `extract_chinese_bigrams` /
`is_pure_restatement` / `prefer_richer_text` / `normalize_text` / `merge_candidate_into_existing_hook` /
`create_canonical_hook` / `build_canonical_hook_id` / `sort_hooks` / `unique_strings`。

## 关键技术点

- **强类型适配（与 TS 的核心差异）**：
  - `status` 为 `HookStatus` 枚举（非字符串）：`!= "resolved"` → `!= HookStatus::Resolved`；`"progressing"`/`"open"` → 枚举变体
  - `payoff_timing` 为 `Option<HookPayoffTiming>` 枚举；调用 `resolve_hook_payoff_timing`（接收 `Option<&str>`）时，用 `payoff_timing_as_str` 把枚举经 serde 名映射回字符串——与 TS 字符串路径**完全等价**，避免枚举↔字符串的有损往返分歧
  - `RuntimeStateDeltaSchema.parse(...)`（TS 运行时校验）→ Rust 编译期强类型，无需 parse，直接 clone 重组
- **HookAdmissionReason 枚举→TS 字符串名**：决策 `reason` 字段沿用 TS 串（`"duplicate_family_with_novelty"` 等），便于前端/日志观测不变；用 `admission_reason_str` 映射
- **正则 OnceLock 编译一次**：5 个正则（non_keepable/english_term/chinese_term/chinese_run/trailing_dash）经 `OnceLock<Regex>` 缓存，零运行时重编译；CJK 范围用 `\x{4e00}-\x{9fff}`（TS 的 `一-鿿`）
- **中文 bigram 纯重述判定**：`is_pure_restatement` 的 `novel_terms == 0 && novel_chinese < 2` 阈值逐字对齐 TS——这是 load-bearing 的去重语义

## 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| payoff_timing 传递 | 枚举→serde 名字符串→resolve | 与 TS 字符串路径等价，避免有损往返；复用已验证的 `resolve_hook_payoff_timing` |
| STOP_WORDS | `&[&str]` const + `stop_words_contains` | 编译期常量，零分配；封装 contains 避免 `&&str`/`&str` 歧义 |
| 私有辅助函数测试 | 同模块 `#[cfg(test)]` 直接访问 | `build_canonical_hook_id` 后缀逻辑用单元测试精确覆盖，比集成造冲突场景更干净 |

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **16 项**（story_markdown 8 + hook_arbiter 8）全绿 |
| TS 行为对齐 | hook-arbiter.test.ts 的 3 个核心场景（map/mention/create）在 Rust 端等价复现 ✓ |
| 全量 lib 测试 | **324 passed**（上轮 308 + 本次 16） |
| golden 差分测试 | **22 passed**（未触及） |
| clippy 双模式 | 零警告（default + `--features export-bindings`，三新模块无新增警告） |

## state 域 + utils 域现状

**utils 域 hook 级联完整度**：policy → lifecycle → governance → **arbiter**（本次）→ stale_detection → promotion。
hook-arbiter 是 hook 操作进入 reducer 前的最后一道裁决，至此 utils 的 hook 子链**全链路 Rust 化**。

**runtime-state-store 解锁进度**：
- ✅ memory_db（持久化）
- ✅ reducer（归约）
- ✅ validator（校验）
- ✅ hook_arbiter（hook 裁决，本次）
- ⬜ state-bootstrap（markdown→结构化状态引导，645 行，仍待移植）

runtime-state-store 仍依赖 state-bootstrap（`bootstrapStructuredStateFromMarkdown` + `parseCurrentStateFacts`），
故其完整移植需先完成 state-bootstrap。state-bootstrap 是 markdown 解析大模块，列为下一独立子目标。
