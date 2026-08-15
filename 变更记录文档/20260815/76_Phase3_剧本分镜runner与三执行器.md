# 76 号：剧本/分镜 runner 与三执行器

**日期**：2026-08-15
**阶段**：Phase3 深度收尾（75 号"下一步"首选：script-storyboard 域本体 + script_create/storyboard_create 双执行器 + generate_cover 顺带接线——**确认意图 10/11**）
**契约源**：`packages/core/src/pipeline/script-storyboard-runner.ts`（717 行消费面）、`agents/script-storyboard.ts`（639 行：双代理提示词 + spec 渲染 + 提取函数）、`pipeline/short-fiction-runner.ts` L409-L450 + L549-L579 + L887-L968（generateShortFictionCover / generateCoverImageArtifact / buildCoverImagePrompt / normalizeSellingPoints）、`agent/agent-tools.ts` L1533+/L1860+（createScriptCreationTool / createStoryboardCreationTool / createGenerateCoverTool）

---

## 一、背景

75 号后剩余确认意图 4 个，其中三个可在本轮接通：script_create/storyboard_create 依赖 script-storyboard 域本体（此前未移植）；generate_cover 依赖 74 号已交付的 cover 基础设施（纯接线 + 补封面提示词双模式）。完成后确认意图 10/11，仅剩 interactive_film_create（依赖互动影游全链 story graph 生成）。

## 二、交付

### 1. 剧本/分镜代理 `engine-rs/src/agents/script_storyboard.rs`（新 ~660 行 + 4 单测）

- **spec 渲染双语逐字**：renderScriptSpec（目标/集数/时长/素材 + 改编边界四条——"不替用户决定改编强度/内心戏转可演/短剧每集钩子"）与 renderStoryboardSpec（粒度/画幅/风格/上限 + 分镜边界四条）；summarizeSourceForSpec（空白折叠 + UTF-16 字符数标注）。
- **双代理提示词 zh/en**：script-creation-writer（0.55，maxTokens 按集数估算 clamp 12k-32k）与 storyboard-creation-writer（0.45，按镜头数 clamp 10k-24k）；system（"剧本创作工具不是小说续写器"/"分镜不是剧情摘要"）+ user（spec + 完整源素材/占位 + 输出格式——竖屏短剧"第N集/场次/人物/动作/对白/集尾钩子"、分镜"镜号/画面/…/备注" + 图像提示词必须独立 `Prompt: ...` 行）。
- **Markdown 提取面**：extractMarkdownSection（标题归一——**加粗剥壳/杂符剥离/空白折叠/lower + 前缀匹配容忍分隔符）；parseStoryboardPromptLines **三形态**（`Prompt:`/`提示词：` 行 + Markdown 表格 prompt 列——表头定位/分隔行跳过/列提取 + 编号行兜底）；extractStoryboardImagePrompts（小节提取 + 1. N 编号化）；normalizeScriptEpisodeEndLabels（"第N集"标题对齐"字幕：第N集完"——集尾错位标签按当前集修正）。

### 2. runner `engine-rs/src/pipeline/script_storyboard_runner.rs`（新 ~380 行）

- **runScriptCreation**：projectId（safeSegment 80 上限/slugify 回退）→ spec 落盘 → LLM 剧本（集尾标签归一）→ script.md + status.json（completed/kind/title）。
- **runStoryboardCreation**：spec → LLM 分镜 → storyboard.md + image-prompts.md（提取编号化）+ assets 三目录（source/generated/selected）+ **assets.json manifest**（version/kind + shot-001… 资产条目 prompt_ready + 三子目录路径）+ status.json。
- 路径辅助逐字：safeChildPath（词法防逃逸）/writeProjectText（补尾换行）/mergeRequirements（"补充要求："拼接）/normalizeOutputDir/resolveProjectBaseDir（basename==projectId 判定）/relPath posix。

### 3. generate_cover 全链（cover.rs 扩展 + 2 单测）

