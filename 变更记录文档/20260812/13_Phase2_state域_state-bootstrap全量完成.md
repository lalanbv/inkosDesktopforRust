# 13 — Phase 2 state 域：state-bootstrap 全量 Rust 化完成（主入口编排）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`12_Phase2_state域_state-bootstrap-markdown引导层.md`
> 性质：**里程碑**——state-bootstrap 全量 Rust 化，runtime-state-store 解锁

## 移植内容

### `state/state_bootstrap.rs`（主入口编排完成）
移植最后一块：`load_or_bootstrap_*` 系列 + 主入口。

- [`load_or_bootstrap_summaries`]：读 JSON；失败/缺失 → markdown bootstrap；即使从 JSON 加载也去重（历史数据可能含重复）；写回 + 记录新文件
- [`load_or_bootstrap_hooks`]：读 JSON（含 repair，repair 后写回）；失败/缺失 → markdown bootstrap
- [`load_or_bootstrap_current_state`]：读 JSON；失败/缺失 → markdown bootstrap
- **[`bootstrap_structured_state_from_markdown`]**：主入口，4 文件编排
- [`BootstrapStructuredStateResult`]：返回值（createdFiles/warnings/manifest）

## 主入口编排流程（对齐 TS `bootstrapStructuredStateFromMarkdown`）

```
1. mkdir {book_dir}/story/state
2. 读 existing manifest（合法则保留 schemaVersion/projectionVersion/migrationWarnings/language）
3. language = existing 合法值 ?? book.json 推断（仅 "zh" 认定）
4. load_markdown_bootstrap_state（summaries/hooks/current + durable_progress 聚合）
5. 三域 load_or_bootstrap（传入预加载 bootstrapState 避免重复读 md）：
   - summaries/hooks/current_state 各自「读 JSON 或从 markdown 引导，写回，记录 createdFiles」
6. derived_progress = markdown durable（max(显式 fallback, 章节产物前缀)）
   existing.lastAppliedChapter > derived → warning 规范化
7. 写 manifest（schemaVersion=2, language, lastApplied=derived, projectionVersion, migrationWarnings 合并去重）
8. 返回 result（createdFiles/warnings/manifest）
```

## 关键技术点

- **幂等性**：已存在的合法 JSON 保留并校验/修复；缺失才从 markdown 引导并写入。manifest 始终重算 progress。
  对齐 TS 的「重建而非破坏」语义——多次调用不丢用户数据。
- **hooks repair 持久化**：`load_hooks_state_if_valid` 返回 `repaired=true` 时，主入口写回修复后的 JSON
  （与 TS 一致：`if (existing.repaired) writeFile(...)`）。
- **summaries JSON 去重**：即使从 JSON 加载也走 `deduplicate_summary_rows`（TS 注释：stale data may have duplicates），
  去重后行数减少则写回。
- **derived_progress 反幻觉规范化**：`existing.lastAppliedChapter > derived_progress` → warning + 规范化到 derived
  （防止 manifest 残留的幻觉大数字，如 1988 章）。
- **migrationWarnings 合并去重**：existing manifest 的历史 warnings + 本次 warnings 经 `unique_strings` 合并。
- **language 双源**：existing manifest 的合法 `"zh"`/`"en"` 优先（保留用户设定），否则读 book.json。
- **writing_language 不强转**：manifest.language 本是 String（对齐 TS），主入口直接用字符串，无需 WritingLanguage 枚举转换。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **6 项**端到端（全新建/保留合法/repair hooks/损坏重建/summaries 去重/fallback）全绿 |
| state_bootstrap 模块总测试 | **49 项**（纯逻辑 16 + async 7 + 中间件 11 + markdown 引导 9 + 主入口 6） |
| 全量 lib 测试 | **404 passed**（上轮 398 + 本次 6） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## state-bootstrap 全量移植完成

state-bootstrap（645 行 TS）**全量 Rust 化**，跨 4 个会话里程碑（09 纯逻辑 / 10 async 基座+durable / 11 中间件 / 12 markdown 引导 / 13 主入口）。

移植完整清单：
- 纯逻辑：`resolve_contiguous_chapter_prefix` / `deduplicate_summary_rows` / `normalize_hook_status` /
  `normalize_hook_type` / `parse_strict_integer_*` / `normalize_explicit_chapter` / `append_warning` / `unique_strings`
- async 编排：`resolve_durable_story_progress` / `resolve_runtime_language` / `repair_hooks_state_value` /
  `load_json_if_valid` / `load_hooks_state_if_valid` / `load_markdown_*_state` / `load_markdown_bootstrap_state` /
  `load_or_bootstrap_*` / **`bootstrap_structured_state_from_markdown`**

## runtime-state-store 解锁

runtime-state-store（164 行）的全部依赖现已就位：
- ✅ memory_db（rusqlite 持久化）
- ✅ reducer（增量归约）
- ✅ validator（快照校验）
- ✅ hook_arbiter（hook 裁决）
- ✅ **state_bootstrap（全量，本次完成）**

runtime-state-store 是 state 域 I/O 编排层主链的最后一环——它消费以上全部，对外暴露 `loadRuntimeStateSnapshot` /
`buildRuntimeStateArtifacts` / `saveRuntimeStateSnapshot`。完成后 state 域 I/O 编排层主链贯通。
