# 16 — Phase 3 agents 域：首批纯逻辑叶子（detection-insights + settler-parser）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`15_Phase2_state域_chapter-workspace.md`
> 性质：Phase 3 agents 域移植开端（纯逻辑叶子）

## 移植内容

### 模块结构
把 `agents.rs`（占位）改为 `agents/` 目录模块，启动 Phase 3 agents 域自下而上移植。

### 1. `agents/detection_insights.rs`
移植 `detection-insights.ts`（72 行）——检测历史聚合统计：
- detect/rewrite 总数；按章分组，每章取首/末 attempt 的 score 为 original/final
- passRate（finalScore <= originalScore 的章节占比）；平均分 round3（对齐 `Math.round(x*1000)/1000`）

### 2. `agents/settler_parser.rs`
移植 `settler-parser.ts`（38 行）——结算输出 `=== TAG ===` 段提取：
- 8 个 TAG（POST_SETTLEMENT/UPDATED_STATE/UPDATED_LEDGER/UPDATED_HOOKS/CHAPTER_SUMMARY/...）
- `numerical_system` 门控 UPDATED_LEDGER；缺省中文兜底文案（"(状态卡未更新)" 等）

## 关键技术点

- **Rust regex 不支持 lookahead**：TS `extract` 用 `(?==== [A-Z_]+ ===|$)` lookahead 界定段尾。
  Rust `regex` crate 无 lookahead/lookbehind——改用手动扫描 [`find_next_tag_header`]：
  定位 `=== {tag} ===` 后，从下一个行首 `=== [A-Z_]+ ===` 处截断。语义等价，零正则特性依赖。
  **这是本项目首次遇到 Rust regex vs JS regex 的特性差异**（此前移植的正则均无 lookahead），
  记录为后续移植的经验：含 lookahead/lookbehind 的 TS 正则需改写。
- **round3 精度对齐**：TS `Math.round(x * 1000) / 1000` → Rust `(x * 1000.0).round() / 1000.0`，
  IEEE754 行为一致。passRate 同理用 100（2 位小数）。
- **DetectionAction 枚举**：TS `"detect" | "rewrite"` 字符串 → Rust 枚举，`==` 比较更安全。
- **SettlementOutput 全字段**：8 个字段对齐 TS，Default 派生便于测试。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **11 项**（detection 5 + settler 6）全绿 |
| 全量 lib 测试 | **430 passed**（上轮 419 + 本次 11） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## agents 域移植开端

agents 域约 30 个文件，分三类：
- **纯逻辑解析叶子**（无 LLM）：detection-insights ✓ / settler-parser ✓ / settler-delta-parser /
  writer-parser / ai-tells / sensitive-words / rules-reader / radar 等
- **prompt 构造**：planner-prompts / observer-prompts / settler-prompts / en-prompt-sections 等
- **agent 编排**（依赖 streaming_client + state）：architect / consolidator / planner / composer 等

本会话完成首批 2 个叶子。agents 域的纯逻辑叶子群可继续批量移植（每个独立可测），
agent 编排则需待 streaming_client 与 state 完全打通（state 已全量 ✓，streaming_client 已功能性子集 ✓）。

## 会话总结

本会话（06–16 号，11 个里程碑）完整交付：
- **state 域全量 Rust 化**（I/O 编排主链贯通：memory_db / store / state_bootstrap / runtime_state_store / chapter_workspace）
- **utils 域 hook 子链全链路 + story-markdown 解析全函数**
- **Phase 3 agents 域开端**

lib 测试 278 → **430**（+152 测试，0 回归），22 golden 全绿，clippy 双模式零警告。
变更记录 06–16 归档于 `变更记录文档/20260812/`。