- **buildCoverImagePrompt 双模式双语逐字**：generic（提示词文件——"按用户给出的标题、简介、卖点和视觉要求生成封面图。"）与 short（生成图片——"3:4 竖图 + 封面方向平台短篇书封 + 高对比高饱和 + 文字不稳定时留白排版区"六段）；normalizeSellingPoints（`;；\n` 拆分）。
- **generateShortFictionCover**：title 必填 → outputDir 默认 `covers/{safeSegment(title)}` → cover-prompt.md（generic）→ 生成（**short 提示词**——TS 两处模式分别调用的语义）→ cover.png/jpg → `{title, outputDir, coverPromptPath, coverImagePath}`。

### 4. 三执行器接线（agent_production.rs）

装配分支（缺 title → 502 中文逐字）+ 执行分支 + tool 名（script_create/storyboard_create/generate_cover）。结果文本：script 四行 / storyboard 五行 / cover 三行逐字；details kind（script_project_created/storyboard_project_created/cover_generated）+ 全路径。**确认意图 10/11**。

### 测试

- **lib 单测（6 个）**：markdown 节提取（加粗/杂符/无匹配/回退）、prompt 行三形态（Prompt:/表格列/编号 + 编号化）、集尾标签对齐、spec 渲染双语形态、卖点归一、封面提示词双模式。
- **E2E `script76_e2e`（3 个）**：script_create 全链（spec/script/status 落盘 + **集尾标签归一 E2E 断言** + 缺 title 502）；storyboard_create 全链（image-prompts 编号化 + assets manifest shot-001/002 prompt_ready + 三目录 + 缺 title 502）；generate_cover 全链（mock 生图 → prompt generic 落盘 + png + 响应三行 + 缺 title 502 + **未配置 cover → 502 "cover endpoint is required"** 经统一错误面）。

## 三、parity 要点

1. **提示词双调模式**：cover-prompt.md 写 generic（用户可读的原始意图），生成图片用 short（营销封面方向）——TS 两处调用同模式。
2. **集尾标签对齐**：LLM 常把"字幕：第N集完"写成错误集数——按当前"第N集"标题归一（normalizeScriptEpisodeEndLabels）。
3. **图像提示词独立行契约**：`Prompt: ...` 单独成行（不混表格/正文）——资产管理按行提取。
4. **表格 prompt 列定位**：表头含"提示词/Image prompt"列 → 后续行取同列（表格式分镜输出兼容）。

## 四、偏差备案

1. **interactive-film 代理与 runner 暂缓**（runInteractiveFilmCreation：剧情树/旗标/多结局/互动剧本/分镜五节 + story graph 生成链）——77 号，完成后 interactive_film_create 接线（最后一个意图）。
2. **appendPromptPackGuidance 未接**（沿 74 号备案）。
3. **normalizeSellingPoints 数组形态**：TS 支持 string | string[] 双入参；Rust 只接 string（`;；\n` 拆分）——actionPayload strict 校验下 sellingPoints 就是 string。
4. **slugify/safeSegment 输出序**：与 TS 一致按插入序/键序，非排序（此处无 Map 序差异）。

## 五、暂缓件（沿 75 号清单滚动）

- interactive_film_create（最后一个确认意图——互动影游五节创作链 + story graph 生成）
- short_run（short-fiction runner 主体）
- sceneReconciler + regenerate 变体/checkpoint 面、play en 提示词、play_step 聊天工具
- 单章写作中途截断、/agent model 校验、resumeFrom、fetchWithProxy、attachments、模型四层解析

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **1045** 通过（+6） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **127** 通过（+3） |
| `cargo test --features export-bindings --lib` | 1204 通过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：POST /agent 的剧本/分镜/封面确认卡全通（衍生创作三域 Rust 端可用）；**确认意图 10/11**。
- **下一步（77 号候选）**：interactive_film_create（互动影游五节创作代理 + story graph 生成链 + 最后一个意图接线——完成后 11/11 确认意图全通）；或 short_run（short-fiction runner 主体 ~588 行 + 意图）；或散件（单章截断/model 校验/resumeFrom/attachments）。
