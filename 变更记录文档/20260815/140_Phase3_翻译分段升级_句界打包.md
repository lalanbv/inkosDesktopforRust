# 140 号（Phase3）：翻译分段升级——句界打包与词界递归

日期：2026-08-18 · 前置：合并 d3ba425e 漂移勘测（translation/script-storyboard）

## 勘测结论

合并对 translation 面的变更两类：
1. **llm-model.ts**：chatCompletion → runWorkerAgent + 技能指导注入
  （139 号 OPERATION_SKILLS 机制已等价覆盖，无需重复接线——translation
   runner 的 Rust 移植届时自然继承 router 出口注入）。
2. **text.ts 分段算法升级**（本路面）：段落超限时由**硬切 maxChars** 升级为
   **句边界贪心打包 + 超长句按词边界递归打包**——杜绝句中截断。

## 实现（`src/translation/text.rs`，TS splitParagraph/packBoundaryUnits 逐字）

- `split_long_paragraph` 重写：`unicode_sentences()`（UAX #29 句边界）分句
  过滤空白 → `pack_boundary_units`。
- `pack_boundary_units` 逐字：贪心打包 units 到 ≤maxChars；超长 unit 先落
  当前缓冲再按 `split_word_bounds()`（词边界）递归；单词不可再分则整段
  直出；尾 trim 过滤空。
- 依赖 += `unicode-segmentation`（UAX #29——TS `Intl.Segmenter` 的
  locale 无关对应物；locale 细化规则差异备案）。

## 测试（+3）

- 长段句界打包：片段全部 ≤maxChars 且以句末标点收尾（**不句中截断**）、
  拼接无损。
- 超长句词界递归（英文空格词界拆包）；连续中文按词界（≈逐字）递归拆包
  （UAX 与 Intl.Segmenter 同行为）；单个超长无界 token 直出。
- segment_translation_text_vec 集成同语义。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1198** 过（+3） |
| `cargo test --test e2e_write_next_contract` | **192** 过 |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1356** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. **141 号**：script-storyboard runner 同事务化——TS 把三 run 函数的分散
   落盘升级为 commitProductionArtifacts（137 号积木）+ 交付校验
  （assertScriptDeliverable：恰好一个 Characters 节 + 一个非空 Script 节，
   拒则零提交）+ status.json 从旧 completedAt 形态改为 ProductionRunSnapshot
   形态。**契约敏感**（studio 前端消费 status.json 形态），独立轮核对消费面
   后实施。Rust 侧为完整移植（900 行三函数）但落盘仍旧形态。
2. runner harness 类形态结构对齐（行为面已等价，纯结构重构收益待评估）。
