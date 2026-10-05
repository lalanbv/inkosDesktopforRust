# 658 号：restore 分支活体覆盖——chapter-review-cycle 六分支矩阵全收（657 决策表 L4 清偿）+ 655–658 矩阵复验

日期：2026-10-06。类型：chore(scripts 走查基建)+docs(矩阵复验)。前置：657 决策表 L4「『回退到最高分版本』restore 分支未活体覆盖（构造需 mock 修订输出加超长对）」。

## 一、L4 清偿：restore 分支活体构造与验证

- **构造**：mock 新 env `WALKTHROUGH_MOCK_REVISE_BLOAT=1`——修稿 PATCH 的 REPLACEMENT 追加超长填充（~1833 字），修订内容越篇幅带（约 4239 > hardMax 3818）。默认不启用，既有形态（646/653/655）逐字节不变。
- **活体结果**（AUDIT_SCORES="45,91"+local+BLOAT=1）：`修复轮次 1/1（当前 45 分）→ 修稿（超长版）→ 复审 91 分（lengthInRange=false → isPassed=false → 净提升 91≥48 true → finalContent=超长版，maxIter=1 自然结束）→ bestSnapshot 择优：初稿 in-range 胜出 → logWarn「回退到最高分版本（45 分 vs 当前 91 分）」→ finalContent=初稿`。
- **落盘验证**：0002=初稿 2405（无 PATCH 痕迹、无超长尾段）、章节 audit-failed（45 分初稿，诚实语义——修稿越界被回退）。
- **六分支矩阵全收**：达标退出/净提升继续/未净提升退出/未产出新内容/多轮上限/restore 回退——chapter-review-cycle 全部继续/退出/回退分支均有活体证据。

## 二、655–658 矩阵复验（654 先例）

默认形态回归（无注入）fixture 全绿零回归；clippy 双 0；testgate engine-rs 45 目标 1926 + src-tauri 25 目标 584（与 640/647/654 基线同数=四号零 Rust 改动吻合）；duel 真跑独立 10/0（41.15s）；audit 双 0；gate:ts 八步全绿。Rust 面引用链继续锚定 HEAD。

## 三、门禁

bench（候谷轮询后通过零回退）+ gate:ts 八步全绿；clippy/testgate/duel/audit 如上。Rust 源码零触碰（mock env 构造，纯 scripts）。

## 四、备案

- REVISE_BLOAT 与 REVISE_SEQ 解耦（env 开关独立于轮次对）：默认关；开启时每轮修稿都产超长版（多轮序列下每轮都触发 restore 回退初稿——语义一致）。
- L4 清偿后 657 决策表仅余 L1（内容回显近似，需真 LLM）/L2（三面空节，随断言）/L3（计数器共享，并发用不同端口）/L5（scope 分离，真实需求）/L6（pi1.0，等 key）——全部挂起态有明确触发条件。

## 五、教训

1. **restore 分支的构造关键是让「修订比初稿差在长度维度」**：分数高（91）但 lengthInRange=false → isPassed 拒绝 → 净提升判定却放行 finalContent=超长版 → bestSnapshot 的 lengthInRange 优先判据把初稿选回来——三个判定（isPassed/净提升/bestSnapshot）各按自己的语义走，构造用例要让三者分叉。
2. bench-gate 自身的负载闸是权威判定（外层脚本轮询逻辑失效时它仍正确拒绝/放行）——门禁的闸不做双层假设，外层只做候谷等待。
