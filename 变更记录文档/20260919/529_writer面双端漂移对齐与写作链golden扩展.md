# 529 号：writer 面 section 漂移对齐 + 写作链 golden 扩展（writer 6 面）

日期：2026-09-19
提交：本文件同批路径限定提交
类型：fix(engine) + test(golden)

## 起因

528 号备案的"writer prompt golden 扩展"开工时，通读双端 writer-prompts 源码即发现
**section 级漂移**：Rust `build_writer_system_prompt` 的 zh/en 序列比 TS 多注入 6 个
大段（核心规则 / 写作铁律卡 / 文笔执行 / 创作宪法 / 代入感六支柱 / 黄金开篇特殊指令），
且 Rust 文件头注释声称移植自"1057 行"的 TS 文件，而当前 TS 仅 578 行。

## 考古定案（git 证据链）

- `0783173c`（2026-04-08，v10）：TS 加入 writing craft card 等 6 段；
- `f407c275`（2026-08-14 13:06）：Rust 全量移植 writer-prompts 三件套，移植时 TS 确实
  含这 6 段——**移植本身忠实**；
- `c56586ec`（2026-08-14 13:41）：TS "unify pi harness retrieval and skills" 重构，
  writer-prompts.ts -483 行——6 段从 system prompt 硬编码序列删除，craft 方法浓缩进
  `inkos-long-writing` skill，经 pipeline runner 的 `appendActivatedSkillGuidance`
  激活通道按需注入（PRODUCTION_SKILL_IDS.longWriting）；
- Rust 停留在重构前形态，**35 分钟的窗口差导致此后每次 writer 调用多注入数千 token**
  且措辞与 TS skill 通道分叉。

裁决：以 TS 当前形态为唯一事实源（与 165 号"切后报错先双端对照 Node 行为"、480/481
号双端对照审计同一纪律），Rust 删除对齐。

## 改动

### engine-rs/src/agents/writer_prompts.rs（1346 → 1002 行）

- zh 序列删 6 个调用（core_rules / craft_card / prose_execution_rules /
  creative_constitution / immersion_pillars / golden_chapters_rules），
  en 序列删 5 个调用（同上减 golden_chapters_rules）；
- 删除 6 个函数定义与分节注释、`build_english_core_rules` import、
  `golden_chapters_rules_zh_three_en_five` 单测；
- 对应单测断言反转为"不再注入"（防回潜）；头注释记录 529 考古结论；
- `src/agents/mod.rs` 段数注释同步（zh 16 段 / en 15 段调用项）。

### engine-rs/src/agents/en_prompt_sections.rs（245 → 64 行）

TS 侧该文件现仅存 `buildEnglishGenreIntro` 一个导出。Rust 侧
`build_english_core_rules` / `build_english_anti_ai_rules` /
`build_english_character_method` / `build_english_pre_write_checklist` 四函数在 TS
已不存在且 Rust 零引用（纯死代码），随本号删除，含 4 个对应单测。

### writer golden 双端 6 面锁死

- `packages/core/src/__tests__/golden-writing-prompts.test.ts`：writer 6 案例面——
  生产主形态（zh+creative+governed, ch6）、黄金开篇（ch2 纪律段）、英文书（en 序列 +
  words 单位）、legacy+full 库兼容面、numerical+fullCast+主角铁律+人称硬约束全家桶、
  fanfic 三段（canon + 允许偏离）；`lengthSpec` 一律不传 → 顺带锁死
  `buildLengthSpec(3000)` 默认推导面；
- `golden/writing-prompts.json`：9 键 → 16 键（REGEN 生成，settler.user.zh.min 仅
  行尾逗号形态变化、内容零变化）；
- `engine-rs/tests/golden_writing_prompts_diff.rs`：6 面 Rust 镜像断言，
  `WriterSystemPromptInput` struct 形态（TS 13 位置参数 → Rust struct）。

**首跑即绿**：6 面码点级一致。再次验证 528 号结论——手抄移植质量本身可靠（404 号
逐字纪律），漂移全部出在"TS 侧后续演进 Rust 未跟进"这一类。

## 立案（非本号范围）

Rust 写作链无 skill 激活通道（TS 经 runner.ts:2987/3114
`appendActivatedSkillGuidance` 注入 `inkos-long-writing` craft 方法）。删除 6 段后
Rust writer 的 craft 指导依赖此通道补齐——移植立案，含 skill registry +
production bindings + runner 接线三件套，后续号专项。

## 门禁（8 项全绿）

| 门禁 | 结果 |
| --- | --- |
| cargo test engine-rs | 1801 passed / 0 failed（1806 − 5 个死函数测试，账目吻合） |
| cargo test src-tauri | 578 passed / 0 failed（与 528 基线一致） |
| clippy:gate（--all-targets 双 crate） | 0 告警 |
| gate:ts:fast | typecheck 19s + test 69.3s + audit:npm 全绿 |
| 契约差分器（活体） | 41 对照 0 分歧；内容面零备案硬门禁绿；BM25 score 观察面恒定比例因子 1.1646 复现 |
| node-fallback-smoke | 双引擎一致性全部通过 |
| export-epub-smoke | EPUB 结构校验通过 |
| INKOS_DUEL=1 strangler_duel | 10/10 真跑 |
| bench:gate | 负载守卫三次拦截（用户应用负载 23-29%/核，WindowServer/ZCode 渲染为主）后于低谷窗口（2.04=11%/核）通过，9 基准全部噪声区间零回退 |

## 教训

- **golden 首跑即绿的两种含义要分清**：planner/settler（528）证明"移植没抄错"，
  writer（本号）证明"对齐后一致"——若 528 号就把 writer 面塞进 golden，首跑会红，
  漂移当轮即可发现。golden 覆盖面每延后一轮，漂移潜伏期就多一轮；
- **移植提交与被移植仓库的演进竞态**：35 分钟窗口差可造成 section 级漂移并潜伏
  两周。凡"对齐 TS 当前形态"的裁决，锚点是 git 考古证据链（提交时刻 + 内容 diff），
  不记忆推测；
- bench:gate 负载守卫拦截时，先 `ps aux | sort -rk3` 辨明负载来源：本会话遗留进程
  应清理，用户应用（ZCode/WindowServer/微信）只能等窗口。轮询 `sysctl -n vm.loadavg`
  挤低谷比盲等有效。

## 下一号

自 530 起（开号前双查当日目录最大号）。备案候选：Rust 写作链 skill 激活通道移植
（三件套）、writer prompt pack 层（withPromptPackGuidance）golden 另案。
