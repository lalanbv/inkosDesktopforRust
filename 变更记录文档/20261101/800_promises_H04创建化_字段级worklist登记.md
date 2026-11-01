# 800 号：promises H04 创建化——timeline 结构收敛与字段级 worklist 登记

日期：2026-11-01。前置：799 号登记（promises timeline 长度 3vs4 的 H04 差异）。

## 一、H04 源头定性与创建化

H04 "镜宗与皇室的隐秘盟约"（kind worldview，openedAt 3）= walkthrough/夹具 **台账种子的第 4 条**（mock ANALYZER_HOOKS_TABLE 与 fixture 台账同源）——rust ledger 经 resync 导入台账全量（H01-H04）故 timeline 有 H04；node 腿 settle delta 只携带模板 H01-H03 → H04 整条缺失。

修复：mock settle 工具支 upsert 扩为 **4 条台账对齐记录**（H01-H03=模板 merge 终态值，H04=台账原始记录 startChapter 3/expectedPayoff 空/notes 镜宗与皇室）。

## 二、活体收敛状态

- **H04 已在 node timeline 出现**（795 时代整条缺失 → 结构性收敛）；timeline 结构/notes/summary/expectedPayoff 全对齐。
- **余下字段级差**（contract-diff 活体捕获，逐项登记）：
  - H04 openedAt node 1 vs rust 3——node H04 记录的 startChapter 被写链某环节改写（疑 resync settle 的 delta.chapter 覆盖面），待 --keep 定位；
  - H01 lastAdvancedAt node 2 vs rust 3——同族（lastAdvancedChapter 被中途 settle 的 delta.chapter 覆盖）；
  - node 行缺 `kind` 键——handler 台账 enrich 未命中（待查 hookId 匹配/解析形态）；
  - eval/reject 级联（797 定性维持）。
- 这四项均为**字段级值差**而非结构/存在性差——收敛路径明确（写链 settle 的 startChapter/lastAdvancedChapter 覆盖面核查 + kind enrich 修复）。

## 三、验证

- diff 报告模式：共享面 71 对比 / 6 分歧（promises 分歧首叉从「timeline 长度」深入到「字段值」=收敛推进的度量证据）；legacy 通道 11/11。
- 全电池：core 176 文件 + studio 134 + cli 23 全绿；clippy 双 0；testgate 1956+584 零漂移；gate:ts 八步全绿。
- 插曲：derived-confirmation 测试单跑即绿（并行资源竞争假红——散进程清场后全电池绿，795 同款教训复刻）。

## 四、教训

1. **结构性收敛先于字段级收敛**：H04 从「整条缺失」到「在场但字段值差」是收敛的阶段度量——worklist 的粒度随收敛推进而细化，这是正常的收敛节奏。
2. **mock 的 settle upsert 应以「rust ledger 终态」为准绳**而非模板——模板是 rust 链的中间态（resync 导入台账 + settle merge 后的终态才是 ledger 实际内容）。
