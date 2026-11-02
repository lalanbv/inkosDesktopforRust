# 821 号：promises 时间线撤回态可见化——withdrawn 投影双栈延伸 + 节奏债排除

日期：2026-11-02。前置：814 号收尾核查（studio 其余伏笔状态消费点沿同口径核）。

## 一、巡检发现（814 的同类缺口在时间线面）

- `PromiseTimelineCard` 的 `state` 枚举（open/advancing/fulfilled/overdue）无撤回态——已撤回前提按 expected/currentChapter 投影为 **advancing 或已逾期**（撤回的承诺显示"推进中/已逾期"=误导）。
- `detectPacingDebts`（双侧）只排除 fulfilled——已撤回的**核心**钩子会计入节奏债（"核心承诺已搁置 N 章"——撤回的承诺无回收义务）。
- 与 806 检索排除（817 定案 is_searchable_terminal_status）同族：805 撤回生命周期的时间线/债务延伸。

## 二、修复（双侧同口径）

- **core `promise-ledger.ts`**：`WITHDRAWN_PATTERN`（superseded/withdrawn/已撤回/撤回，与 hook-lifecycle 别名表同集）+ `isWithdrawn`；`buildPromiseTimeline` 撤回优先分支 → `state: "withdrawn"`；`PromiseTimelineEntry["state"]` 联合增 `"withdrawn"`；`stateRank.withdrawn = 4`（历史信息排最末不参与紧迫度）；`detectPacingDebts` 跳过撤回。
- **rust `promise_ledger.rs` 镜像**：`is_withdrawn` + timeline 分支/rank `"withdrawn" => 4` + `detect_pacing_debts` 过滤。
- **studio `PromiseTimelineCard.tsx`**：本地 state 联合同步；`STATE_STYLE.withdrawn`（弱化+删除线，与 814 徽标同风格）+ `STATE_LABEL`（已撤回/Withdrawn）。

## 三、G10 共享向量族扩展（唯一事实源双侧消费）

`promise-ledger-vectors.json`：契约 `timelineStates` 增 `"withdrawn"`；新增 timeline 向量 `superseded-projects-withdrawn-818`（open+superseded 双钩子 → advancing+withdrawn，撤回排最末）；新增 pacing 向量 `superseded-core-hook-no-debt-818`（同章 open 与 superseded 双核心钩子 → 仅 open 告警）。rust `promise_ledger_contract()` 同步加 `"withdrawn"`。golden-promise-ledger 契约冻结测试随向量更新。

## 四、验证

双侧 G10 全绿（TS 5/5；rust golden_promise_ledger_diff 6/6——共享向量族驱动）；typecheck 双侧 0；cargo:testgate 47 目标 1980 + src-tauri 584 + RustSec 双 0；clippy 双 0；gate:ts 八步全绿；权威 pnpm -r test core 180/988 + studio 137/725 + cli 23/81；diff 71/1 维持（**promises ✓：H03 双腿投影 withdrawn**——撤回态端到端收敛）。

## 五、805→821 撤回生命周期全景（终局）

settle 三守卫 → 状态落盘 → 投影转义 → 台账回读还原 → UI 徽标（814）→ **时间线投影 withdrawn + 节奏债排除 + 卡片徽标（821）**；检索排除（806）与比较 urgency 排除（808）已在位。撤回的前提在引擎、投影、检索、UI 四层一致呈现为「已撤回」。

## 六、登记

- 数值分协议扩展（上游面）维持唯一实质余项。
- 呈现差异备忘：rust comparison.md 多意图匹配列（818 定性维持，只写件）。

## 七、教训

1. **状态枚举的可见化要问"还有谁在读"**：814 补了侧栏徽标，本轮发现时间线与节奏债仍是旧口径——枚举值的每个消费站（timeline/pacing/检索/urgency/比较）都要按同一终态语义过一遍；promise-ledger 内聚后消费站收敛，扩展成本一个分支。
2. **G10 共享向量族是双栈语义变更的最佳护栏**：向量加一组用例，双侧测试自动驱动（TS 5 断言/rust 6 断言同源），无需双侧各写实现级测试。
