# 141 号（Phase3）：script-storyboard 同事务化——交付校验与产物快照原子提交

日期：2026-08-18 · 前置：137 号（commit_production_artifacts 积木）/ 140 号勘测

## 勘测结论（契约安全）

`status.json` 在 studio API/前端**零消费**（`/api/v1/interactive-films` 列表
读 story-graph、translations 读 manifest）——旧 completedAt 形态 →
ProductionRunSnapshot 形态变更契约安全，纯运维产物升级。

## 实现

### A. 交付校验积木（`src/agents/script_storyboard.rs`，TS 逐字）

- `count_markdown_sections`（TS countMarkdownSections：# 级 1-6 标题匹配
  标题族的节数）。
- `assert_script_deliverable`（TS assertScriptDeliverable：恰好一份人物节
  （zh: 人物/Characters；en: Characters）+ 一份非空剧本正文节——
  extract 的节边界规则为"下一标题级别 ≤ 起始级即截断"（TS 逐字），正文
  节内的更深层标题（###）不截断；拒则**零提交**，错误文案双语逐字）。

### B. 三函数同事务化（`src/pipeline/script_storyboard_runner.rs`）

- `text_artifact`（TS textArtifact：内容保证尾换行）/
  `assert_non_empty_artifacts`（TS assertNonEmptyArtifacts）/
  `commit_production_complete`（三面共用尾：kind 分面 + status complete +
  stage commit + observations 空）。
- **run_script_creation**：交付校验先行 → [script-spec.md, script.md] 同
  事务 + status.json 快照。
- **run_storyboard_creation**：[storyboard-spec.md, storyboard.md,
  image-prompts.md, assets.json] 同事务。
- **run_interactive_film_creation**：八产物同事务——**story-graph.json 并入
  事务**（TS 已移除 saveStoryGraph 独立写；schema 校验保留在提交前），
  assets 三子目录预建保留（目录创建非事务内容）。

### C. 测试（+2 e2e，既有 fixture 适配）

- sub141 同事务成功：spec/script/status 三文件就位、status 为快照形态
 （version/kind script/id/status complete/stage commit/artifacts 两项、
  旧键 completedAt/title 不存在）。
- sub141 拒交零写入：缺人物节 → TS 错误文案逐字 + 三文件零落盘。
- 既有 script76/film77 适配：mock 正文节内 h1 改 h3（新校验的节边界语义
  ——h1 会截断正文节）、kind 断言 interactive-film、status 断言 complete
  + artifacts 数。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1198** 过 |
| `cargo test --test e2e_write_next_contract` | **194** 过（+2） |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1356** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. runner harness 类形态结构对齐（行为面已等价——task-local 作用域/同事务/
   快照/技能注入全套已按 TS 语义承载；纯类形态重构收益待评估）。
2. translation runner 的 run-store 面接入同事务积木（TS run-store.ts +23
   行变更待勘测）。
