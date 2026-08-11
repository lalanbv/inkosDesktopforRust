# 25 — Phase 3 agents 域：writer-parser（章节输出解析）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`24_Phase1_utils域_hook-health.md`

## 移植内容

### `agents/writer_parser.rs`
移植 `writer-parser.ts`（178 行）——LLM 章节 `=== TAG ===` 输出解析：

- [`parse_creative_output`]：CHAPTER_TITLE / CHAPTER_CONTENT / PRE_WRITE_CHECK 提取 + fallback（小模型常缺 tag）
- [`parse_writer_output`]：全字段（title/content/state/ledger/hooks/summary/subplots/emotionalArcs/characterMatrix）+ numericalSystem 门控 ledger + 中英占位
- [`fallback_extract_content`] / [`fallback_extract_title`]：`# 第N章` / `Chapter N` / `正文：` 标签 / 最后手段剥离 tag+kv 行（>100 字符才采纳）
- `CreativeOutput` / `ParsedWriterOutput` 结构 + 中英 default 占位

## 关键技术点

- **复用 extract_tag**：settler_parser 的 `extract_tag`（lookahead 手动扫描，16 号实现）直接复用——
  writer-parser 的 TS 正则用同样的 `(?==== [A-Z_]+ ===|$)` lookahead，Rust regex 不支持，
  复用已验证的手动扫描方案，零重复实现。
- **fallback 三级策略**：小模型常缺 `=== TAG ===`，按 `# 第N章` heading → `正文：` label →
  最后手段剥离 tag/metadata-kv 行（>100 字符阈值）逐级回退。
- **LengthCountingMode 中英分流**：英文模式用 `# Chapter N` / `content:` / `Chapter {n}` 默认；
  中文用 `# 第N章` / `正文` / `第{n}章`。
- **numericalSystem 门控**：`updated_ledger` 仅在 genre profile 开启数值系统时填充（否则空串），
  与 settler_parser 同设计。
- **ParsedWriterOutput 字段子集**：TS `Omit<WriteChapterOutput, "postWriteErrors"|"postWriteWarnings">`，
  Rust 独立定义（不依赖未移植的 writer.ts 全类型）。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **9 项**（creative tagged/fallback heading、writer 全字段/numerical 门控/默认/英文、fallback 剥离/过短空/label 标题）全绿 |
| 全量 lib 测试 | **529 passed**（上轮 520 + 本次 9） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## agents 域进度

纯逻辑/解析叶子（无 LLM 调用）：detection_insights / settler_parser / settler_delta_parser /
ai_tells / style_analyzer / continuity 类型层 / sensitive_words / **writer_parser**（本次）= 8 个解析叶子 +
detector（HTTP）。

agents 域**纯解析叶子群已基本移植完毕**。剩余主要 ContinuityAuditor（BaseAgent 主体，依赖 rules-reader/governed-context 未移植）+ agent 编排群（architect/consolidator/planner/composer，依赖 streaming_client + state）。

## 会话累计（24 个里程碑）

lib 测试 278 → **529**（+251 测试，0 回归），22 golden 全绿，clippy 双模式零警告。
