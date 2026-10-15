# 711 号：Rust 引擎 0GC 对齐轮——标题去重热路径分配收敛 + duel 守卫前置（clippy zombie 清偿）

> 日期：2026-10-16（710 号同日续号；目标点名 Rust 客户端/桌面程序与 0GC 高性能面的本轮工件）
> 范围：engine-rs（agents/post_write_validator.rs 热路径 + tests/strangler_duel.rs 资源卫生）；桌面壳核查性通过

## 一、0GC 工件：标题去重两轮分配收敛（bench 实证 -14.4%/-19.8%）

目标函数 = `resolve_duplicate_title`（write-next 每章必经；bench 基线中最贵项 `title_dedup/resolve_duplicate_hit_200` 175.82µs）。685 号已做过一轮（索引复用/blocked HashSet/英文侧缓冲复用），本轮在其基础上收尾：

**第一轮（qualifier 抽取三则，语义逐字）**：
1. `existing_titles.join("")+format!` 双重分配 → 单缓冲 `String::with_capacity` 构建（200 标题连接体 ~5KB/次的中间串消除；中英两变体同修，英文侧空格分隔逐字节等价）；
2. 中文变体 `segments: Vec<String>` 全量 collect → 惰性借用迭代（首个命中即返回，collect 是纯前置浪费）；
3. 中文候选逐 (start,size) 一次 String 分配（collect+trim+to_string 三段）→ 复用缓冲 + `&str` 裁剪窗（685 号英文侧同型收尾）；`chars: Vec<char>` 逐段分配 → 跨段复用缓冲。

**第二轮（TitleIndex 构建面，交错语义逐字保持）**：
4. `lower: Vec<String>` → `Vec<Cow<str>>`——无大写字符时借用原串（CJK 标题常态），免 200 次 `to_lowercase` 中间分配（`is_uppercase` 前哨保证 'İ' 类特殊映射不走借用路径）；
5. `strip: Vec<String>` → `Vec<OnceCell<String>>` 惰性逐项 memo——精确命中在近重复探测之前断掉的迭代（bench hit 形态）只为已访问项付 regex/分配成本；**strip 仍在每次比较时消费**，交错报告顺序（near-hit@2 先于 exact-hit@4）逐字保持——两阶段重构会改变违约类型报告顺序，故弃。

**实测（criterion，quick 采样与全量门禁双确认）**：

| bench | 基线 | 本轮 | Δ |
| --- | --- | --- | --- |
| title_dedup/resolve_no_duplicate_200 | 37.14 µs | 29.78 µs | **-19.8%** |
| title_dedup/resolve_duplicate_hit_200 | 175.82 µs | 150.45 µs | **-14.4%** |
| 其余 12 项 | — | — | 全部带内（sse_broadcast +15% 为纳秒级负载抖动，未触该代码，阈值 +30%） |

bench-gate ✓；基线**不刷新**（门禁文档：--update 仅超阈值回退时需要；改进保持为每次复跑的正向增量可见）。criterion "Performance has improved"（p<0.05）活体标注。

## 二、clippy zombie_processes 清偿（duel 守卫前置）

clippy 新 lint（工具链演进引入，非本轮代码）拦截 `tests/strangler_duel.rs` 两处：`duel_require_fork_contract!` 的豁免早退路径会丢弃已 spawn 的引擎子进程（无 wait → zombie 风险）。修复=**守卫前置到 bin spawn 之前**（static_face/SSE 两测同型）——守卫只探测 TS 腿契约源，语义等价；豁免早退路径不再冷启引擎（跳过态附带省一次引擎启动）。其余七处守卫用 `spawn_rust_engine` helper（自有回收形态）不在拦截面。

## 三、桌面壳核查性通过（src-tauri）

零 TODO/FIXME；`cargo test --lib` 468 passed（testgate 全口径 584 含集成）；clippy 0 告警。壳侧无待办积压（updater 双通道 166 号已落地）——本轮 Rust 面工项按证据落在引擎热路径。

## 四、门禁终态

| 门禁 | 结果 |
| --- | --- |
| cargo:testgate | 1933 + 584 **双零漂移**（duel 守卫前置后活体复跑） |
| clippy:gate | engine-rs 0 + src-tauri 0（zombie 清偿后） |
| bench:gate | ✓ 带内通过（title_dedup 双项改进 -14.4%/-19.8%，p<0.05） |
| TS 侧 | 零改动（gate:ts 于 710 号已认证） |

## 五、教训

1. **0GC 收敛要区分「计算顺序的语义」与「分配时机」**——strip 惰性化安全的前提是它在每次比较时仍被消费（OnceCell memo 保交错报告顺序）；两阶段化（先全表精确再全表 strip）虽然更快但会改变违约类型报告顺序，逐字契约下不可取。
2. **借用快路径的哨条件要覆盖特殊映射**——`is_uppercase` 前哨让 'İ'（lowercase 展宽为两码元）类字符不走借用路径，`to_lowercase` 恒等性只在「无大写字符」时才可保证。
3. **bench 基线不追改进**——基线是回退闸不是成绩单；优化落地后保持基线不动，改进以正向增量每次可见，避免把负载抖动（本轮 sse_broadcast +15% 纳秒级）烘进新基线放松未来门禁。
4. **新 clippy lint 是资源卫生的免费审计**——zombie_processes 抓到的「守卫早退泄漏子进程」在跳过态（INKOS_DUEL 未开 fork 契约）每次都发生；守卫前置比 Drop guard 更优（省一次冷启）。
