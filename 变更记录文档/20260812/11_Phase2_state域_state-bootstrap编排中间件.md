# 11 — Phase 2 state 域：state-bootstrap 编排中间件层

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`10_Phase2_state域_StateStore基座与durable-progress.md`
> 性质：state-bootstrap 主入口编排的前置中间件（language/repair/load）

## 移植内容

### `state/state_bootstrap.rs`（编排中间件扩展）
移植 4 个主入口依赖的中间件：

- [`resolve_runtime_language`]：读 `{book_dir}/book.json` 的 `language` 字段，仅 "zh" → Zh，其余 → En
- [`repair_hooks_state_value`]：修复 hooks 的 `serde_json::Value`——空/缺失 `type` → "unspecified"（+warning），非空 type 去空白
- [`load_json_if_valid<T: DeserializeOwned>`]：泛型 JSON 读取+解析；文件缺失 → None（无 warning），解析失败 → None + warning
- [`load_hooks_state_if_valid`]：读取 + repair + 解析 hooks 状态，返回 `(state, repaired)`

## 关键技术点

- **repair_hooks_state_value 的 Value 同构**：TS `repairHooksStateInput` 操作反序列化前的 `unknown`；
  Rust 用 `&mut serde_json::Value` 同构——在 `from_value::<HooksState>` 前修复，保留 TS 的「先修复再校验」语义。
  这样历史 hooks.json 的空 type 数据能通过强类型反序列化。
- **load_json_if_valid 泛型**：`T: serde::de::DeserializeOwned`，一个函数服务 manifest/current_state/
  chapter_summaries 三种类型，对齐 TS `loadJsonIfValid<T>` 的泛型签名。
- **ENOENT 语义**：TS 用 `/ENOENT/.test(message)` 区分文件缺失与解析错误；Rust 用 `read_to_string` 返回
  `None`（store 层已把 NotFound 转为 None）天然区分——更干净，无需正则匹配错误信息。
- **resolve_runtime_language 宽松**：任何错误（缺文件/非 JSON/无 language 字段）→ En，与 TS try/catch 一致。

## ⚠️ 诚实记录：Rust serde vs TS zod 严格性差异

移植中发现一个真实的语义差异：

- **TS zod schema** 对 hooks 的缺失字段（startChapter/status/lastAdvancedChapter 等）往往有 default，宽松接受
- **Rust serde** 的 `HookRecord` 仅 `expected_payoff`/`notes` 标了 `#[serde(default)]`，其余必填字段缺失即反序列化失败

后果：`load_hooks_state_if_valid` 的 repair 只修复 `type`，若历史 hooks.json 还缺其它字段，
Rust 版会返回 None（+warning）而 TS 版可能成功。**测试用完整字段 JSON 验证 repair 逻辑本身**。

**后续主入口移植时需评估**：是否给 `HookRecord` 的 `start_chapter`/`last_advanced_chapter`（default 0）、
`status`（default Open）等加 `#[serde(default)]`，以对齐 TS zod 的宽松度。这是 models 层决策，需确认
不破坏 reducer/projections 的现有行为。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **11 项**（language 2 + repair 6 + load 3）全绿 |
| 全量 lib 测试 | **389 passed**（上轮 378 + 本次 11） |
| golden 差分测试 | **22 passed**（未触及） |
| clippy 双模式 | 零警告 |

## state-bootstrap 移植进度

主入口 `bootstrapStructuredStateFromMarkdown` 的依赖现已基本就位：
- ✅ StateStore trait（10 号）
- ✅ resolve_durable_story_progress（10 号）
- ✅ 纯逻辑 normalization（09 号）
- ✅ story_markdown 解析（08 号）
- ✅ **编排中间件**（本次：language/repair/load_json/load_hooks）
- ⬜ `load_markdown_*_state`（4 个 markdown 引导：读 *.md → 解析为状态对象）
- ⬜ `load_markdown_bootstrap_state`（聚合 markdown 状态）
- ⬜ `load_or_bootstrap_*`（4 个：读 JSON 或从 markdown 引导，写回）
- ⬜ **主入口 `bootstrap_structured_state_from_markdown`**（编排 4 文件 + manifest）

下一目标：移植 `load_markdown_*_state` + `load_markdown_bootstrap_state`（markdown 引导层），
然后是 `load_or_bootstrap_*` 与主入口，完成 state-bootstrap 完整移植。
