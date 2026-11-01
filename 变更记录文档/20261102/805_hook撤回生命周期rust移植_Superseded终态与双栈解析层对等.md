# 805 号：hook 撤回生命周期 rust 移植——Superseded 终态与双栈解析层对等

日期：2026-11-02。前置：804 号登记（rust `HookStatus` 无 Superseded 变体，TS 撤回生命周期 HOOK_RESOLVED_HISTORY / HOOK_WITHDRAWAL_REASON_REQUIRED 缺移植，"superseded" 反序列化落 Open(other)）。

## 一、语义面摸底（改前）

TS 侧 hook 撤回语义共 6 点：`HookStatusSchema` 含 superseded；reducer resolve/defer 对 superseded 抛 HOOK_SUPERSEDED；`mergeHookRecord` 两分支（existing superseded 终态原样返回；incoming superseded → resolved 历史不可撤 / 须依据 notes / notes 追加）；`memory-retrieval` searchableHooks 排除 resolved+superseded；settler 工具字面量；流测试 `superseded-hooks.flow.test.ts`。

**TS 自身的漏**：`HOOK_STATUS_ALIASES`（hook-lifecycle.ts）不含 superseded → `normalizeStoredHookStatus("superseded")` 回落 "open" → md 投影往返把已撤回前提**复活为 open**（state JSON 经 zod 严格枚举保真，损失点只在 md 字符串解析层）。`filterActiveHooks` 也未排除 superseded。

rust 侧：枚举无变体（"superseded" 落 Open(other)、status_raw 保原文）；4 处穷举 match（编译器强制）；3 处 md→状态解析（`hook_lifecycle::normalize_stored_hook_status` / `story_markdown::parse_hook_status` / `state_bootstrap::normalize_hook_status`——不补则**静默**落 Open，编译器不报）；6 处以 Resolved 作终态判断的过滤点。

## 二、rust 移植（13 文件）

- `models/runtime_state.rs`：`HookStatus::Superseded`（serde rename + ts 导出串 + 终态语义文档）。
- `state/reducer.rs`：`StateReducerError` 增 `HookSuperseded{op,hook_id}` / `HookResolvedHistory` / `HookWithdrawalReasonRequired`（TS 三 code 逐一对偶）；`merge_hook_record → Result` 前置两分支逐字移植（notes = 既有 + `\n` + 依据 trim；status_raw 清空；其余字段 `..existing.clone()` 保留原内容供历史查证）；resolve/defer 对 superseded 拒绝；准入 `active` 排除 Superseded（否则同族新提案被吸收进死记录而静默消失）。
- **顺路漂移**：rust defer `max(last_advanced, delta.chapter)` 推进 → TS 2.0（7e60e13f）改为**不推进**（「延后不是叙事推进的证据」；移植时 b3694e8c TS 确实推进→同款移植漂移）→ 对齐。
- 穷举臂 4 处（hook_lifecycle canonical / hook_health / projections / hook_governance）→ "superseded"。
- 解析器 3 处识别 `superseded|withdrawn|已撤回|撤回`（与 TS 别名表新增组逐字同集）。
- 终态同列 6 处：`filter_active_hooks`、`memory_retrieval::recycle_terminal_re`（检索排除）、`hook_health` 活跃集、`hook_governance` 过滤、`hook_stale_detection::is_resolved`（已关闭记录不计 stale/blocked）、`hook_arbiter` 候选准入 active 排除 + 吸收合并终态保持（防止已撤回记录被复活为 progressing）。
- `runtime_state_store.rs`：StoredHook.status 由 `format!("{:?}").to_lowercase()` 改走 `hook_status_canonical`（R26/409 多词变体陷阱同型，枚举扩展不再依赖"恰好单词"）。
- 其余 HookStatus 引用（planner_context/writer/architect/governed_working_set/hook_promotion/projections 构造处）均为 `status: Open` 构造器，无逻辑。

单测 +10：reducer 6（撤回归档+notes 追加+原内容保留 / 终态不可复活 / 拒撤 resolved+须依据 / resolve·defer 拒绝 / defer 保持 la / 已撤回不吸收同族）；lifecycle 2（别名 + serde↔canonical↔词表三向往返 + filter 排除）；bootstrap 1；retrieval 1；story_markdown 别名测试扩展。

## 三、TS 解析层对等（最小）

`hook-lifecycle.ts`：别名表增 superseded 组（同集）；`filterActiveHooks` 排除 superseded。新测试 `hook-lifecycle-superseded.test.ts` 三场景：别名解析 / 非活跃债务 / **md 投影往返不复活**（renderHooksProjection → parsePendingHooksMarkdown 保持 superseded）。

## 四、验证

cargo:testgate engine-rs 47 目标 **1967**（804 为 1957，+10 逐项对账）+ src-tauri 25 目标 584 + RustSec 双 0；clippy 双 0；gate:ts 八步全绿（含 engine-contract-diff + legacy 11/11）；权威 pnpm -r test core **177/982**（+1 文件 +3 测）+ studio 134/717 + cli 23/81。

验证层级声明：撤回路径为**单元级**（TS 流测试五场景逐一移植 + serde/解析往返 + 终态过滤）；walkthrough mock 无撤回分支故活体差分未覆盖 superseded——登记为可选后续（mock 增 withdrawal 支 → diff 双腿对跑）。

## 五、登记（695 工项余项）

- **rust 检索终态口径含 deferred**（`recycle_terminal_re` 排 deferred/paused/hold…）而 TS searchableHooks 只排 resolved+superseded（「every unresolved hook remains searchable canon」）——既有差异，独立定性（rust 行内注释「休眠种子仍可检索」与正则自相矛盾，需定口径）。
- rust reducer resolve/defer 对未知 id 优雅跳过 vs TS 抛错（TS 有 writer 层 validateSettlementHookIds 前置守卫，未知 id 不达 reducer；rust 分析师 delta 直达）——防御性差异，维持。
- UI：PendingHooksView 无 superseded 徽标（仅 deferred）——展示层可选增强。
- reject 回滚级联 / quality-trend / roster / detect / eval 维持登记。

## 六、教训

1. **枚举扩展的三类受影响面**：穷举 match（编译器报）、字符串解析（**静默**回落，必须手工枚举解析器）、以某变体作终态判断的过滤（语义审查）。只信编译器会漏掉后两类。
2. **上游加枚举值但未跟别名表**是真实的 TS 自身缺口（2.0 在 schema/reducer 层加 superseded，fork 期移植的字符串工具层未跟）——双栈对等不是「rust 追 TS」单向，解析层要双向补。
3. **同函数顺路漂移要考古定性再收**（defer 推进：移植时同款、上游后改 → 漂移 → 对齐），与 804 方法论同款。
4. **言语行为决定合并策略（续）**：撤回（incoming superseded）是第三种言语行为——不是覆写也不是调和，而是**归档**（保留原内容+附依据），其校验（不可撤 resolved、须依据）是产品规则非技术细节。
