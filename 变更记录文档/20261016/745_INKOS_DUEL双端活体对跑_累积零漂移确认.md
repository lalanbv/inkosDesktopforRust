# 745 号：INKOS_DUEL=1 双端活体对跑（累积零漂移确认）+ 上游巡检附带复核

> 目标线：自主持续开发。712-744 三十三批次后的双端契约活体验证：
> INKOS_DUEL=1 全量 strangler_duel 复跑，确认 TS/Rust 双端在所有嫁接面
> 与改造面上零漂移。

## 一、上游巡检附带复核（第十七巡）

fetch upstream 双 master 判据复跑：落后 upstream/master = 0，全部分支零
落后——第十七巡维持。

## 二、双端活体对跑实证（INKOS_DUEL=1）

strangler_duel 全量 10 测全绿（712-744 累积改动后零漂移）：

| 对跑面 | 状态 |
| --- | --- |
| strangler_full_grey_simulation | ✓ |
| strangler_session_domain_duel | ✓ |
| strangler_cross_write_read_duel | ✓ |
| strangler_timeline_duel | ✓ |
| strangler_series_backfill_duel | ✓ |
| strangler_services_domain_duel | ✓ |
| strangler_readonly_face_duel | ✓ |
| + 3 项其余 | ✓ |

**跨写读一致性**（cross_write_read_duel）在 714 号 repair-state 嫁接与
725 号 story-markdown 移植后仍然全绿——TS 写→双端读 + Rust 写→双端读
的一致性契约未受累积改动影响。

## 三、验收

零代码改动（纯验证批次）；engine-rs lib 1499 + duel 10 + golden 70 +
clippy 0 此前已确认。

## 涉及文件

- 本件（验证归档，零代码改动）
