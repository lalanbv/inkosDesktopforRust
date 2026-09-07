# 212 号：双端 golden 差分回归——节拍沉淀纯函数域共享向量守门

- 日期：2026-09-07
- 分支：develop
- 关联：191 号（beats 双端「逐字对齐」声明——本批补差分守门）、189 号（Rust 端先例）、179–211（大批量改动复核起点）
- 推送核验：origin/develop 仍停 6bc65564——**201–211 十一提交均未推送**；本批次后本地领先 12。两项默认值无新答复。

## 一、复核盘点（179–211 契约面）

| 增量域 | 双端守门现状 |
|---|---|
| 回填 extract/apply（190/192/194） | **duel 已覆盖**（`strangler_series_backfill_duel`：400 面 + mock LLM 全链草稿等价差分）✓ |
| 时间线端点面（189/191） | **duel 已覆盖**（`strangler_timeline_duel`）✓ |
| SSE/静态面/灰度全链 | duel 已覆盖（10 用例）✓ |
| 重试行为（205/206） | 行为级双端对跑（Rust 集成测 + core llm-retry-behavior）✓ |
| **beats 纯函数（parse/merge）** | **零双端差分**——191 号「逐字对齐」仅人工声明，无向量守门 ← 本批补 |

## 二、实施：共享 golden 向量差分

- **向量文件**：`packages/core/src/__tests__/golden/beats-vectors.json`——8 parse 用例（围栏剥离/裸 JSON/未知 id 过滤/双空白丢弃/title 空白保留/非字符串 id 跳过/缺 beats 数组错误/非 JSON 错误）+ 5 merge 用例（原位替换/升序中插/尾插+空白净化/未知忽略+多线计数/零落格保持 updatedAt）。
- **core 侧**：`__tests__/golden-beats.test.ts` 读向量逐例断言（13 用例；updatedAt 非确定——断言刷新语义而非具体值）。
- **engine 侧**：`tests/golden_beats_diff.rs`（`include_str!` 跨包读**同一向量文件**）逐例断言（2 测试函数承载 13 例）。
- **语义确认**：差分首轮即比对双端实现逐行同构（围栏剥离/HashSet 过滤/is_blank 双空白丢弃/原位替换/`take_while` 升序插入/`filter(!trim().is_empty())` 净化/applied 计数/`utc_now_iso` 与 `toISOString` 刷新）——**13/13 双端全绿，无漂移**。

## 三、验证

- engine：`golden_beats_diff` 2/2（13 向量例）、lib **1286**、集成 194+70+10（+golden_beats_diff 新文件）、clippy 零告警、INKOS_DUEL=1 duel **10/10**。
- core：vitest **199 文件 1892 用例**（+13）、typecheck ✓。
- 无生产行为改动（纯测试资产 + 向量契约文件）。

## 四、结论

179–211 增量契约面复核闭合：duel 十面 + 重试对跑 + 本批 beats golden 差分，新增域全部有双端守门。向量文件成为 beats 域的唯一事实源——后续任一侧改动引发语义漂移即红（此前只能靠人工比对）。
