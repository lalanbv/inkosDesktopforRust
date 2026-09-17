# 524 号：chapters/index.json 定性——篇幅越界审计缺报修复 + 工件对照时间戳归一

日期：2026-09-18　分支：develop　基线：85dd6879（523 号）

## 背景

522 工件备案清单中的 `chapters/index.json`（node 853B / rust 781B）定性专项。

## 定性（--keep 现场逐字段对照）

| 差异字段 | 定性 |
|---|---|
| `createdAt`/`updatedAt` 时间戳 | 双腿独立起引擎写入时刻不同——时序波动，非契约 → 差分器归一 |
| ch2 `auditIssues`：rust 缺 `[critical] Chapter length 62 is outside the required range 2182-3818.` | **实质缺陷**：Rust 审计环漏报篇幅越界 |
| 结构形状（number/title/status/wordCount 等 8 字段） | 完全一致 |

## 缺陷修复（engine-rs/src/pipeline/chapter_review_cycle.rs）

Rust `assess_content` 的 `all_issues` 漏掉 length 条目（TS `lengthIssues`：critical/`length-budget`/越界文案+修复建议）；修复为越界时构造并入。**passed 判定不降级**：TS 实测越界章 status 仍 `ready-for-review`（write-next 主链 `chapterStatus` 仅 state-degraded 显式传入），Rust 对齐该语义。

**过程教训**：首轮修复曾同时给 `passed` 加 `|| !length_in_range`——6 个 write-next e2e 立即红（audit-failed/needs-review vs 断言 ready-for-review/complete）。这些断言锁的正是 TS 实测语义，**静态推理（review cycle passed 公式含 `!lengthInRange`）与实测（status 不降级）矛盾时，以双端产物实测为准**——TS 主链存在静态阅读未覆盖的判定环节，逐字段对照才是裁决依据。同型教训三犯（522 尾换行、521 移码块、本轮 passed），「改后立即活体复测」惯例再确认。

## 差分器改进

工件对照 json 归一化新增 **ISO 时间戳值归一**（`createdAt`/`updatedAt` → `<ts>`）——双腿写入时刻差异非契约。

## 备案清单状态（收敛 5→3 类）

- ✅ index.json（本号清偿：审计缺报修复+时间戳归一）
- 备案：仅 node 有 runtime/chapter-0001 intent/plan/rule-stack（resync 留痕面）
- 备案：0001 尾换行 1B（fixture 直写形态）；runtime/chapter-0002 intent/plan/rule-stack 措辞（prompt 模板对齐专项）；snapshots hooks.json（serde 空字段序列化形状）

## 验证

- 差分器 **41 对照项 0 分歧**（index.json 分歧消除；chapters/2 API 面恢复绿）。
- engine-rs 全量 cargo test **1805 绿**（含 6 个恢复的 e2e）；`pnpm clippy:gate` 双 crate 0 告警。
- 双活体套件绿；`gate:ts:fast` 全绿；`pnpm bench:gate` 零回退（-3.1%~-17.9%，负载守卫拦截后回落重跑）。

## 关联

- 522（工件面与备案清单）、523（别名表同型对齐）、453/522（实测优先定性原则）、257（bench 负载守卫）。
