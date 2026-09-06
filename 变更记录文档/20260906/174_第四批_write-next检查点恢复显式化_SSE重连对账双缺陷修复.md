# 174 · 第四批：W-C5 write-next 检查点恢复显式化 + SSE 重连对账双缺陷修复

- 日期：2026-09-06
- 模块：engine-rs（server/write_next_route.rs / agent_production.rs / sse.rs / mod.rs）、packages/studio（server.ts / server.test.ts）、总体方案文档
- 类型：feat + fix（双端同步）
- 关联：172 号（对账/停机）、173 号（duel 真跑门禁）、[重构优化总体方案_v1](../../开发时SpecCoding'sPlan/inkosDesktop/06_全局重构优化/重构优化总体方案_v1.md)（W-C5 → 已落地标注同步）

## 一、背景

第四批拣选总体方案剩余 backlog：**W-C5 检查点恢复显式化**（对标 LangGraph durable execution；task_store 雏形 39 号已存在）。实施前取证发现一个休眠特性与两处被掩盖的存量缺陷：

1. **write-next 检查点休眠**：Rust 侧快照只在终态落、且依赖 body `sessionId`——而 UI 调用（Dashboard/BookDetail）从不传 sessionId，任务运行中崩溃/重启无任何持久痕迹。Node 侧更是零快照。
2. **SSE 重连复活死任务卡（parity bug）**：Rust `events_handler` 用**原始** `load_studio_task_snapshot`，而 TS（server.ts L3475）用 `loadReconciledTaskSnapshot` 对账读取——引擎重启遗留的 running 快照会在重连时以 running 复活，UI 出现永远转圈、停止按钮无效的死任务卡。
3. **SSE 快照恢复是生产链路死代码**：Rust 处理器要求 query 同时带 `sessionId` 与 `projectRoot`，但前端 EventSource 只传 `sessionId`（use-sse.ts / chat action.ts）——**快照恢复在 Rust 引擎上从未真正生效过**。由真实形态的冒烟脚本曝光（单测传了 projectRoot 所以从未命中）。

## 二、改动

### write-next 会话检查点（双端同步）

- **启动留痕**：`sessionId` 给定时，spawn 闭包先注册活跃记账，再落 `Running` 快照（`id=write-next-{bookId}`、`requestedIntent=write_next`、`label=撰写下一章（{bookId}）`、`args={wordCount?,temperature?}`）；终态（completed/error）复用同一 `startedAt` 落终态快照，落盘后才释放记账（注册先于首次持久化、晚于终态持久化——TS 确认任务的窗口语义）。`persist_task_snapshot` 重构为 `WriteNextCheckpoint`（一次任务的 Running/终态共用执行 id 与 startedAt）。
- **活跃记账用独立集合**：engine-rs `active_write_next_tasks()`（`Mutex<HashSet<String>>`）、TS `activeWriteNextTasks`（`Set<string>`）——**不混入确认任务句柄表**。write-next 管线不消费 abort 信号，若混入 `activeConfirmedTasks`，abort 端点会找到控制器、宣称 `aborted:true` 而管线实际停不下来（假停止）；独立集合让「停止」保持诚实的「无任务可停」。
- **重启对账纳入第二存活来源**：`load_reconciled_task_snapshot`（Rust）/ `loadReconciledTaskSnapshot`（TS）活跃性判定由单表改双表（确认任务句柄表 ∥ write-next 活跃集合）——存活任务不被误杀。
- **对账文案双语**：Rust 对账 error 消息原硬编码中文，改按 `current_project_language` 分支（对齐 TS `pick`）。
- **Node write-next 端点对齐**：body 增收 `sessionId`；同一检查点生命周期（含 `deletedSessionIds` 守卫）。HTTP 响应契约不变（`{status:"writing", bookId}` 即刻返回）。

### SSE 重连对账两缺陷修复（engine-rs）

- `events_handler` 快照补发改用 `load_reconciled_task_snapshot`（TS 同款语义）。
- **`EventsState` 注入项目根**：新增 `sse::EventsState { hub, project_root }`，`router_with_runtime` 装配时从 write-next 运行时取同一项目根（检查点落盘面与恢复面同源）；handler 的 query `projectRoot`（历史形态）保留为覆盖。前端零改动即恢复生效。

## 三、验证

| 项 | 结果 |
| --- | --- |
| `engine-rs cargo test` 全量 | **全绿**：lib 1231（基线 1227 + 检查点生命周期/零副作用/对账活杀/SSE 对账快照净增 4）+ 集成 194 + 70 + 8，0 失败 |
| `engine-rs cargo clippy --all-targets -- -D warnings` | 零告警 |
| `src-tauri cargo test` | 全绿（466 + 集成套件，0 失败） |
| `packages/studio` vitest 全量 | **739/739 绿**（含新增 3 用例：检查点生命周期含在途不误杀、无 sessionId 零副作用、SSE 重连对账） |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **8/8 真跑全绿**（EventsState 变更后复跑） |
| 真实 bin 冒烟①（检查点） | 带 sessionId POST → Running 快照落盘（args/label/startedAt 正确）→ mock LLM 失败 → error 终态（同 startedAt、completedAt、错误消息完整） |
| 真实 bin 冒烟②（重启对账） | 挂起 LLM 使任务在途 → SIGKILL bin（快照留 running）→ 重启 → SSE `?sessionId=` 重连 → 收到 `task:snapshot` 对账终态（`status:error` +「任务已中断：Studio 服务在任务运行期间重启…请重新发起」+ completedAt） |

## 四、过程教训与登记

1. **单测参数会掩盖装配层死代码**：SSE 快照恢复的单测一直自传 `projectRoot` query——「测试传参 ≠ 生产传参」，前端只传 sessionId 的事实让该分支在 Rust 引擎上从未生效。冒烟脚本必须用**生产形态**的请求（真实 EventSource 参数集）才能曝光这类缺陷。
2. **abort 语义诚实性**：注册表语义是「存活证明」+「可中止句柄」两种能力，混用前先问句柄是否真被消费——不可取消的管线放进句柄表会制造「假停止」（报告成功实际继续跑）。
3. **write-next 可停止性**（登记待办）：真实可停止需 abort 接入管线（`WriteNextRunner` 签名扩展 + 两侧 UI 停止按钮语义对齐），超出本批范围。

## 五、遗留

- 总体方案剩余 backlog：W-A4b（secrets 掩码，需 UI 确认）、W-B4/B5（插件路由 / WIT batch，待插件生态起量）、W-C3/C4（系列书导入 / 时间线视图，产品级）、W-D4（bench 常态化门禁——benches 尚未建立）、write-next 可停止性（本批登记）。
- TROUBLESHOOTING.md 全面复审、en/ja README 未译（既有遗留）。
- UI 侧 write-next 调用（Dashboard/BookDetail）尚未传 sessionId——检查点能力已就绪，接线随 W-C3/C4 产品批次一并考虑。
- 推送须在 Fork 图形端执行（本机 CLI 无 GitHub 凭据，既有约定）。
