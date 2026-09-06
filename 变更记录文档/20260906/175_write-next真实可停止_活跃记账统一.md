# 175 · W-C5 后续：write-next 真实可停止（Rust 引擎）+ 活跃记账统一

- 日期：2026-09-06
- 模块：engine-rs（server/write_next_route.rs / agent_production.rs、bin/inkos-engine-server.rs、tests/e2e_write_next_contract.rs / strangler_duel.rs）、总体方案文档
- 类型：feat（引擎侧能力；TS 回退端不动）
- 关联：174 号（检查点 + 独立活跃集合的由来）、总体方案 W-C5「待办登记」项

## 一、背景

174 号把 write-next 的活跃记账做成**独立集合**，理由是「管线不消费 abort 句柄——混入确认任务表会造成假停止」。但 write-next 管线的 abort 机制（`WriteNextConfig.abort` + 阶段边界 `check_aborted` + 取消快照路径）在 136 号起就已存在并有单测（`abort_before_entry` / `abort_during_draft_stops_at_safe_point`）——**缺的只是从路由到 config 的最后一段接线**。接上这段，独立集合的存在理由即消失：句柄被真实消费后，write-next 可以（且应该）注册进确认任务句柄表，stop 端点、重启对账恢复单一活跃来源。

## 二、改动

1. **`WriteNextRunner` 签名第五参 `AbortHandle`**（engine-rs 内部装配类型，非 HTTP 契约）：路由每次任务创建句柄并传入；bin 闭包 `config.abort = Some(abort)` 注入管线。
2. **注册表统一**：write-next（sessionId 给定时）注册进 `active_confirmed_tasks`（执行 id → 句柄），174 号的 `active_write_next_tasks` 集合删除；`load_reconciled_task_snapshot` 活跃性判定回归单表。
3. **stop 语义**：`POST /api/v1/sessions/:id/abort`（scope=all）经 `find_running_task_controller` 命中 write-next 句柄 → 置位 → 管线在下一阶段边界停止 → runner 返回「Operation aborted…」→ 既有 error 路径广播 `write:error` + 落 error 快照 + 注销。
4. **测试**：`e2e_write_next_contract.rs` / `strangler_duel.rs` 的 runner 闭包与 `E2eRunner` 别名同步签名；新增路由级中止测试（真实 `find_running_task_controller` 查找 + 置位 + 检查点退出 + 终态/注销断言）。

### 语义边界（如实登记）

- **UI 仍不传 sessionId**（Dashboard/BookDetail 现状）——可停止能力随检查点同条件激活（`sessionId` 给定时）。引擎契约已完备；sessionId 接线 + 页面停止按钮属产品批次。
- **TS 回退端不动**：Node `writeNextChapter` 管线无信号参数，无法诚实停止——TS 侧保持独立集合 + `aborted:false` 语义。abort 契约在 write-next 会话上双端分歧（Rust `aborted:true` 实停 vs TS `aborted:false`），反映真实能力差，不属 duel 断言面。
- 「每会话一个生产任务」409 守卫（`find_active_running_task`）经确认表自然覆盖 write-next：同会话在写期间再确认生产任务会被拒——与 studio 单任务名额规则一致。

## 三、验证

| 项 | 结果 |
| --- | --- |
| `engine-rs cargo test` 全量 | **全绿**：lib 1232（净增中止端到端 1）+ 集成 194 + 70 + 8，0 失败 |
| `engine-rs cargo clippy --all-targets -- -D warnings` | 零告警 |
| `src-tauri cargo test` | 全绿（466 + 集成套件，0 失败；公共类型签名变化无外溢） |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **8/8 真跑全绿**（含 bin 进程真 runner 闭包执行——验证 config.abort 注入路径在真 bin 上运行） |
| 中止单测（`stop_handle_registered_and_setting_it_stops_runner`） | 真实 stop 查找路径命中 → 置位 → runner 检查点退出 → write:error 含「Operation aborted」→ error 快照 → 注册表释放 |
| TS vitest | 本批未动 TS，不重跑（174 号基线 739/739） |

## 四、过程教训

1. **「注册表语义 = 存活证明 + 可中止句柄」的合一前提是句柄被消费**——174 号为此拆表、175 号接线后合表；拆合的依据都是句柄的真实消费性，不是表数量偏好。
2. 装配缺口审查：管线能力（abort）与路由能力（stop）各自存在、中间断裂——接线类工作要顺着「能力已存在，查最后一段」的方向找，成本远低于新增机制。

## 五、遗留

- write-next 的 sessionId UI 接线（Dashboard/BookDetail）+ 页面停止按钮——随 W-C3/C4 产品批次。
- 总体方案剩余 backlog：W-A4b（secrets 掩码，需 UI 确认）、W-B4/B5（插件路由/WIT batch）、W-C3/C4（产品级）、W-D4（bench——plugin_execute 对象在本仓不存在，落地前需先重新选型真实热路径）。
- TROUBLESHOOTING.md 全面复审、en/ja README 未译（既有遗留）。
- 推送须在 Fork 图形端执行（既有约定）。
