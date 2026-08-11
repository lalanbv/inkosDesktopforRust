# 22 — Phase 1 utils 域：safe_child_path + hook-ledger-validator

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`21_Phase3_prompts域_short-fiction完整.md`

## 移植内容

### 1. `utils/path.rs`（safe_child_path 追加）
移植 `path-safety.ts`（11 行）——路径遍历防护：
- [`safe_child_path`]：逻辑 resolve（组件级消解 `.`/`..`，不碰 fs）+ `is_same_or_descendant`（按组件前缀匹配，防 "rootx" 假前缀）
- 越界（`..` 逃逸）或绝对路径劫持 → `Err`（对齐 TS throw `Path traversal blocked`）

### 2. `utils/hook_ledger_validator.rs`（新增，hook 域补全）
移植 `hook-ledger-validator.ts`（277 行）——Phase 9-3 hard gate：
- [`parse_hook_ledger`]：提取 `## 本章 hook 账` / `## Hook ledger for this chapter` 段 + open/advance/resolve/defer 子段 + `[new]` 占位计数
- [`validate_hook_ledger`]：advance/resolve 关键词证据检查（warning）+ 揭1埋1 硬下限（critical）
- [`extract_keywords`]：引号名优先，否则 →/-> 前文本；CJK runs（≥2 字符）+ 2-gram（≥3）+ 3gram（≥4 取首尾）+ ASCII 词（≥3），去停用词去重
- `draft_echoes_entry`：关键词子串匹配（ASCII 不区分大小写）/ 裸 id 词边界回退

## 关键技术点

- **逻辑 resolve 不碰 fs**：TS `resolve` 是纯字符串语义（不要求路径存在）；Rust 用 `Component` 遍历消解 `.`/`..`，
  避免 `canonicalize` 的 fs 依赖，使 `safe_child_path` 可纯单测。
- **「揭 1 埋 1」硬下限**：`resolved > 0 && opened < resolved` → critical。`opened = open.len() + new_open_count`
  （`[new]` 占位行是 planner 声明新 hook 的常用方式，无 id 但计入埋设数）。
- **关键词提取的优先级**：引号名（`"胖虎借条"`）最信息丰富，writer 应回声；无引号则取 →/-> 前文本
  （→ 后是新状态描述，易致角色名误命中）。CJK 拆 2/3-gram 使部分回声仍计数。
- **词边界 escape**：裸 ASCII id 用 `regex::escape` + `\b` 边界（防 H0077 假匹配 H007）。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **16 项**（path safe_child 5 + hook_ledger 11）全绿 |
| 全量 lib 测试 | **497 passed**（上轮 481 + path 5 + ledger 11） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## utils 域 hook 子链完整度

hook 子链（policy→lifecycle→governance→arbiter→stale→promotion）+ **ledger_validator**（本次）。
hook-health（192 行）仍依赖 continuity.ts 的 AuditIssue，待 continuity 移植后补。

## 会话累计（20 个里程碑）

lib 测试 278 → **497**（+219 测试，0 回归），22 golden 全绿，clippy 双模式零警告。
- state 域全量 + utils hook 全链路 + agents 6 叶子 + prompts 100% + path 安全 + hook ledger 校验
