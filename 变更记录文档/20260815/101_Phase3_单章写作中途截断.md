# 101 号变更记录：单章写作中途截断（writer 编排链内安全点，67 号备案闭合）

## 一、背景

100 号变更记录"下一步"首选：单章写作中途截断——67 号备案"TS `runWithAbortSignal` 在 pipeline 内部检查点抛中止；Rust 66 号写作链无内建中止信号，67 号多章轮间轮询 abort，单章中途截断随写作链中止信号专项补"。

TS 语义勘测（`packages/core/src/pipeline/runner.ts`）：
- signal 经 `AsyncLocalStorage` 注入（`runWithAbortSignal`），`throwIfOperationAborted()` 在 `_writeNextChapterLocked` 四处检查：**章首（L1795）/ 草稿后（L1844，writeChapter 返回即查）/ 审查环收敛后（L1932，manual 模式同样经过）/ promotion 后落盘前（L1957）**；`writeChapters` 循环每轮再查（L1754）；`importChapters` 每章 + 分析后查（L2858/2896，另行范围）。
- 检查点间无落盘动作（落盘是单段持久化），中止即全书保持检查点前原样——"安全点"语义的本体。
- studio 侧：确认任务用 `createWriteNextChapterTool` 包 `runWithAbortSignal(signal, ...)`；**聊天面 sub_agent 同样经 `runPipelineWithAbortSignal` 注入**（agent-tools.ts）——两面都要接线。
- 中止错误呈现：非忙错误走 `formatAgentActionFailure` → 502 `AGENT_ACTION_FAILED` + 原文透传（agent:error / tool:end isError 同面）。

## 二、交付

### 1. `pipeline/write_next.rs`：链内安全点检查点

- `WriteNextConfig` 增 `abort: Option<AbortHandle>`（`Arc<Mutex<bool>>`，None 全链不可截断，Default None）。
- `WriteNextError` 增 `Aborted` 变体（英文文案 `"Operation aborted: the user requested to stop this task."`，与 67 号轮间中止文案同源）。
- `check_aborted(config)` 助手：置位即抛；**四处插入**与 TS 逐位对齐——①`write_next_chapter_locked` 首行（ensure_control_documents 前）②`write_chapter` 返回后（writer_count 前）③审查环/manual 块收敛后（promotion 前）④`run_promotion_pass` 后、持久化装配前。安全点选取原则：检查点之间不存在部分落盘（persist 为 staged→backup→就位三段式原子事务），中止即零残留。

### 2. `server/agent_production.rs`：确认面接线

- `execute_write_next` 单章/多章双分支共用 `WriteNextConfig { abort: Some(abort.clone()), ..Default::default() }`（多章轮间轮询保留——与链内检查点叠加，语义更严）。
- `write_next_error_text(error, lang)`：`Aborted` → 双语逐字（`"操作已中止：用户请求停止该任务。"` / `"Operation aborted: the user requested to stop this task."`，与轮间中止同一文案），其余 `to_string` 不变。
- 顺带修正单章分支一处历史缩进异常。

### 3. `interaction/sub_agent_tool.rs`：聊天面接线（TS runPipelineWithAbortSignal 等价）

- `SubAgentDeps` 增 `abort: Option<AbortHandle>`；`agent_route.rs` 装配处注入聊天轮的 `abort_flag`（与 agent loop 轮询同一句柄——abort 端点 scope=chat 即可中止链内写作）。
- `writer` 执行器：config 透传 + 多章轮间预查 + `Err(Aborted)` 分支 → 双语逐字错误结果（其余错误面 to_string 不变）。

### 4. 测试

- **单测 2**（write_next.rs tests）：`abort_before_entry_stops_without_any_llm_call`（预置位 → 入口即停：零 LLM 调用 + 零落盘 + 章号仍 1）；`abort_during_draft_stops_at_safe_point_without_persistence`（`AbortingWriter` 在草稿 LLM 调用内自置位 → 检查点②命中：草稿已生成但章节/索引/真相面全部零落盘）。
- **E2E 1**（`sub101_e2e::confirmed_write_next_abort_mid_draft_stops_at_safe_point`）：慢作家 mock（标记后睡 600ms，测试钉死"草稿生成中"时序无抖动）→ 确认任务起跑 → 轮询见草稿在途 → POST abort（`aborted:true`）→ 断言 **502 + `AGENT_ACTION_FAILED` + 双语错误逐字**、章节目录/真相面零落盘、任务快照 `status=error` + 错误文本、广播序 agent:start → tool:start → agent:aborted（插队）→ tool:end(isError:true)。

## 三、parity 要点

- 四检查点相位逐位对齐 TS L1795/L1844/L1932/L1957；manual 审核模式同样过③④。
- 双面接线对齐 TS（确认任务 + 聊天面 sub_agent 皆有 signal 注入）。
- 中止文案与 67 号轮间中止统一（双语逐字）；错误面 502 `AGENT_ACTION_FAILED` 走既有 `format_agent_action_failure`。

## 四、偏差备案

1. **审查环内部无检查点**：TS 环内（assess→revise 迭代间）同样不查（`runChapterReviewCycle` 未收 signal）——逐位一致，非偏差。
2. **import 链检查点未接**：TS `importChapters` 每章 + 分析后有检查点（L2858/2896），Rust `import_chapters_chain_with_resume` 本轮未接（长链中止属同族但独立链本体，规模可控，随 102 号候选）。
3. **REST `/books/:id/draft|write` 面不接 abort**：TS REST 端点同样无 signal 面（studio 确认面才传），一致。

## 五、暂缓件（滚动）

- P2 清单**全清**（PDF 抽取 100 号 + 单章截断 101 号）。
- 候选余量：import 链检查点（同族小件）、P3 精修批（层 3 secrets 保序 / responses 传输 / provider 特判族 / 模型卡元数据）、迁移收官审计轮（全备案复核 + 94/98 基线终版 + 迁移总结文档）。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1141** 过（+2） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **166** 过（+1：sub101） |
| `cargo test --features export-bindings --lib` | **1300** 过（+2） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（102 号候选）

1. **首选：迁移收官审计轮**——全备案清单复核（30-101 号偏差备案逐条销账或升级为正式缺口）、94/98 基线终版（P2 全清后状态刷新）、strangler 切换总结文档（迁移完成度盘点）。P1/P2 全清后这是切换前最后一块。
2. 其次：import 链检查点（TS importChapters 每章 + 分析后中止点，`import_chapters_chain_with_resume` 接 `Option<AbortHandle>`，同族小件一轮可清）。
3. 或：P3 精修批（层 3 secrets 保序 / responses 传输 / provider 特判族 / 模型卡元数据）。
