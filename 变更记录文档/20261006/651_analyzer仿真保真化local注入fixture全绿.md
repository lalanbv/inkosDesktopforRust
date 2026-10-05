# 651 号：analyzer 仿真保真化——local 注入下 fixture promises 断言全绿（650 备案清偿）

日期：2026-10-06。类型：chore(scripts 走查基建)。前置：650 号备案「analyzer 占位的 truth 保真边界（UPDATED_* 最小占位致 memory.db promises 投影缺失→fixture promises 断言红）」。单文件改动（scripts/walkthrough-mock.mjs 的「小说连续性分析师」分派）。

## 一、保真化设计

promises 投影由 pending_hooks.md 的伏笔行驱动（14 列 R23 台账，分类列驱动 kind：悬念/情感/物品/世界观 → suspense/emotion/artifact/worldview）。650 号的最小占位把 UPDATED_HOOKS 写空 → 伏笔池投影 0 条 → fixture 断言红。

升级形态：mock 新增 `ANALYZER_HOOKS_TABLE` 常量——**与 walkthrough-fixture 预置池逐行同源**的 14 列全表（H01 悬念/H02 情感/H03 物品/H04 世界观），analyzer 分派的 UPDATED_HOOKS 节引用之。mock 与 fixture 为配套脚本（walkthrough 系列），同源耦合以注释锚定。UPDATED_STATE 从 SETTLER 模板语境派生近似表；content/title 沿用 650 形态（persistenceOutput 强制回写审改后 finalContent，analyzer 解析器对 content/title 有兜底——保真要点仅在 truth 更新面）。

## 二、红绿（活体，AUDIT_SCORES="45,91"+AUDIT_SCOPE=local）

- **红**：650 号 650g 实录——`promises timeline 断言失败（memory.db 投影重建缺失）`（timeline 0 条），env 断言自杀。
- **绿**：`promises timeline = 4 条，kinds = worldview,suspense,emotion,artifact`（四 kind 全齐、断言全绿、env 完整可用）+ 审改循环保持（`修复轮次 1/1（45 分）→ 修复后达到通过线（91 分）`）+ **PATCH 版落盘保持**（0002_风起.md 含替换句「缓缓收紧五指」×1、旧句 ×0）。
- **回归**：默认 structural 形态（无注入）fixture 全绿零回归（analyzer 仅修稿后被调，无循环路径不触及）。gate:ts 八步全绿。Rust 零触碰（647 刚整体复验）。

## 三、备案

- ANALYZER_HOOKS_TABLE 与 fixture 预置池为同源副本：预置池行变更（加伏笔/改 kind）须同步 mock——两个 walkthrough 脚本的配套耦合，锚注释已互相指认。
- UPDATED_STATE 为近似表（当前章节写死 2）——多章连续 local 注入时状态卡不随章号走；promises 断言不依赖 state 卡，走查语义可接受；若需章号动态化可复用 settlerDelta 的「第 N 章」抽取法，另号。
- analyzer 的其余输出面（CHAPTER_SUMMARY/UPDATED_SUBPLOTS/EMOTIONAL_ARCS/CHARACTER_MATRIX）未仿真（parseWriterOutput 缺 TAG 容忍）——fixture 断言不覆盖，需要时同法扩。

## 四、教训

1. **mock 占位形态的保真度要按「谁消费它」定标**：analyzer 输出的消费方是 truth 落盘+memory 投影——占位写到「解析器不炸」只满足了第一层；promises 投影把分类列当数据源，占位空表=数据清空。占位升级的验收标准=下游全部消费方的断言全绿，不是管线不再悬挂。
2. **同源数据的副本要有锚注释**：ANALYZER_HOOKS_TABLE 与 fixture 预置池的耦合是 walk-through 配套脚本的既有模式（mock 的 ARCHITECT/PLANNER/WRITER 常量均与 fixture 呼应），副本漂移靠注释互相指认+fixture 断言兜底。
