# 525 号：工件对照升级备案快照断言——settle 摘要表优先级对齐 + 噪声归一化三类消除

日期：2026-09-18　分支：develop　基线：4cc8297f（524 号）

## 背景

522 备案清单 3 类差异的精确化与工件对照的机制化：把「信息性输出」升级为**备案清单快照断言**——已定性差异精确登记（清偿后移除），未备案的新分歧硬拦截（divergences 计数）。

## 备案内差异逐项精确化（--keep 现场解剖）

| 项 | 定性 | 处置 |
|---|---|---|
| `runtime/chapter-0002.plan.md`（2427B 同长） | 内容 100% 相同，唯双腿 tmp root **绝对路径嵌入**——环境噪声 | 归一化：root/tmpdir 替换占位符，**消除** |
| `runtime/chapter-0002.rule-stack.yaml`（764/688B） | 语义等价，**YAML 序列化格式**（TS 两空格缩进列表 vs serde_yaml 顶层） | 归一化：逐行 trim 规范化，**消除** |
| `chapter-0002.intent.md`（2B） | **实质**：摘要表 ch1 行冲突/揭示强度列 TS=6\|4、Rust 空 | 修复（见下） |
| `story/state/hooks.json` 及 snapshots ×2（19B） | serde 序列化含空字段（`status_raw:""` 等），TS 丢键——形状差异 | 快照断言捕获补登记 |
| `0001` 尾换行 1B | fixture 直写形态 | 登记备案 |

## 实质修复（engine-rs/src/agents/writer.rs）

`settle_chapter_state` 的 `chapter_summary` 组装**优先级反了**：TS（writer.ts）优先 `runtimeStateArtifacts.chapterSummariesMarkdown`（10 列全量摘要表，含冲突/揭示强度），`renderDeltaSummaryRow`（8 列单行）仅兜底；Rust 直用 8 列单行 → resync 产物摘要表缺两列。对齐为 artifacts 优先、单行兜底。

**探查过程**（5 层链）：mock SETTLER delta 带 conflictLevel 6/4 → serde 反序列化正常（rename_all camelCase）→ reducer `apply_summary_delta` clone 无损 → 渲染层 `render_chapter_summaries_projection` 10 列正常——最终定位在**组装优先级**。静态逐层排查 + 保留现场 diff 是定位主手段。

## 快照断言机制

- `ALLOWED_ONLY_NODE`/`ALLOWED_CONTENT_DIFFS` 备案清单：命中 → 信息输出（清偿后移除）；**未命中 → divergences 计数硬拦截**，提示先定性登记或修复。
- 机制首次运行即捕获漏登记的 `story/state/hooks.json`（主 state 目录）——断言有效性实证。

## 验证

- 差分器 **41 对照项 0 分歧**；工件备案精确化（3 类噪声归一消除：plan root 路径/rule-stack yaml 格式/index 时间戳；4 项登记：intent tension 列、hooks.json ×3、0001 尾换行、resync 留痕 ×3）。
- engine-rs 全量 cargo test **1805 绿**；`pnpm clippy:gate` 双 crate 0 告警；双活体套件绿；`gate:ts:fast` 全绿；`pnpm bench:gate` 零回退（守卫两轮拦截后回落重跑）。
- TS 侧零改动。

## 关联

- 522/524（工件面与实测优先原则）、521（分词同源——本号探查的相邻面）、519（resync 留痕裁决）、487（golden 差分体系）。
- 后续候选：intent tension 列注入链专项（delta→rows 的 tension 传导）；prompt 模板措辞对齐（golden_agent_prompts_diff 扩展）。
