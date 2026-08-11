# 17 — Phase 3 agents 域：纯逻辑叶子群（settler-delta-parser + ai-tells）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`16_Phase3_agents域_首批叶子.md`

## 移植内容

### 1. `agents/settler_delta_parser.rs`
移植 `settler-delta-parser.ts`（53 行）——结算输出 RUNTIME_STATE_DELTA 段提取 + 反序列化：
- [`parse_settler_delta_output`]：提取段 → `sanitize_json`（剥控制字符 + 尾逗号）→ `strip_code_fence` → serde 反序列化为 `RuntimeStateDelta`
- 复用 [`settler_parser::extract_tag`]（提升 `pub(crate)`，同形态 TAG 提取）
- `RuntimeStateDeltaSchema.parse`（TS zod）→ `serde_json::from_value`（Rust 强类型）

### 2. `agents/ai_tells.rs`
移植 `ai-tells.ts`（161 行）——纯规则 AI 痕迹检测（无 LLM）：
- dim 20：段落长度 CV<0.15（变异系数）
- dim 21：套话词密度 >3 次/千字
- dim 22：公式化转折重复 ≥3 次
- dim 23：列表式结构（连续 ≥3 句同前缀）
- `AITellIssue` / `AITellResult` / `AITellSeverity`；中英文词表（HEDGE/TRANSITION）

## 关键技术点

- **sanitize_json 的两步清理**：TS `sanitizeJSON` 先剥控制字符（0x00-0x08/0B/0C/0E-0x1F/7F）再去尾逗号（`,}`/`,]`）。
  Rust 用 `chars().filter(!matches!(...))` + `replace_all`。`matches!` 宏替代 match（clippy 建议）。
- **代码围栏剥离**：`` ```json ... ``` `` 或 `` ``` ... ``` `` → 内容。OnceLock 缓存 `(?i)^```(?:json)?\s*([\s\S]*?)\s*```$`。
- **UTF-16 长度对齐**：ai-tells 的段落长度/句长用 `encode_utf16().count()`（对齐 TS `.length`），
  句长过滤 `>2`（UTF-16 码元）。
- **AsRef<str> 签名**：`analyze_ai_tells<C: AsRef<str>>` 兼容 `&str`/`String` 调用（TS `content: string`），
  比固定 `&str` 更灵活，零运行时开销。
- **中英文前缀差异**：dim 23 中文取前 2 char（`chars().take(2)`），英文取首词（`split_whitespace().next()`）。
- **count_word 大小写不敏感**：英文用 `(?i)` + `regex::escape`（防特殊字符），中文精确。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **15 项**（settler_delta 7 + ai_tells 8）全绿 |
| 全量 lib 测试 | **445 passed**（上轮 430 + settler_delta 7 + ai_tells 8） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## agents 域进度

纯逻辑叶子群（无 LLM 依赖，可批量移植）：
- ✅ detection_insights（16 号）
- ✅ settler_parser（16 号）
- ✅ **settler_delta_parser**（本次）
- ✅ **ai_tells**（本次）
- ⬜ writer-parser / sensitive-words（依赖 continuity.ts 的 AuditIssue） / rules-reader / radar
- ⬜ prompt 构造（planner-prompts / observer-prompts / settler-prompts）
- ⬜ agent 编排（architect/consolidator/planner/composer，依赖 streaming_client + state）

## 会话累计

本会话（06–17 号）累计：
- **state 域全量 Rust 化**（I/O 编排主链贯通）
- **utils 域 hook 子链全链路 + story-markdown 解析全函数**
- **agents 域纯逻辑叶子群 4 个**（Phase 3 开端）

lib 测试 278 → **445**（+167 测试，0 回归），22 golden 全绿，clippy 双模式零警告。
