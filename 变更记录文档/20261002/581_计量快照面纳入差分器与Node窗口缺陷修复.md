# 581 号：564 备案清偿——上下文计量快照面纳入差分器+Node 计量窗口零缺陷修复（当场抓获）

- 日期：2026-10-02
- 类型：fix(core)+test(scripts)
- 路线：564 号「快照面 fixture 不驱动」备案清偿；差分器对照 45→46
- 状态：✅ 门禁全绿收口

## 交付

### ① 差分器会话面新增上下文计量快照对照（46 对照）

会话面驱动的 chat 轮落幕后双腿 `GET /api/v1/context-meter?sessionId=` 活体对照：键集全等 + `source==="estimate"` + `anchorValid===false` + `inputWindow` **相等且 >0** + `surfaceNodes` 计数非零。`tokens` 数值备案不深比（聊天 system prompt 双端未 golden 锁定，启发式基数可差——活体实测 node 2626 / rust 2433 同为估算语义）。

### ② 当场抓获并修复 Node 计量窗口缺陷（真实缺陷一例）

首跑即红：`inputWindow node=0 vs rust=128000`。考古链：无模型卡模型（mock）的 `contextWindow` 经服务端模型列表组装为 **0**（`>0 才带字段`），直传 `new TokenMeter(0)` → 计量窗口 0 → **超窗告警面永不触发**（观测盲区）。修复：`agent-session.ts` 计量构造点 `model.contextWindow > 0 ? model.contextWindow : 128_000`（非正数一律按缺省窗口，与 provider.ts 模型卡 miss 兜底同源）。修复后双腿 128000 相等，断言加深为窗口相等。

### ③ 两次脚本侧自纠（诚实记录）

- 形态断言初版假设 `surfaceNodes` 为数组——活体实测双端均为**数字计数**，修正判定（假设须对照活体形态）；
- `?? 128_000` 对 **0** 不兜底（0≠undefined），改显式 `> 0 ? : 缺省`；
- core 为 dist 消费：patch 后必须 `pnpm --filter core build` 再验（572 号「gate typecheck 瞬态红=dist 陈旧」同源第二例）。

## 门禁

gate:ts 七步全绿（typecheck/test/build/node-fallback-smoke/engine-contract-diff 46 对照 0 分歧/export-epub-smoke）；Rust 零改动。

## 下一号自 582 起
