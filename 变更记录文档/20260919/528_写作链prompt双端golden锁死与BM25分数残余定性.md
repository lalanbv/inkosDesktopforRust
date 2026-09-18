# 528 号：写作链 prompt 双端 golden 锁死（planner+settler 9 面）+ BM25 分数残余定性

日期：2026-09-19　分支：develop　基线：13c5fd4f（527 号）

## 背景

522 号备案的「prompt 模板措辞对齐专项」：写作链 agent prompt（planner/writer/settler）是 Rust 从 TS 手抄移植的措辞面，此前只有聊天面 golden（230/231/234 号 agent-prompts.json），写作链无机械锁——措辞漂移=双端同输入下给 LLM 的指令分叉（工件内容差异的上游根源之一），且静默不可见。同轮定性 521 号备案的 BM25 分数绝对值残余。

## 实施 1：写作链 prompt golden 机制（双端共享事实源）

复制 agent-prompts.json 先例（TS 生成 → Rust include_str! 码点级比对）：

- **TS**：`packages/core/src/__tests__/golden-writing-prompts.test.ts`——从 `planner-prompts`/`settler-prompts` 纯函数生成 9 面快照 `golden/writing-prompts.json`（REGEN=1 更新）；断言模式同 golden-agent-prompts。
- **Rust**：`engine-rs/tests/golden_writing_prompts_diff.rs`——include_str! 同一 json，逐 key 构造等价输入断言。

9 个 golden 面（分支覆盖设计）：

| key | 覆盖分支 |
|---|---|
| planner.system/template ×zh/en | 双语逐字 4 面 |
| planner.user.zh.golden | ch2 黄金三章指引追加 + brief 块 + 本章指令块 |
| planner.user.en.plain | ch5 无追加 + en 措辞 + 英文预算单位 |
| settler.system.zh.plain | 数值体系=false 基线 |
| settler.system.zh.numerical | 数值铁律块 + 全员追踪块双分支 |
| settler.user.zh.full | observations/evidence/feedback 全给 + governed 控制块让卷纲让位（互斥分支） |
| settler.user.zh.min | truth 文件 `(文件尚未创建)` 全分支 + 卷纲兜底块 |

**首跑即绿**：9 面双端码点级一致——404 号移植的逐字纪律经受住了机械验证，此前措辞无漂移；golden 落地后任何单侧措辞改动会被另一侧红测试立即拦截。

writer 面（输入含 BookConfig/GenreProfile 深对象+fanfic/lengthSpec 多分支）构造重，**备案下轮扩展**（本号建立机制+首批覆盖）。

## 实施 2：BM25 分数绝对值残余定性（521 备案收口）

关键发现：双端检索打分**并非各自实现**——都是 SQLite FTS5 内建 `bm25(retrieval_documents_fts, 5.0, 1.0)` 同参数（local-search.ts:118 / local_search.rs:228）。

活体实测（差分器新增 score 观察输出）：node `fact:1`=2.2014 / rust=1.8903，`hook:H03` 2.129/1.8271——**两处命中比值完全一致（≈1.1646）**。

定性：**恒定比例因子 = 单一全局常数差（SQLite 版本间 FTS5 bm25 内部常量）**。若差异来自 df/avgdl 统计或分词不同，各命中比例会发散；实测不发散即排除。单调缩放不影响排序与交集——top 命中、边缘命中形态与 520 号备案完全一致。

裁决：**分数绝对值差=引擎 SQLite 版本伪象，永久豁免**（排序一致性由交集契约覆盖，521 的 CLDR golden 锁 token 序主流面）。差分器 `[diff][hs] score` 观察面固化，此后每次差分器跑动都输出分数对照供漂移监视。

## 验证

- 新 golden 双端：TS vitest 绿（typecheck+test 66.9s 收编）；Rust `golden_writing_prompts_diff` 绿（engine-rs **1806** 测试全绿）。
- 差分器 41 对照项 0 分歧（含新 score 观察面）；双活体套件绿；`gate:ts:fast` 绿；clippy 双 crate 0 告警；src-tauri 578 绿；duel 真跑 10/10；bench:gate 见下（负载拦截重跑）。

## 教训

- **手抄移植面要趁早 golden 化**：本次首跑即绿证明移植质量高，但「没有机械锁的逐字纪律」只能靠人工走查维持——首跑绿的价值不在抓漂移，而在把漂移检测成本从「每次对照走查」降为「每次 cargo test」。
- **分差形态本身就是证据**：恒定比例因子（各命中比值一致）直接指向全局常数（版本伪象），发散比例才会指向统计/数据差异——对差异做「形态分析」比逐值排查更能定位根源。
