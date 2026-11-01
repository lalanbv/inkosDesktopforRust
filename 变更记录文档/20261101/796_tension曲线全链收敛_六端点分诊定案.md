# 796 号：tension-curve 全链收敛——六端点分诊定案（695 工项第一端点清偿）

日期：2026-11-01。前置：795 号报告模式共享面 68/6=派生数据 worklist 显形化。本轮清偿第一端点（tension-curve）并对其余五端点完成**根因分诊定案**。

## 一、tension-curve 三层修复链（活体实证）

1. **TS schema 张力字段缺失**（剥离点①）：`ChapterSummaryRowSchema`（runtime-state.ts）8 字段 strict——settle delta 的 conflictLevel/revealLevel 进 state 前被剥。修复：补 optional 两字段（rust `ChapterSummaryRow.conflict_level/reveal_level` Option+skip 序列化同语义；1-10 约束镜像；旧摘要无此键完全兼容）。
2. **settle 工具 schema 缺张力**（剥离点②）：`ChapterSummaryToolSchema`（settler-tool.ts）同 8 字段——settle 工具 args 的张力在 decode 时被剥。修复：`Type.Optional(Type.Integer({minimum:1,maximum:10}))` 双字段。
3. **writer 显式映射丢张力**（剥离点③）：writer.ts settle 的 delta 构造逐字段挑 8 个——修复：透传 conflictLevel/revealLevel（undefined 时 zod optional 丢键=8 列形态，rust skip 序列化同语义）。
4. **投影渲染 8 列硬编码**（剥离点④）：`renderChapterSummariesProjection`（state-projections.ts）——修复：无条件 10 列（镜像 rust `render_chapter_summaries_projection` R2/359 号；缺分空单元格，旧表消费者无感）。
5. **handler 路径硬编码 books/**（剥离点⑤——第三层根因）：TS tension 处理器读 `books/<id>/story/chapter_summaries.md`（迁移前静态副本）而 2.0 腿活书在 `state.bookDir(id)`=works/<id>/source——修复：切换活书数据面。
- 活体实证：write-next 后 markdown 第 2 章行 10 列带 `6|4`、tension-curve 3 点全带张力值（含探针双写的第 3 章——真实流 2 章）。

## 二、其余五端点分诊定案（工程量与形态各异，逐项登记）

| 端点 | 根因定性 | 形态 |
|---|---|---|
| promises | node memory.db 仅检索表（retrieval_documents，无账本 schema）——**TS 腿无 promise ledger 写入器**（rust settle→memory.db 投影）；且 handler 亦硬编码 books/ 路径 | **存储功能移植**（promises 投影从 hooks state 重建或 ledger 写入器移植）+路径 |
| quality-trend | currentChapter 0 同源（audit 历史存储面） | 同 promises 族（历史存储移植） |
| roster-candidates | candidates 0 vs 1（rust 从 roles 生成候选） | **候选生成逻辑移植** |
| eval | avgParagraphWarnings 计入口径差（book-eval 计算，非存储） | **计算口径对齐**（book-eval 逐项比对） |
| detect/:num | node 404=无检测结果存储（rust 检测写 memory.db/文件） | **检测存储移植** |

## 三、验证

- 活体：/tmp/wtest 端到端（write-next → markdown 10 列 → tension-curve 3 点带张力）。
- core vitest **979/979**（state-projections 测试更新为 10 列断言+空单元格形态）+ tsc 三包归零。
- clippy 双 0 + testgate 1956+584 零漂移 + gate:ts 八步全绿（含 legacy 通道 11/11、报告模式 68/6=登记 worklist）。
- 根级 `npx vitest run` 131 败=根配置环境伪象（794 时即同型非本轮回归；权威口径 `pnpm -r test` 全绿）。

## 四、教训

1. **同一分歧五层剥离链**：schema→工具 schema→显式映射→渲染→路径——每层独立剥一次张力字段；「修了 schema 就好」的假设在显式映射架构下必错，逐层活体验证是唯一路径。
2. **rust 注释是移植规范书**：`Option+skip_serializing_if 对齐 TS undefined 丢键语义`——rust 侧注释直接给出了 TS 侧的正确形态（optional+undefined 丢键），照抄即对。
3. **分诊先分形态**：六端点三种形态（路径可修/存储移植/计算口径）——统一「收敛」承诺不诚实，逐端点定性后工程量才可估。
