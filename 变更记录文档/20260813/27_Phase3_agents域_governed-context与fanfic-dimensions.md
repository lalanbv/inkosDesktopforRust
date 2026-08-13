# 27 — Phase 3 agents 域：governed-context 与 fanfic-dimensions（ContinuityAuditor 解锁链第二段）

> 日期：2026-08-13
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`26_Phase3_agents域_rules-reader与规则解析层.md`
> 会话背景：Claude Code 时代（06–25 号）之后的 ZCode 第二个里程碑。已读取
> Claude Code 项目记忆（~/.claude/projects/...）与最后会话收尾记录，路线延续其
> 「下一会话首目标：governed-context / rules-reader 解锁 ContinuityAuditor」。

## 移植内容

### `models/input_governance.rs` — 补齐剩余类型（文件 100% 完成）
移植 `input-governance.ts` 除 ChapterMemo 外的全部类型（124 行 TS 全量）：
- `ChapterIntent`（planner 产物）/ `ContextSource` / **`ContextPackage`**
  （governed-context / ContinuityAuditor 的输入）
- `RuleStack` 族：`RuleLayerScope` / `RuleLayer` / `OverrideEdge` / `ActiveOverride` /
  `RuleStackSections` / **`RuleStack`**
- `ChapterTrace` 族：`TraceTokenBudget` / `TraceContextTiers` / `TraceCompression` /
  `ChapterTrace`（pipeline 追踪产物）
- zod 的 min(1)/int 约束由构造方保证（与 ChapterMemo 同策略），类型层只承载形状

### `utils/governed_context.rs`（新文件）
移植 `governed-context.ts`（101 行，纯逻辑）：
- [`build_governed_memory_evidence_blocks`]：ContextPackage.selectedContext 按 source
  前缀/全等分 7 桶（伏笔 / hook 债 / 章节摘要 / 卷级摘要 / 标题历史 / 情绪轨迹 / 正典）
- `render_evidence_block`（`- {source}: {excerpt ?? reason}`）与
  `render_hook_debt_block`（无 source 前缀，`- {excerpt ?? reason}`）逐字移植
- 中英标题分语（hook 债块两语言同为 "Hook Debt Briefs"——TS 原文如此，保留）

### `agents/fanfic_dimensions.rs`（新文件）
移植 `fanfic-dimensions.ts`（87 行，纯逻辑）：
- `FANFIC_DIMENSIONS` 常量（维度 34-37 定义 + 中文 baseNote）
- [`get_fanfic_dimension_config`]：模式（canon/au/ooc/cp）→ 严重度覆盖（SEVERITY_MAP
  逐字移植为 match）+ 注记（baseNote + severityLabel）+ 维度 1 的 ooc 放宽/canon 收紧
  + 番外维度 28-31 停用
- `_allowed_deviations` 参数 TS 实现未用，保留签名对齐（有单测钉死惰性）

### `agents/continuity.rs` — AuditSeverity 增强
`AuditSeverity` 补 serde（小写 rename）+ ts-rs 绑定（`"critical" | "warning" | "info"`），
供 fanfic-config 序列化与 golden 差分。

## 关键技术点

- **Map → BTreeMap**：TS `Map<number, ...>` 在 Rust 用 `BTreeMap<u32, ...>`——序列化键序
  确定，golden 差分中与 TS `Object.fromEntries`（数字键字符串化）同形。
- **undefined ↔ None ↔ 省略键**：TS 返回对象的 undefined 字段被 JSON.stringify 丢弃；
  Rust `Option` + `skip_serializing_if` 对齐（empty-package 向量断言 `{}`）。
- **前缀桶与全等桶双计入**：`recent_titles` 同时命中 chapter_summaries 前缀桶与标题
  历史全等桶——TS 行为如此（前缀过滤不排除全等桶），逐字保留并有单测钉死。
- **ts-rs 与 serde rename_all 不兼容**：带 `ts(type)` 的枚举不能用容器级
  `rename_all`（ts-rs 报错），须逐 variant `#[serde(rename)]`（与 book.rs Platform 同模式）。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **11 项**（input_governance 3 + governed_context 4 + fanfic_dimensions 4）全绿 |
| 全量 lib 测试 | **572 passed**（上轮 561 + 本次 11） |
| golden 差分测试 | **26 passed**（上轮 24 + 本次 2 域：governed-context 5 向量、fanfic-config 4 模式），本次零分歧 |
| clippy 双模式 | 零警告 |

## 解锁进度（ContinuityAuditor 路线）

- ✅ rules-reader（26 号）
- ✅ ContextPackage / RuleStack 类型层（本次）
- ✅ governed-context（本次）
- ✅ fanfic-dimensions（本次）
- ⬜ `utils/outline-paths.ts`（340 行，fs 读取：readVolumeMap / readCharacterContext /
  readCurrentStateWithFallback）——**最后一个阻塞项**
- ⬜ ContinuityAuditor 主体（continuity.ts 375-842 行：auditChapter 编排 + 维度注记
  buildDimensionNote/buildDimensionList + 审查结果解析 parseAuditResult）

## 会话累计（26 个里程碑）

lib 测试 278 → **572**（+294 测试，0 回归），26 golden 全绿，clippy 双模式零警告。
