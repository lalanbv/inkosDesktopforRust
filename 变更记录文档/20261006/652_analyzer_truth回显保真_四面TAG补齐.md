# 652 号：walkthrough-mock analyzer truth 回显保真——CHAPTER_SUMMARY/UPDATED_* 四面补齐（650/651 备案延伸）

日期：2026-10-06。类型：chore(scripts 走查基建)。前置：651 备案「analyzer 其余输出面（CHAPTER_SUMMARY/SUBPLOTS/EMOTIONAL_ARCS/CHARACTER_MATRIX）未仿真（parseWriterOutput 缺 TAG 容忍）」。单文件改动（scripts/walkthrough-mock.mjs 的 analyzer 分派）。

## 一、缺口实证（650 同族）

651 轮（local 注入）与无循环基线的 truth 对照：**chapter_summaries.md 587→341、current_state.md 432→292、audit_drift.md 761→313**——analyzer 输出缺四面 TAG → parseWriterOutput `extract` 返回空串 → 落盘覆盖缩水（`??` 不兜空串）。其中 audit_drift 的差异为审计注入面正常差异（注入 issue 数不同），前两者为真缺口。

## 二、实现（truth 回显保真）

真实 analyzer（governed 模式）的请求形态经临时 dump 实证：truth 区块拼在 **user prompt**（「已选伏笔证据」「已选章节摘要证据」「当前状态卡」「其他状态」；system 仅 persona+格式）。据此：

- **CHAPTER_SUMMARY**：从 system 的「已选章节摘要证据」行（`- story/chapter_summaries.md#N: 章 | 标题 | 关键事件 | 状态变化 | 伏笔动态`）回显已有行+按 mock 进程内 `LAST_SETTLER_CHAPTER`（settlerDelta 结算时记录）追加当前章新行。
- **UPDATED_STATE**：当前章号改用 `LAST_SETTLER_CHAPTER`（651 为写死 2）。
- **UPDATED_SUBPLOTS/EMOTIONAL_ARCS/CHARACTER_MATRIX**：governed 请求无对应输入区块且 story/ 目录无对应落盘文件——空节保持（不产生文件，无缩水面）。
- 首版 grab 从 user 抽「## 已有章节摘要」等 buildSystemPrompt 形态的区块头——实测 governed 模式区块头完全不同（「已选章节摘要证据」证据行形态），改为证据行正则回显+转译（临时 dump 请求全文一步定位，实证后移除）。

## 三、红绿（活体，AUDIT_SCORES="45,91"+AUDIT_SCOPE=local）

- **红**：651 实录 chapter_summaries 341（缺当前章行、形态为旧短文）。
- **绿**：652c——promises timeline 4 条四 kind 全齐（保持）+ PATCH 版落盘保持（0002 含替换句）+ chapter_summaries 含当前章新行（形态为 writer 同款 10 列）。审改循环达标（45→91）保持。
- **回归**：默认 structural 形态 fixture 全绿零回归。gate:ts 八步全绿。Rust 零触碰（647 刚整体复验）。

## 四、备案

- **CHAPTER_SUMMARY 单行长尾**：真实 LLM 会输出全部章的摘要行；mock 单行（证据回显+当前章）已满足 fixture 断言（timeline 非空+kind 齐不依赖摘要行数）。章号取值受 settlerDelta 的「第 N 章」抽取影响（mock PLANNER 文本「# 第 1 章 memo」先于目标章命中→LAST 可能偏差），promises 断言不依赖该值——保真度长尾随需再投。
- audit_drift 差异为审计注入面正常行为（issue 数随注入变化），非仿真缺口。
- 首版「回显 buildSystemPrompt 区块」假设被实证推翻（governed 模式 blocks 全在 user prompt 且头名不同）——临时 dump 一步纠偏。

## 五、教训

1. **回显型 mock 的抽取源要用真实请求 dump 校准**：governed 模式把 truth 区块拼在 user prompt 且头名为「已选XX证据」证据行形态——按源码 buildSystemPrompt 假设写正则两轮空转，dump 一次全清。
2. truth 保真的验收口径=与无循环基线做 **同文件 diff**（650 同族缺口的通识：有审改循环的走查必须对照无循环基线检查 story/ 全目录），单看 promises 断言会漏 chapter_summaries/current_state 的缩水。
3. 「缺 TAG 容忍」的真正语义是「空串覆盖」——parseWriterOutput 的 extract 失败返回 ""，`??` 不兜空串：**容忍解析器不炸 ≠ 容忍数据不丢**。
