# 527 号：resync 留痕永久备案裁决 + 工件对照全量转硬门禁（内容备案清单出清）

日期：2026-09-18　分支：develop　基线：89292993（526 号）

## 背景

526 号后工件内容备案清单尚余 7 项登记（实际命中已归零）+ resync 留痕 only-node 3 项。本轮裁决 resync 留痕面「清偿 vs 永久备案」，并兑现 522 号立约的「清偿后工件对照转硬门禁」。

## 裁决：resync 留痕 = 永久备案（不清偿）

### 证据链

- **TS 侧**：`resolveGovernedPlan`（packages/core/src/pipeline/runner.ts）在无 externalContext 时 `loadPersistedPlan(bookDir, chapterNumber)` 复用持久 plan **跳过 planner LLM 调用**，plan 后 `savePersistedPlan`——intent/plan/rule-stack 三件套是 **planner memo 化缓存 + 人类可读留痕**，功能性文件。
- **Rust 侧**：`write_next.rs` 的 `load_persisted_plan`（外上下文为空时复用）+ `plan_chapter` + `save_persisted_plan`——**write-next 主链同等 memo 化已在**。
- **用户可见影响**：resync 后首章 write-next 若无新上下文，Rust 侧会多一次 planner LLM 调用（成本面，非正确性面）+ 回放留痕缺失（透明度面）。
- **补齐代价**：须给 Rust resync 直写链加 governed plan 阶段 = 改变 resync 的 LLM 调用面——519 号已裁决 resync 架构维持现状。

结论：**永久备案**。收益（省一次 planner 调用+留痕补全）不抵改变 resync 架构面的风险，与 519 裁决一致。

### 实施

`ALLOWED_ONLY_NODE` 注释升级为裁决记录（引用 519/527 双号，写明 memo 化等价证据与不清偿理由）。

## 工件对照全量转硬门禁

- `ALLOWED_CONTENT_DIFFS` **7 项全部出清置空**：522 号建立时备案（尾换行装置形态/settle tension 注入列/plan 措辞/rule-stack 序列化格式/status_raw 形状），经 523–526 修复与归一化后活体实测命中归零——本轮差分器输出「✓ 落盘工件内容一致（归一化后；零备案硬门禁）」。
- 此后**任何工件内容分歧（归一化后）直接计入 divergences 硬拦截**；新分歧原则上定性修复，不再新增备案（清单结构保留供极端装置差异应急）。
- 备注文案同步纠偏：豁免复核输出「无豁免 ✓（可删豁免条目）」为 519 收口期陈旧文案（promises/roster-candidates 豁免彼时已撤销）——改为「裸比对一致 ✓」并更新循环注释（context-lens 豁免仍按 519 裁决保留）。

## 验证（全量门禁 8 项全绿）

- 差分器 **41 对照项 0 分歧**；工件树仅备案 resync 留痕 3 项；内容面零备案。
- engine-rs cargo test **1805 绿**；src-tauri **578 绿**；`pnpm clippy:gate` 双 crate 0 告警。
- 双活体套件（node-fallback-smoke 8/8、export-epub-smoke 双腿）绿；`gate:ts:fast` 全绿。
- `INKOS_DUEL=1` 真跑 strangler_duel **10/10**；`pnpm bench:gate` 零回退（负载 19.75 拦截后静候回落 2.71 重跑）。

## 教训

- **备案清单是快照不是垃圾场**：清单条目在实际命中归零后若不出清，读者无法区分「仍在分叉」与「已修复留位」——快照断言+定期出清（对照活体输出）才能让清单反映真实分歧面。
- **裁决要写进机制现场**：519 号裁决散落在变更记录里，差分器现场只有一句旧注释——裁决升级时把证据链（双端 memo 等价、影响面、不补齐理由）写进机制注释，后来者不必重查。
