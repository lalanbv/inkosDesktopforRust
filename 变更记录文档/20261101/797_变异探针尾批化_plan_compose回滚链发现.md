# 797 号：变异探针尾批化——plan/compose 触发 preserveFailedWork 回滚链发现

日期：2026-11-01。前置：796 号登记（eval 口径对齐 / promises 存储移植）。本轮诊断把 eval 空态的根因从「计算口径」**重新定性为「探针触发的工作副本回滚」**，并落地差分器变异探针尾批化；eval 口径对齐经逐项比对证实**两侧实现等价**（分歧非计算问题）。

## 一、eval 口径对齐——逐项比对的结论：两侧实现等价

book-eval 的 avgParagraphWarnings 逐项比对（rust book_eval.rs vs TS book-eval.ts）：
- 段落拆分 `\n\s*\n` ✓ 同；短段判定 UTF-16 码元 <35 ✓ 同（JS .length 即 UTF-16 码元）；40% 阈值 ✓ 同；平均除法 ✓ 同；aiTellDensity round2 ✓ 同；parseChapterRange 缺省全量 ✓ 同。
- **结论：计算口径无分歧**——795/794 时代观察到的 eval 空（totalChapters 0）是 node 腿书目状态被清空后的**忠实投影**，非计算差异。

## 二、真根因链（--keep 现场取证 + 活体分离实验）

1. **--keep 现场**：diff node 根 works/镜花水月/source/chapters = 仅 index.json `[]`、章文件零、state rows []——而 books/镜花水月（迁移源）完好（0001+index 1 行）。
2. **变异探针定位**：共享面 `POST /plan`、`POST /compose` 在 node 腿走 CreativeEpisode 机制 → `Unknown creative episode: 镜花水月`（server log 实锤）→ 失败路径 `preserveFailedWork` → `syncWorkSourceArtifacts(accept:false)` → **工作副本回滚到上一接受修订**——写链刚产出、尚未被修订机制 accept 的第 2 章（及 index/state）被整个回滚。
3. **活体分离实验**：/tmp/wtest7 上按 diff 探针序单打 plan/compose/consolidate → 不复现（该书 3 章、状态全在）——回滚需要「写链产出未被 accept」的前提，diff 环境的 write-next 后状态恰好满足。
4. **探针序实验**：reject/approve 移表尾后 eval 仍空 → plan/compose 是第二个变异源（reject 第一个，797 早前实锤 node reject 后章节清空）。

## 三、修复：变异探针尾批化（差分器）

- approve/reject×2（797 早前）+ **plan/compose**（本轮）→ 全部移共享面**表尾**（draft 防污染同款先例）——其后无 GET 基线可污染。
- repair-state 留中段（500 护栏探针非变异）；consolidate 留中段（幂等零归档）。
- 复跑：共享面 71 对比 / 6 分歧（对比项 +3=approve/reject×2 移尾后计入）——**tension-curve 保持收敛**（796 链 diff 实证）+ eval/promises/quality-trend/roster/detect/reject 六项分歧=**回滚语义差 + 存储移植**两类（见下）。

## 四、六分歧定性（695 工项活体清单更新）

| 分歧 | 定性 |
|---|---|
| eval 空态 / reject node 404 | **plan/compose→preserveFailedWork 回滚链**（node 腿 episode 未注册）+ reject 回滚后章节清空——回滚语义差 |
| promises / quality-trend currentChapter | node 腿 promise ledger/trend 派生未接线（795 定性维持） |
| roster-candidates 0 vs 1 | 候选生成移植 |
| detect 404 | 检测存储移植 |

前置阶梯现状：写链双腿全通（795）+ tension 收敛（796）+ state 应用（794）——剩余全部收敛于 **episode 注册/回滚语义** 一族。

## 五、教训

1. **「计算口径对齐」的预判要被比对结果推翻**——逐项比对证明两侧等价后，分歧必在输入侧（书目状态被清空）；预判定性是假设，比对才是裁决。
2. **差分探针本身就是状态变异源**——POST 探针（plan/compose/approve/reject/draft/consolidate）各有副作用面；GET 基线对比必须先于全部变异探针，变异探针集中表尾（draft 先例的通用化）。
3. **--keep 现场 + 双布局对照**（books vs works）是定位「写链产物消失」类问题的最快路径——文件级证据胜过全部推理。
