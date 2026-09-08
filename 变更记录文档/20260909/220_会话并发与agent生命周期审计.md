# 220 号：会话并发/agent 生命周期面审计——串行队列 + abort 链 + 删除守卫

- 日期：2026-09-09
- 分支：develop
- 关联：64/68 号（abort 注册表）、65 号（聊天执行体）、102 号（abort 句柄）、217 号（轮收尾持久化）
- 推送核验：origin/develop 仍停 6bc65564——**201–219 十九提交均未推送**；本批次后本地领先 20。两项默认值无新答复。

## 一、审计范围与结论

| 审计点 | 结论 |
|---|---|
| 同 session 并发聊天 | **缺口（已修）**：Rust 无条件覆盖注册表句柄——abort 链丢失 + 两轮 LLM 并发交错；TS 为 per-session FIFO 排队（runInAgentSessionQueue） |
| abort flag 跨轮复用 | **缺口（已修）**：排队轮拿锁后创建全新 flag（前轮中止不波及新轮），含单测 |
| 会话删除 vs 运行中聊天 | **缺口（已修）**：轮收尾追加无删除守卫——已删会话被 transcript「还魂」（TS appendSessionMessagesUnlessDeleted 对应面） |
| agent_loop 轮数上界 | ✓ MAX_ROUNDS=12 有界 |
| sub_agent 递归 | ✓ 双端一致：固定分支派发（architect/writer/auditor/reviser/exporter 专用链），子代理无工具循环不可再派生 |
| 注册表生命周期 | ✓ remove 在 match 前覆盖所有 Ok/Err/abort 退出路径；aborted 提前 return 在 remove 之后 |
| abort 端点死锁风险 | ✓ 新队列设计：abort 只碰 abort_flag（std Mutex 同步置位），不碰 queue（tokio owned guard），无死锁 |

## 二、修复实现（agent_route.rs）

1. **per-session 串行队列**：`AgentSessionHandle` 增 `queue: Arc<tokio::sync::Mutex<()>>`——聊天轮整轮持有 owned guard（RAII），并发同会话请求排队等待；`entry().or_insert_with()` 复用既有条目（不再覆盖丢失 abort 链）；拿到队列锁后**刷新注册表句柄 + 全新 abort flag** 并重插条目（前轮结束 remove 后本轮 abort 仍可达；前轮置位的中止不波及新轮）。
2. **删除守卫**：`append_chat_turn` / `append_failed_chat_turn` 开头检查 `deleted_session_ids()`——轮进行中会话被删时跳过追加（同名重建清除标记后恢复），防「还魂」。
3. `std` guard 不跨 await（先 clone Arc 再 `lock_owned()`，保证 handler future Send）。

## 三、验证

- engine：lib **1309**（+3：并发串行序断言〔first-start→first-end→second-start〕、排队轮全新 flag、删除守卫含重建恢复）、集成 **196**（68 号 abort e2e 补 queue 字段后照常绿）、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**。
- TS 零改动（core 1917 / studio 793 于 218/219 号已绿）。

## 四、遗留

1. 排队请求无进度反馈（TS 同构：请求挂起直至前轮完成）——若需「排队中」即时反馈需 SSE 面扩展，暂无需求信号。
2. 生产任务（confirm 面）已有单任务闸门（reserved_production_sessions 409）与聊天排队语义不同——TS 同构，不改。
