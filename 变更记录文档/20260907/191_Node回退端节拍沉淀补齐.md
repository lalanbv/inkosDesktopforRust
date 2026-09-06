# 191 号：Node 回退端时间线节拍自动沉淀补齐（与 189 号 Rust 对齐）

- 日期：2026-09-07
- 分支：develop
- 关联：189 号（Rust 节拍沉淀，本批对齐目标）、181 号（timeline 数据面）、174 号教训（测试传参 ≠ 生产传参）

## 一、变更内容

### 背景

189 号落地节拍自动沉淀时声明了已知差异：「Node 回退端仅端点对齐，Node 管线不沉淀」。按绞杀者模式双端行为对齐原则，本批补齐 Node 侧——开关打开后，Node 回退引擎（sidecar 模式）与 Rust 引擎行为一致。

### packages/core

1. **`pipeline/timeline-settle.ts`**（新模块，与 Rust `agents/timeline_settler.rs` + `pipeline/timeline_settle.rs` 逐字对齐）：
   - `buildBeatsPrompt`：中英双语 system/user，名册、章号、JSON 形状与 Rust 完全一致；
   - `parseBeatsJson`：剥围栏、过滤未知 plotline id 与空节拍、结构性垃圾抛错；
   - `mergeChapterBeats`：原位替换/章号升序插入、未知线条忽略、空 title/note 净化、有落格刷新 updatedAt（ISO）；
   - `settleTimelineBeatsForChapter`：读 `story/timeline.json`（缺失→null 静默跳过；坏载荷→抛错交调用方告警）→ 线条名册 → 端口提取 → 合并 → 落盘（pretty + 尾换行，与 Node timeline PUT 同风格）。
   - 端口 seam：`TimelineBeatsChat` 接收 req 返回原始文本——生产接 `chatCompletion`，测试注入假 chat（不 mock 重型 provider 内部）。
2. **`pipeline/runner.ts`**：`_executeNextChapterLocked` 末尾（pipeline-complete webhook 之后、return 之前）新增沉淀钩子：
   - 仅当 `book.writing?.autoTimelineBeats === true`（默认关，零行为变化）；
   - chat 经 `agentCtxFor("inspiration", bookId)`（与 Rust "inspiration" 档位同语义；用户 per-agent 模型覆盖可生效）；
   - `chatCompletion(client, model, messages, { temperature: 0.3, maxTokens: 1500, signal })`；
   - 成功/跳过 info、失败 warn（`[timeline]` 前缀），**失败不影响章节产物**；
   - 成本：名册 + 章节梗概（复用 settler 已产出的 `persistenceOutput.chapterSummary`），maxTokens 1500。

### 测试

3. **`__tests__/timeline-settle.test.ts`**（新，7 用例）：prompt 双语、parse 容错/过滤/垃圾抛错、merge 替换/排序/净化/未知 id/updatedAt、settle 写入+名册透传、静默跳过（不发起 LLM）、坏载荷抛错、LLM 失败透传且不改写原文件。
4. **`__tests__/pipeline-runner.test.ts`**（+2 接线用例，沿既有全链 mock 基建）：
   - 开关开：writeNextChapter 后 timeline.json 落入节拍 cell，beats prompt 携带名册（按 prompt 形态过滤断言——写路径中另有既有 chatCompletion 调用，全量计数不可用）；
   - 默认关：零 beats 形态调用，时间线未被改写。

## 二、验证

| 门禁 | 结果 |
| --- | --- |
| core vitest 全量 | 196 文件 1869 用例全绿（+9） |
| core typecheck | 干净 |
| studio vitest 全量 | 87 文件 777 用例全绿（本批零 studio 改动，回归确认） |
| studio typecheck（双 tsconfig） | 干净 |
| duel（INKOS_DUEL=1） | 10/10（Node sidecar write-next 默认关不触发沉淀，无契约分歧；Rust 侧零改动故 cargo 门禁沿用 190 号结果） |
| 真机 E2E | 沿用 189 号 Rust 侧结果（行为逐字对齐 + 同参数温度/maxTokens；Node 侧与 Rust 共用同一 timeline.json 读写面） |

## 三、影响面与备注

- **默认行为零变化**：不开启开关时 Node/Rust 管线均零新增调用（有接线测试防护）。
- 双端现在行为一致：开启 `writing.autoTimelineBeats` 后，无论默认 Rust 引擎还是 Node 回退端，write-next 完成都会沉淀节拍。
- 190 号教训沿用：merge 模式回填与节拍沉淀互不相干（不同文件）；时间线写入与 UI PUT 同一 schema 校验面。
