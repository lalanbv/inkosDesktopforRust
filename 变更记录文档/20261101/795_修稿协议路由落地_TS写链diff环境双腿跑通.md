# 795 号：修稿协议路由落地——TS 写链 diff 环境双腿跑通（698 间隙清偿）

日期：2026-11-01。前置：794 号登记项 ② 尾段（submit_revised_chapter 修稿协议）+ 报告模式 write 启用复验。

## 一、修稿协议路由（mock 三增量）

1. **submit_revised_chapter 工具支**（ChapterRewriteToolSchema `{revisedContent}` min1，validate 长度门=篇幅预算带）——复用 WRITER_BODY（645 号口径达标），与 draft 同文=确定性收敛。缺路由时修正循环打转（794 实锤 182 次 revise）。
2. **settle 工具支四键全发**：TS `SettlementToolSchema` 的 hookOps 要求 mention/resolve/defer **required**（rust serde default 容忍缺省）——显式空数组补齐。
3. **newHookCandidates 化**：空基线台账（chapter-0 快照 hooks 空）下 upsert 已知 id 被 reducer 拒（"Unknown hook id(s): H01,H02,H03. ... submit a newHookCandidate"）——模板 H01-H03 改走 `newHookCandidates`（NewHookCandidateSchema 三字段：type/expectedPayoff/notes；hookId 由 reducer 派生）。

## 二、对称深播夹具（794 回退版重施——本轮写链已通，约束前提成立）

- node 腿 §2.5：outline 双件 + `snapshotState(BOOK, 0)`（793/794 已验证的解锁件）。
- rust 腿报告分支：`seedRustOutlineAndSnapshot`——books 布局镜像（outline 双件 + snapshots/0 文件镜像 rust `snapshot_state_at` 的 7 md 尽力拷 + state/*.json）。
- **双腿对称真写**：两报告分支统一 `runWriteChain`（resync → write-next → 轮询 chapters≥2，240s 预算）——chaptersWritten 2=2 由真写达成，消融 698 的「双腿停在播种态」不对称 hack。

## 三、验证

- **写链双腿跑通实证**：diff 报告模式双腿夹具零 die 零部分失败（轮询退出条件=chapters≥2，die 未触发）——node/rust 各自完成 resync→write-next→第 2 章落盘全链（793 时 node 240s 超时、794 单跑才通 → 本轮 diff 环境双腿全通=698 间隙正式清偿）。
- legacy 通道 **11/11** 保持绿；共享面 68 对比 / 6 分歧=**派生数据 worklist 显形化**（tension-curve 曲线点/promises-quantity-trend currentChapter/roster-candidates/eval avgParagraphWarnings/detect 存储——引擎级派生差异，695 迁移工项活体清单，报告模式记账 exit 0 设计内）。
- clippy 双 0 + testgate 1956+584 零漂移 + gate:ts 八步全绿。

## 四、登记（695 迁移工项活体清单，按需开工）

报告模式共享面现存 6 分歧均为**引擎级派生语义差**（非 legacy 簇、非 mock 缺口）：①tension-curve 曲线点数（node 第 2 章 summary 张数差）②promises currentChapter 与 timeline 来源（rust promise ledger vs TS 派生）③quality-trend currentChapter 同源 ④roster-candidates 候选生成面 ⑤eval avgParagraphWarnings 计入口径 ⑥detect 结果存储面（TS 404=无检测存储）。收敛=695「fork 契约恢复」主工程逐端点处置。

## 五、教训

1. **写链协议的 mock 路由要逐工具对 schema**：本轮三增量（revised/review/state_validation）+794 两件（memo/draft/settle）=2.0 写链全工具面补齐——每个工具的 decode schema 都独立，"同链同形"是错觉。
2. **TS schema 的 required 严格度 > rust serde default**：hookOps 四键、settle 四件——TS 侧显式、rust 侧容忍，mock 必须按最严侧构造。
3. **空基线台账的 hook 语义差**：rust 文本 delta 的 upsert 会被 serde default 容忍后由 reducer 拒绝（同错误），TS 侧显式要求 newHookCandidates——两边都是「新钩子走 candidate」，只是错误形态不同。
