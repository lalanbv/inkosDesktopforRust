# 804 号：hook 合并语义 rust 对齐——incoming 优先与 promises H02/H03 notes 收敛

日期：2026-11-02。前置：803 号登记（diff 首叉 /timeline/2/summary 单字段差=H02 notes 模板 vs 台账覆盖语义，交接定性「纯 mock 数据级修复」）。

## 一、根因终审（移植漂移实锤——推翻「mock 数据」定性）

对照双实现后根因改写为**引擎级合并语义分歧**：

| 字段 | rust `merge_hook_record`（reducer.rs:178，改前） | TS `mergeHookRecord`（state-reducer.ts:129） |
|---|---|---|
| notes / expectedPayoff / type | `prefer_richer_text`：更长者胜、**等长取既有** | `incoming.X.trim() \|\| existing.X`：incoming 优先、空白回落 |
| status | `merge_hook_status`：resolved 粘滞 + 进展/任一 progressing → progressing + 否则既有 | `existing resolved ? resolved : incoming.status` |

git 考古定方向：
- `b3694e8c`（2026-08-11 移植 state-reducer）当时 TS 正是 `preferRicherText` → rust 为**忠实移植**；
- `7aed0d82`（2026-08-30 harness v2 迁移）TS 改为 incoming 优先并删除 richer 概念（TS src 今无 "richer"）；
- rust 从未跟进 = **移植漂移**；394 号同函数 `kind` 字段先例即 rust→TS 对齐。

分歧触发形态（夹具数据）：H02 模板 notes「与镜灵的婚约誓言待启。」与台账「苏檀与镜灵的婚约誓言。」**等长（11 字）**→ richer 取既有台账、TS 取 incoming 模板 → 单字段差。H03 模板（9 字）比台账（13 字）短 → rust richer 取台账——被 H02「首叉」遮蔽的**潜在第二分歧**。H01 模板更长（11 vs 10）故 803 已一致。

**为什么不走 mock 数据对齐**：输入相同则输出必同——把差分器变成回声室，掩盖真实 LLM 输出（必与台账不同）下的引擎语义分歧，与 diff 的存在意义相悖。

## 二、修复（rust→TS，仅共享契约面）

`engine-rs/src/state/reducer.rs`：
- 三文本字段 → `prefer_incoming_text`（incoming 非空白取 trim 形态，否则**原样**保留既有——TS `||` 逐字同款）；
- status → resolved 粘滞否则 incoming 直赢（进展不再自动升级 progressing）；
- 移除 `merge_hook_status` 与 reducer 内 `prefer_richer_text`（改后死代码，clippy `-D warnings` 会拦；`hook_arbiter.rs:452` 自有副本保留）。

语义依据与边界：
- upsert = LLM 对该钩子的**权威最新陈述**，notes 须可被纠正/缩短（richer 使 notes 单调不缩=upsert 语义缺陷，线性可控性受损）；
- 同族吸收路径（新 id 命中既有族，reducer.rs:137）同为 upsert 言语行为 → 共用本函数；
- 候选提案（newHookCandidates）为弱言语行为 → `hook_arbiter::merge_candidate_into_existing_hook` 的 richer 调和语义**保留**（TS v2 已无该函数=rust 独有增强，695 已登记 TS 缺口）；
- rust 独有产品字段（kind/payoff_timing/core_hook/half_life_chapters/advanced_count/promoted）语义不动。

单测：改 `upsert_merges_existing_hook_by_id`；新增 `merge_text_fields_prefer_incoming_even_when_shorter_or_equal_length`（H02 等长复现）、`merge_text_fields_fall_back_to_existing_when_incoming_blank`、`merge_status_follows_incoming_unless_existing_resolved`（三条旧语义反例：existing progressing+incoming open→open、open+deferred→deferred、进展不升级）；删除两个 helper 测试。净 +1（1956→1957）。

## 三、活体验证（字段级）

- diff 报告模式：共享面 **71 对比 / 5 分歧**（803 时代 71/7）；**promises ✓（200）**。余 5 条=quality-trend/currentChapter、roster-candidates/candidates、eval/avgParagraphWarnings、detect/1 404vs200、chapters/1/reject 404vs200——全部为已登记 695 移植族/级联项。
- **取证方法修正**：`--keep` 终态根是尾批 approve/reject 探针**之后**（node 级联清空回基线 lastApplied 0；rust 回退 ch1）——不能作 promises 比对时刻证据。改用 `--probe '/api/v1/books/$BOOK/promises' --keep` 早退（变更探针前）保留写链完成现场：双腿 lastApplied 2 / summaryRows [1,2]；**H01-H04 共享六字段差异 0**——H01「镜中世界来历初现端倪。」/H02「与镜灵的婚约誓言待启。」/H03「碎镜夜半发光之谜。」（均模板）、H04「镜宗与皇室的隐秘盟约。」（基线）；status/la 全同。
- mock、TS **零改动**——唯一变量 rust reducer → 收敛归因唯一。

## 四、验证

cargo:testgate engine-rs 47 目标 **1957** + src-tauri 25 目标 584 + RustSec 双 0；clippy 双 0；gate:ts 八步全绿（含 engine-contract-diff + legacy 11/11）；权威 pnpm -r test core 176/979 + studio 134/717 + cli 23/81。

## 五、登记（695 工项余项）

- **rust `HookStatus` 无 `Superseded` 变体**（models/runtime_state.rs:30：Progressing/Deferred/Resolved/Open-other）：TS 正典含 superseded 撤回生命周期——`mergeHookRecord` 中 existing superseded 终态不可变；incoming superseded → resolved 历史不可撤（HOOK_RESOLVED_HISTORY）/ 撤回须理由（HOOK_WITHDRAWAL_REASON_REQUIRED）/ notes 追加。rust 反序列化 "superseded" 落 Open(other)。独立轮：枚举变体+serde+merge 两分支+StateReducerError 两变体+穷举匹配面（约 18 处 `HookStatus::Deferred` 引用为规模代理）。
- reject 回滚级联 / quality-trend / roster / detect / eval 维持登记。

## 六、教训

1. **交接的「数据级」定性要在双实现对照后复核**：对照即推翻——对齐数据会把差分器变成回声室，修语义才能一次清掉首叉与被遮蔽的次叉（H02+H03）。
2. **git 考古定收敛方向**：`git show <移植提交>:<TS 路径>` 看移植当时的 TS 形态——忠实移植+上游后改=漂移（rust→TS）；移植时即不同=刻意增强（需产品判断）。比凭「谁是产品引擎」拍方向可靠。
3. **--keep 根的时刻语义**：尾批变更探针会改写状态；比对时刻取证用 `--probe` 早退 + `--keep`。
4. **言语行为决定合并语义**：upsert（权威陈述）与 candidate（弱提案）该用不同合并策略——reducer incoming 优先、arbiter 吸收 richer 调和，各安其位。
