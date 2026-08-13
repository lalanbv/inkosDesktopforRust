# 28 — Phase 3 agents 域：outline-paths 与 ContinuityAuditor 主体（解锁链收官）

> 日期：2026-08-14
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`27_Phase3_agents域_governed-context与fanfic-dimensions.md`
> 会话背景：延续 27 号「下一目标：outline-paths（最后阻塞项）→ ContinuityAuditor 主体」。
> 本次完成后 **ContinuityAuditor 解锁链全部贯通**——agents 域首个完整 agent 编排落地。

## 移植内容

### `utils/outline_paths.rs`（新文件，340 行 TS 全量）
移植 `outline-paths.ts`——Phase 5 (v13) 散文大纲路径解析：
- `is_new_layout_book` / `is_book_foundation_complete`（五节地基检查 +
  roles 目录 / legacy 角色矩阵双源）
- `has_legacy_character_matrix_roles`（shim 噪声过滤正则逐字移植）
- `read_story_frame` / `read_volume_map` / `read_rhythm_principles`
  （新布局优先 → legacy 回退 → placeholder）
- `RoleCard` / `read_role_cards` / `read_character_context`
  （中英四 tier 目录收集 + 分组渲染）
- `is_current_state_seed_placeholder`（UTF-16 码元 600 阈值）/
  `read_current_state_with_fallback`（种子占位时从 roles 当前现状 +
  pending_hooks startChapter=0 行派生初始状态块）
- 附带提取器：`extract_current_state_from_role`（中英标题正则 + 截断到下个 `## `）、
  `extract_seed_hooks_from_pending_hooks`（markdown 表格行解析）

### `agents/continuity.rs` — ContinuityAuditor 主体（continuity.ts 375-842 行）
- **维度注记** [`build_dimension_note`]：37 维度双语注记逐字移植（fanfic notes /
  OOC 模式放宽 / 疲劳词 / 爽点类型 / 年代约束 / v10 增强版 7·15·25 /
  Phase 7 hook-debt 升级长文 / 同人 34-37 严重度追加）
- **激活集** [`build_dimension_list`]：gp.auditDimensions ∪ 书级追加（数字 id +
  名称精确/子串模糊匹配）∪ 恒加 {32,33} ∪ era 条件 {12} ∪ parent_canon 番外
  {28-31}（同人模式下停用）⊕ fanfic 激活/停用集，数值序输出
- **解析链**：`extract_balanced_json`（平衡花括号）/ `try_parse_audit_json` /
  [`parse_audit_result`] 四策略（平衡 JSON → 纯 JSON → ```json 代码块 →
  逐字段正则）+ parseFailed 兜底（System Error critical issue）
- **精简控制块** [`build_reduced_control_block`]：selectedContext / 规则栈三节 /
  生效覆盖的审查视角渲染
- **编排** [`audit_chapter`]：11 路真相文件并行加载（current_state 派生回退）→
  truthFileOverrides 覆盖 → 上一章全文 → 规则面（genre 画像/book 语言/book_rules）→
  维度列表 + system/user prompt 逐字构造 → prompt pack 指导追加
  （longform.auditor）→ context_filter 过滤块 / governed 记忆块覆盖 →
  LLM chat（eraResearch 时带搜索路由）→ 四策略解析 + token 用量回填
- **[`AuditorChat`] trait**：TS `BaseAgent.chat` / `chatWithSearch` 的最小面
  （`chat_with_search` 默认回落 `chat`），编排层与 LLM 实现解耦——
  测试注入 mock，生产实现由未来 BaseAgent/LLMRouter 移植提供
  （与 detector 注入 client、StateStore 抽象 fs 同模式）

### 基础设施
- `utils/language.rs`：`utf16_len` 提升为公共函数（JS `String.length` parity 基准，
  outline_paths 与 chapter_memo_parser 共用）

## 关键技术点

- **parseInt parity**：TS `Number.parseInt(cells[1], 10)` 宽松前缀解析
  （"0.5"→0 通过、"abc"→NaN 跳过）——Rust 手写 `parse_int_prefix`
  （`^[+-]?[0-9]+` 前缀 + trim_start），单测钉死 JS 边界（双符号 NaN、
  前导空白跳过）。
- **UTF-16 阈值**：`is_current_state_seed_placeholder` 的 600 字符分支点用
  `encode_utf16().count()` 对齐 JS `.length`（BMP 中文 1 码元、emoji 2 码元），
  golden 向量钉死 600/601 边界与代理对。
- **readdir 顺序语义**：TS `readRoleCards` 用 `Promise.all` 并发 push（竞态序）；
  Rust 按「主要→次要→major→minor」串行序收集——实践等价（首个发起的目录最先
  完成）且确定可复现。目录内条目序两实现同为 OS 序（无排序保证），单测用
  排序断言。
- **分支顺序即行为**：`build_dimension_note` 中 id=15 的简版提前返回使 v10
  增强版的 base 非空分支不可达、id=25 的 v10 三问使 switch 情绪弧线版不可达
  ——逐字保留死分支形状，单测钉死「永不产出」。
- **Set<number> 键语义**：TS `Set<number>` 收纳非整数（1.5）后
  `dimensionName(1.5)` 查不到而跳过——Rust `as_dimension_id`（非整/超界 →
  None）等价复刻。
- **?? 合并语义**：`overall_score ?? overallScore` / `repair_scope ?? repairScope`
  用 `coalesce`（缺失或 null 取右值）精确对齐；非字符串 severity 在 TS 运行时
  保留但下游按非 critical/info 处理 → Rust 枚举收拢 Warning（行为等价，注释说明）。
- **平衡 JSON 不感知字符串语义**：`{"x": "a}b"}` 在字符串内 `}` 处截断——
  TS/Rust 同样按字符计数，单测钉死。
- **策略 4 触发条件**：任何含完整平衡 JSON 对象（即使无 passed 字段）的内容都
  走策略 1（passed 缺失视为合法）——策略 4 仅在「无任何平衡对象」时到达，
  单测注释说明。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **35 项**（outline_paths 17 + continuity 18）全绿 |
| 全量测试 | **651 passed**（上轮 572 + 52 新增 + golden 27 含新域），0 回归 |
| golden 差分 | **27 passed**（+1 域：is_current_state_seed_placeholder 10 向量——
  UTF-16 边界 600/601、代理对长度、中英标记、超长内容），本次零分歧 |
| clippy（lib+tests） | 零警告 |

## 解锁进度（ContinuityAuditor 路线）

- ✅ rules-reader（26 号）
- ✅ ContextPackage / RuleStack 类型层（27 号）
- ✅ governed-context（27 号）
- ✅ fanfic-dimensions（27 号）
- ✅ outline-paths（本次）
- ✅ **ContinuityAuditor 主体（本次）——路线收官**

## 下一步候选（按依赖序）

1. `agents/writer.ts` 等编排 agent（复用本次 AuditorChat 注入模式 +
   rules-reader / outline-paths 数据链）
2. `BaseAgent` / `chatWithSearch` 的 LLMRouter 生产实现（llm 域 streaming_client
   之上，落地 AuditorChat 的真实现）
3. pipeline 域编排（agent 全链路 → strangler 端点切换）

## 会话累计（27 个里程碑）

lib 测试 572 → **624**（+52），golden 26 → **27 域**全绿，clippy 双模式零警告。
