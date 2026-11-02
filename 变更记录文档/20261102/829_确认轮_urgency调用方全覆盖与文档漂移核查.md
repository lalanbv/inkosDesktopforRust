# 829 号：确认轮——urgency 调用方全覆盖 + 上游零增量维持

日期：2026-11-02。前置：826/827/828 确认轮（全仓卫生终检）。

## 一、巡检快检

- **上游**：`fetch --prune` 复跑仍 **0**。
- **serde_json**：engine-rs 锁仍 **1.0.151**（crates.io 最新 = 1.0.154 回退线，825 定案维持）。
- **RustSec/audit/bench**：820/824/826/827 终态维持（无相关变更）。

## 二、resolvePromiseUrgency 调用方全覆盖（防御纵深核查）

`resolvePromiseUrgency`（promise-ledger.ts）的全部调用方按包核对：

| 调用方 | 终态过滤 | 状态 |
|---|---|---|
| studio `server.ts:4546` quality-trend | `TERMINAL_HOOK_STATUS` 含 superseded 组（808/814） | ✓ |
| core `promise-ledger.ts` `buildPromiseTimeline` | `isWithdrawn` 分支先行（821） | ✓ |
| core `promise-ledger.ts` `detectPacingDebts` | `isWithdrawn` 跳过（821） | ✓ |
| engine-rs `ops_routes::get_quality_trend` | `!is_withdrawn(...)` matches!（808 同步） | ✓ |
| engine-rs `promise_ledger.rs` `build_promise_timeline` | `is_withdrawn` 分支先行（821 镜像） | ✓ |

**全部 5 个调用站均有终态过滤**——`resolvePromiseUrgency` 的 `isFulfilled` 内部检查（不含 superseded）无暴露面，防御纵深完整。

## 三、docs/DESKTOP_WALKTHROUGH.md 漂移核查

grep 全文（quality-trend/resync/superseded/overallScore/审改协议）→ **0 命中**——桌面走查文档不涉及 804-825 触及的语义面，无漂移。

## 四、结论

零新工项。撤回生命周期的紧迫度消费站全覆盖实证闭合。695 共享面活体 71/1 唯一余项=数值分协议扩展（上游 2.0 审改协议无数值分），维持等待出口。

## 五、教训

1. **防御纵深的完整性质证要列全调用方表格**：不是"应该没问题"而是"逐个列出来、逐个标注过滤状态"——表格化让覆盖声明的边界一目了然。
2. 文档漂移核查用**语义面术语**（quality-trend/resync/superseded）而非**实现术语**（函数名/模块名）grep——前者能发现概念级过时，后者只能发现代码级过时。
