# 143 号（Phase3）：翻译主链快照——运行生命周期接线

日期：2026-08-18 · 前置：142 号（save_translation_progress 积木）

## 勘测结论

合并对 `translation/runner.ts`（+109 行）的变更实质 = 翻译主链接入生产
运行快照**三点生命周期** + 章级同事务进度：
1. **开跑**：`translations/{id}/status.json` running 快照（stage translate，
   artifacts=[manifest, glossary]）。
2. **章级进度**：saveTranslationGlossary + saveTranslationChapter 两次写 →
   `saveTranslationProgress` 同事务单点 + running 快照更新
  （artifacts 累加章译文路径、resumeCursor="{chapter}:{已处理段数}"）。
3. **完成**：review-report.md 与 complete 快照**同事务**（artifacts=base+
   全部章译文+报告；stage complete）；**失败**：failed 快照（error 消息，
   写失败不吞原错）。

## 实现（`src/translation/runner.rs`，TS 逐字）

- 主函数拆双层：外层发布 running 快照 → `run_translation_inner` →
  Ok/Err 分支（complete 由内层 commit、failed 由外层发布）。
- 批循环：双写改 `save_translation_progress`（142 号）+ 每批后 running
  快照（resumeCursor 累计 processed 段数）。
- 尾部：报告与 complete 快照 `commit_production_artifacts` 同事务
 （137 号），artifacts 全量清单。
- 既有单写函数保留（TS 未删）。

## 测试（+2）

- 生命周期：complete/stage complete/artifacts 四项（base+章+报告）/
  error 省略（章译文路径在 translated/ 子目录——勘测修正）。
- 失败路径：模型 Err 上抛原文 + failed 快照落地（stage translate +
  error 消息逐字）。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1202** 过（+2） |
| `cargo test --test e2e_write_next_contract` | **194** 过 |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1360** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

runner harness 类形态结构对齐（行为面已等价——task-local 作用域/三类同
事务/快照生命周期/技能注入全套已按 TS 语义承载；纯类形态重构收益待评估，
当前无未移植的行为面漂移）。
