# 87 号变更记录：书会话子代理工具（sub_agent）

- 日期：2026-08-14
- 阶段：Phase3 strangler 迁移（engine-rs）
- 契约源：`packages/core/src/agent/agent-tools.ts` L557-955（SubAgentParams / createSubAgentTool：architect/writer/auditor/reviser/exporter 五分支 + 守卫文案 + resolveToolBookId）、`packages/core/src/interaction/export-artifact.ts` L133-157（writeExportArtifact 落盘）、`packages/core/src/agent/agent-session.ts` L929-945（edit/book 分支注册：subAgentTool + generateCover + read + 写工具族 + research/material）

## 一、背景

agent-session 注册面最后的 book/edit 分支以 sub_agent 为核心：书会话里把重操作委托给专门代理（architect 建书 / writer 写章 / auditor 审计 / reviser 修订 / exporter 导出）。Rust 侧五代理的域链全部已在位（67 号确认面执行器、write_next 链、audit 链、export 工件链）——本轮把它们以聊天工具壳接进聊天面。reviser 与 architect.revise 因 revise/rewrite 链（49/54 号形态）与 TS reviseDraft 参数面差异大，独立轮次接入（备案）。

## 二、交付

1. **`engine-rs/src/interaction/sub_agent_tool.rs`**（新建 ~460 行，2 单测）：
   - 守卫逐字：无活动书 → "No active book. Only the architect agent can create a book from this session."；有活动书 + architect → 双语"当前已有书籍，不需要建书。……"；agent 枚举 5 值；instruction 必填
   - **architect**：复用 67 号 `execute_create_book`（提 pub(crate)）——其文本/details 与 TS 聊天分支逐字一致（"Book \"{title}\" ({id}) initialised successfully…" + `{kind:"book_created"}`）；缺 title → "Error: title is required for the architect agent."
   - **writer**：复用 `write_next_chapter` + `build_write_next_agents/ctx` 装配，**默认 Auto 审核模式**（TS writeNextChapter 语义：审后 ready-for-review；Manual 模式审稿环跳过会落 audit-failed——E2E 调试中确认）；单章文本双分支（ready-for-review/active → "Chapter written for \"{id}\". Word count: N."；其他状态 → needs review 提示）+ details `chapter_written`；多章（chapterCount 1-20）循环连写 + 非 ready-for-review 即停 + "Writer completed N of M … stopped because chapter X ended with status …" + details `chapters_written`（chapters 数组 + stoppedStatus）
   - **auditor**：复用 `run_audit_flow`（提 pub(crate)，AuditFlowError 同提）——AuditRuntime 从 BooksRuntime 造；chapterNumber 缺省取最新章（get_next_chapter_number - 1）；文本 "Audit chapter {n}: PASSED/FAILED, {count} issue(s)." + 每行 `[severity] description`
   - **exporter**：`build_export_artifact` + **落盘**（output_path 父目录 mkdir + 写 payload，对齐 writeExportArtifact）；format 从 instruction 推断（epub/markdown/md）+ approvedOnly 推断（approved/已通过/通过章节）；文本 "Exported \"{id}\": N chapters, W words → {path}"
   - **reviser / architect.revise**：暂缓拦截文案
   - schema（SubAgentParams 逐字，20 字段含 reviser/exporter 参数）
2. **注册**：`ChatToolRouter` 五级分发（propose → research/import → sub_agent → play → 文件）；payload 条件——`agent_book_id.is_some()`（书会话）注册 sub_agent schema（无书 chat 不注册，对齐 TS `if (!params.bookId) return []`）
3. **E2E `sub87_e2e`**（+2）：
   - `book_session_sub_agent_writer_auditor_exporter`：书会话三连——①writer 单章全链（复用全局 planner/writer/auditor/settler mock 分流）→ 卡 completed + "Chapter written…" + details chapter_written（ready-for-review）+ 章节落盘；②auditor 审第 1 章 → "Audit chapter 1: …"；③exporter txt → "Exported \"b1\"…" + 导出文件落盘断言
   - `chat_session_without_book_does_not_register_sub_agent`：无书 chat 不注册 sub_agent（注册面断言）
4. E2E 调试固化两条教训：①"写下一章"是 write-next 确认指令白名单词（聊天测试须避开确认面）；②write_next 链内审稿器消费 "PASS\n95" 行协议（非 JSON）

## 三、parity 要点

- **域链单源复用零复制**：architect=确认面执行器、writer=write_next 链、auditor=audit 链、exporter=export 工件链——聊天工具与 REST/确认面同一入口
- **writer Auto 审核模式**：TS writeNextChapter 默认审后 ready-for-review；Manual 模式（books_routes::draft 用）语义是"尚未审查"→ audit-failed——聊天工具取 Default
- **exporter 落盘**：TS writeExportArtifact 写到工件输出路径（build 只产内存 payload）——Rust 工具补 mkdir + 写盘
- **多章停止语义**：非 ready-for-review 状态即停（连续章在同一书锁下顺序写），文本与 details（requestedCount/completedCount/chapters/stoppedStatus）逐字
- **注册矩阵**：无书 chat 空工具集（除专门分支）——sub_agent 仅书会话

## 四、偏差备案

1. **reviser 暂缓**：Rust revise/rewrite 链（49/54 号）与 TS reviseDraft（五模式 + revision gate 诊断 + applied=false 降级文本）参数面差异大——独立轮次接入前返回拦截文案
2. **architect.revise 暂缓**：72 号 revise_foundation 已有 REST 面但接入形态（feedback/备份文案）需对齐——同 reviser 轮次
3. **无 onUpdate 进度**：与 80/83 号同——loop 的 tool:start/end 覆盖
4. **auditor severity 显示**：`[{:?}]` Debug 输出（"Critical"）vs TS severity 原串（"critical"）——大小写差异，后续对齐 AuditSeverity 的 as_str（如有）

## 五、暂缓件

- reviser 五模式 + architect.revise（revise 面接入）
- 书会话其余工具族（generate_cover 聊天壳 / write_truth_file / rename_entity / patch_chapter_text / replace_chapter_text / delete_latest_chapter）
- 既有备案延续：PDF 抽取（83 号）、zh 提示词逐字（82 号）、resumeFrom 增量与 importMode=series（85 号）
- 散件：单章写作中途截断、/agent model 校验、resumeFrom REST、fetchWithProxy、attachments 归一化、模型四层解析

## 六、验证基线（2026-08-14）

- `cargo test --lib`：**1112**（+2：sub_agent_tool 单测）
- `cargo test --test golden_leaf`：76
- `cargo test --test e2e_write_next_contract`：**148**（+2：sub87_e2e）
- `cargo test --features export-bindings --lib`：**1271**（+2）
- `cargo clippy --lib --tests --bins`：零警告
- TS：`packages/core` vitest 185 文件 / **1798** 测试全过

## 七、影响面与下一步

- 影响面：`agent_production::execute_create_book` / `audit_route::run_audit_flow` / `ToolOutcome` 与 `AuditFlowError` 提 pub(crate)（无逻辑改动）；聊天工具面达 11 件（+sub_agent，书会话注册）
- 下一步（88 号候选）：**首选 reviser + architect.revise 接入**（revise/rewrite 链对齐 TS reviseDraft 参数面——五模式 + revision gate 诊断文本）；其次书会话余下工具族（generate_cover 壳 + 写编辑工具族）；或既有备案落地（PDF 抽取/zh 提示词逐字/resumeFrom 增量）
