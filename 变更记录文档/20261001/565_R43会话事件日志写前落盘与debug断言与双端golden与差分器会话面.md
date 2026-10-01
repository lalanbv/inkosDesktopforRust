# 565 号：R43 会话事件日志写前落盘+「模型可见即已记录」debug 断言+derive/restore 双端 golden+差分器会话面

- 日期：2026-10-01
- 类型：feat(engine+core+scripts)
- 路线：dsh 对标 v7 P1 末项 R43（541 号立项、544 号 §5 施工图）；M2 收官
- 状态：✅ 门禁全绿收口

## 考古推翻设计前提（本专项最大发现）

544 号 §5 写「JSONL 落盘 `{project}/.inkos/sessions/{sessionId}.jsonl`（**新增文件**，零迁移）」——**前提失实**：该路径早被 transcript 持久层占用（Rust 64/68 号已移植、TS 更早，7 事件词汇=6 原始+R32a compaction，双端恢复投影函数族齐备，读侧归一已成立：loop 输入历史本就从 `restore_agent_messages_from_transcript` 重建）。

**裁决**：R43 真实增量=四件，事件词汇采纳现 transcript 语义而非 dsh 字面九事件重命名（活体双端契约迁移零收益高风险，备案偏差——dsh 九事件语义被现词汇覆盖：user/assistant/toolResult=message 事件三角色、tool_call 折叠于 assistant content 块、session_title=session_metadata_updated、turn_start/end≈request_started/committed、step 级事件折叠于消息序）。

## 交付面

### ① 写入时序拆批（「日志即真相」在飞行窗口成立）

- 217 号批写（整轮事后一次 append）拆为三段：`begin_chat_turn`（request_started+user **先于首个 LLM 请求**落盘，返回 request_id）→ `commit_chat_turn`（工具对+assistant+request_committed，同轮 request_id，217 号消息形态逐字节保持，parent 链锚点改为从日志扫本轮 user uuid）→ `fail_chat_turn`（只补 request_failed 终态标记）。
- 崩溃窗口语义：进程中途崩溃时用户输入仍在日志（审计面）；derive 对未提交轮整体排除的双端语义不变（217 号锁定），中断尾不污染派生。
- 备案：工具对仍随 commit 批落盘（非循环内逐对）——未提交尾 derive 排除使中断窗口派生结果与逐对落盘一致；逐请求强断言不进 loop 内部。

### ② 「模型可见即已记录」debug 断言

- `debug_assert_model_surface_is_logged`：请求发出前断言 ①本轮 request_started+user 已在日志 ②日志重建面（committed 恢复管线）== loop 输入（`Vec<LLMMessage>` PartialEq 逐条）。
- 仅 debug 编译档生效（`cfg!` 常量折叠，release 零工作）；断言失败即 panic（cargo:testgate 全目标门禁内当场红）。chat e2e 24 件活体路由流全过=断言管线实证。

### ③ derive/restore 双端 golden 三方锁定

- `packages/core/src/__tests__/golden/session-transcript-derive-vectors.json` 8 case：committed 基线、**中断尾部修复**（撕裂尾剔除且后续轮 seq 连续不受影响）、失败轮排除、kind 过滤同/异、工具轮折叠卡、legacy 无 kind 轮 user 保留、空文本 assistant。
- 消费：Rust `tests/golden_session_transcript_derive.rs`（wire JSON 逐行落盘→双端解析器全收断言→committed uuid 序/scan 对话面/derive 角色序/工具卡数）+ TS `session-transcript-derive.test.ts` 同构，首跑即绿。
- **两处面间不对称语义发现**（golden 落定前实测纠正）：kind 过滤只作用于模型面（committedMessageEvents(kind)/scan），**display derive 不传 kind 展示面全量**（TS 956 行同构）；空文本 assistant scan（模型面）丢弃而 display derive 保留（217 号「空文本丢弃」实为 TS 写入时序——失败轮根本不写 assistant——非 derive 过滤）。工具卡折叠挂最终文本 assistant 非「工具卡消息→文本消息」两独立消息（217 号注释措辞纠偏）。

### ④ 差分器会话面维度（544 号「双腿同脚本跑聊天」落地）

- `engine-contract-diff.mjs` 新增两检查：双腿各 POST chat 会话+驱动一轮 agent（mock 聊天 system 提示词含「同人」关键词走 CANON 纯文本分支，无工具调用，确定性）→ 读双腿 `{root}/.inkos/sessions/{id}.jsonl` 逐行取 type 比对请求族事件计数（session_created/request_started/message/request_committed/request_failed）→ GET `/sessions/:id` derive 读面深比对（sessionId/id/uuid+VOLATILE 剪除后逐字节）。
- session_metadata_updated 豁免备案：标题生成 prompt 双端未 golden 锁定，mock 分支可能分歧。
- 首跑即绿：请求族 5 事件逐类型计数相同+derive 读面一致，全脚本 44 端点 0 分歧。

## RunLog/ContextLens 归一裁决（v7 R43 意图第三件，备案不做）

v7 表述「RunLog/ContextLens/回放 fork 从同一日志派生（三处重复状态归一）」——考古后裁决**不强行归一**：RunLog（403 号）是调用级遥测环形缓冲（per-LLM-call 元数据含重试/接管，会话日志无此粒度，跨会话聚合强读文件语义倒退）；ContextLens 是上下文装配观测面（非会话状态）。真正存在的状态重复=内存 BookSession.messages vs transcript，已由读侧归一（loop 输入从日志重建）+写前落盘+debug 断言三件锁死：**日志=真相、内存=缓存、断言=锁定**。

## 教训

- 施工图「新增文件」类前提必须当轮考古验证（544 号写于 09-22，transcript 层在其前早已存在——与 529/530 号「立案前提当轮验证」同源教训第三例）。
- Rustdoc 文档注释行首禁 `+`（CommonMark 列表符号，clippy doc_list_item_without_indentation 连坐后续行）。
- golden 断言先写「实测语义」再写「应当语义」：两处面间不对称都是向量首跑红后考古出的真实双端行为，凭注释/直觉写预期必错。
- TDAgentMessage 联合类型消费面用窄化访问（AgentMessage 含无 content 成员，TS2339）。

## 门禁

- clippy:gate 双 crate 0 告警
- cargo:testgate：engine-rs 44 目标 1888 passed（+1 golden 目标）+ src-tauri 23 目标 576 passed
- gate:ts 七步全绿（typecheck/test/audit/build/node-fallback-smoke/engine-contract-diff 44 端点 0 分歧/export-epub-smoke）
- TS golden 消费 9/9 首跑即绿；agent_route 单测 14+session_restore 11+chat e2e 24 全绿

## 关联

- 前序：553（R32 压缩）、557（R42 spill）、563/564（R41 计量）
- 路线：v7 P1（R41/R42/R43）至此收官；剩 P2 R44–R46 按需独立立项
- 下一号自 566 起
