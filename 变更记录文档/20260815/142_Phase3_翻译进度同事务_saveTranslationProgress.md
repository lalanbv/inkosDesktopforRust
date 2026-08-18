# 142 号（Phase3）：翻译进度同事务——saveTranslationProgress

日期：2026-08-18 · 前置：137 号（commit_atomic_file_set 积木）/ 141 号备案

## 勘测结论

合并对 `translation/run-store.ts` 的 +23 行变更实质 = 新增
`saveTranslationProgress`：**章译文 + 术语表同事务原子双写**
（commitAtomicFileSet：`{chapterPath}` 章JSON + `translations/{id}/
glossary.json`，均 pretty + 尾换行）——翻译进行中的两个真值文件要么都在、
要么都不在。这是 137 号同事务积木在 translation 面的消费点。

## 实现（`src/translation/run_store.rs`，TS 逐字）

- `save_translation_progress(project_root, project_id, chapter_path, chapter,
  terms)`：glossary 相对路径经既有 `to_posix_path` 构造；术语先
  `merge_glossary_terms`（trim+lower 去重、后写覆盖）再落盘。
- 既有 `save_translation_chapter`/`save_translation_glossary` 单写函数
  保留（TS 未删——独立调用面仍在）。

## 测试（+2）

- 同事务双写：章与术语表同就位（pretty + 尾换行）；术语去重单条且后写
  覆盖（"Patrol" 覆盖 "patrol"，保留后写原文形态）。
- 越界路径拒收零写入（`../escape.json` → Err 且 translations 目录不创建）。

## 事故记录

测试插入脚本误把 tests 追加到文件尾函数 `to_posix_path` 之后（该文件本无
tests mod），恢复时误加分号破坏尾表达式——git checkout 干净重做（新建
独立 `progress_tests` mod），教训：追加前先确认目标文件真实结构。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1200** 过（+2） |
| `cargo test --test e2e_write_next_contract` | **194** 过 |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1358** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. translation runner 主链接入 save_translation_progress（TS runner.ts 的
   +109 行中章级进度调用点——需与 runner 移植状态联动评估）。
2. runner harness 类形态结构对齐（行为面已等价，纯重构收益待评估）。
