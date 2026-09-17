# 519 号：resync 语义分歧产品决策落定——分歧真相反转 + Rust write-next 投影时序缺陷修复

日期：2026-09-18　分支：develop　基线：a658e564（518 号）

## 背景与决策过程

483 号备案的「resync 双端架构分歧」（Rust 直写 vs TS governed 管线）悬置 36 号。518 号差分器实证「resync 后双端读面不一致」但未定位行为方向。本号写一次性采证脚本（双腿独立 fixture → resync 前后各采 currentChapter / lensChapters / sqlite 摘要行）做行为精测，**分歧真相反转**：

| 采证点 | TS 腿 | Rust 腿（修复前） |
|---|---|---|
| currentChapter（fixture 后） | 3 | 2 |
| resync/2 前后变化 | **3→3（幂等）** | **2→2（幂等）** |
| chapter_summaries 表 | ch1+ch2 | 仅 ch1 |
| lensChapters | [1,2,3] | [2,3] |

**定案三连**：
1. **resync 端点双端幂等等价**——两端 resync 前后读面零变化。483 备案的"resync 分歧"名义不成立，无需架构决策。
2. **真实分歧 #1（实质缺陷，修）**：Rust write-next 后 sqlite 摘要投影缺本章行——currentChapter/promises/timeline/roster-candidates 面停在上章。作者写完第 2 章后承诺账本与推进度显示错误。
3. **真实分歧 #2（低危，备案）**：TS resync 产出 ch1 装配留痕而 Rust 不产——仅 context-lens 透明回放完整性，不影响写作数据。保留豁免。

## 缺陷 #1 根因与修复

Rust write-next 有叙事记忆投影段（406 号：结构化 state → memory.db，对齐 TS `syncNarrativeMemoryIndex`），但**位于 `persist_chapter_artifacts`（最终落盘）之前**——投影读到的是本章落盘前的旧结构化 state（只有上一章摘要行），sqlite 永远缺本章。TS 侧顺序为 saveChapter → syncNarrativeMemoryIndex（投影在落盘后），双端语义实为一致、Rust 时序错。

修复（`engine-rs/src/pipeline/write_next.rs`）：投影段移至最终落盘调用之后、result 构造之前，注释写明时序约束（519 号）。

**过程教训**：python 脚本移动代码块时 `src.index()` 命中第一个同名 `.map_err(...Persistence...)` 锚（349 行，章节起草落盘段）——插入到错误函数位置后 cargo check 照样通过、活体复测行为不变；第二次改用「map_err + 后续 `let result = ChapterPipelineResult`」的唯一组合锚并断言 `count==1` 才落对。**代码块移动必须验证目标位置唯一性，且移动后立即活体复测**（编译绿 ≠ 行为对）。

## 差分器豁免裁决落定（engine-contract-diff.mjs）

- **撤销**：`promises`、`roster-candidates` 的 `currentChapter` 豁免——缺陷修复后无豁免全绿。
- **保留**：`context-lens` 的 `chapters` 豁免，理由改写为 519 号裁决备案（装配留痕回放完整性，非 resync 语义分歧）。
- 豁免复核段照常输出自证：promises/roster ✓ 可删（已删）、context-lens ✗（预期，保留）。

## 验证

- 活体：修复后 Rust 腿 currentChapter=**3**（与 TS 一致）、sqlite ch1+ch2 两行、resync 幂等保持；差分器 **38 端点 0 分歧**。
- `engine-rs` 全量 cargo test **1803 绿**（write_next 改动后复跑）；`pnpm clippy:gate` 双 crate 0 告警。
- node-fallback-smoke 双引擎 13/13 绿；export-epub-smoke 双引擎绿。
- `pnpm bench:gate` 9 基准零回退（全部优于基线 -3%~-17%；两轮被负载守卫拦截后静候回落重跑）。
- TS 侧零改动。

## 关联

- 483/484（备案与基线修复——本号反转定案并关闭）、518（差分器 resync 面——本号精测推翻其"resync 后分歧"归因，实为 fixture 阶段差异）、406（投影段引入）、R26/396（promises 面）、403（run-log 面）。
- 483 号「resync 架构分歧专项」就此**注销**：无架构决策需求，一个时序缺陷 + 一个低危备案即全部事实。
