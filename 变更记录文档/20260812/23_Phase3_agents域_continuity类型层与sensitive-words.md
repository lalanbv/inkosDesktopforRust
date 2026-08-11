# 23 — Phase 3 agents 域：continuity 类型层 + sensitive-words

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`22_Phase1_utils域_safe-child-path与hook-ledger-validator.md`

## 移植内容

### 1. `agents/continuity.rs`（纯类型 + 维度层）
移植 `continuity.ts`（842 行）的纯叶子层（`ContinuityAuditor` BaseAgent 留待后续）：

- **类型**：`AuditResult` / `AuditIssue` / `AuditSeverity` / `RepairScope` / `AuditTokenUsage`
- **37 维度标签**（DIMENSION_LABELS，中英，OnceLock HashMap）—— OOC/时间线/设定冲突/.../正典事件一致性
- **工具函数**：`normalize_repair_scope` / `contains_chinese` / `resolve_genre_label` / `dimension_name` /
  `join_localized` / `format_fanfic_severity_note`

### 2. `agents/sensitive_words.rs`
移植 `sensitive-words.ts`（142 行）——纯规则敏感词检测：

- 3 内置词表：政治敏感（block → critical）/ 色情（warn）/ 极端暴力（warn）+ 可选自定义词（warn）
- `SensitiveWordMatch` / `SensitiveWordResult` / `SensitiveWordSeverity`
- `scan_words`：`regex::escape` 字面计数（含元字符转义）

## 关键技术点

- **解锁链验证**：`AuditIssue` 是 writer-parser / sensitive-words / hook-health 的共同依赖。
  continuity 类型层先移植 → sensitive_words 立即解锁并移植，验证了「自下而上、解锁下游」策略有效。
- **37 维度标签 OnceLock**：编译期 `[(u32, &str, &str); 37]` 数组 + `OnceLock<HashMap>` 懒构造，
  零运行时分配（首次访问后稳定）。覆盖 1-37 全部 id（测试逐 id 校验中英存在）。
- **resolve_genre_label 的中英分流**：中文语言或 profileName 无中文 → 用 profileName；
  英文 + 含中文 profileName + genre=other → "general"；余者 genre 的 `_-` 转空格。
- **敏感词字面匹配**：`regex::escape(word)` 后全局 `find_iter().count()`，元字符（`a+b`）安全字面匹配。
- **WritingLanguage 替代 PromptLanguage/SensitiveWordLanguage**：复用 crate 枚举，语义等价（zh/en）。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **16 项**（continuity 8 + sensitive_words 8）全绿 |
| 全量 lib 测试 | **513 passed**（上轮 497 + continuity 8 + sensitive 8） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## agents 域进度

- ✅ detection_insights / settler_parser / settler_delta_parser / ai_tells / style_analyzer / detector
- ✅ **continuity 类型+维度层**（本次，解锁 AuditIssue）
- ✅ **sensitive_words**（本次，首个 continuity 解锁的下游）
- ⬜ writer-parser（依赖 writer.ts + StyleProfile） / hook-health（依赖 continuity 已 ✓ + hook-governance ✓，可移植）

## 会话累计（22 个里程碑）

lib 测试 278 → **513**（+235 测试，0 回归），22 golden 全绿，clippy 双模式零警告。
