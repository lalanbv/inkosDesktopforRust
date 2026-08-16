# 78 号：短篇 runner 与 short_run 接线

**日期**：2026-08-15
**阶段**：Phase3 深度收尾（77 号"下一步"首选：short-fiction runner 主体 + short_run 意图接线——确认面最后一个未接意图落位）
**契约源**：`packages/core/src/pipeline/short-fiction-runner.ts`（1028 行消费面主体 L112-547 + 路径辅助 L986-1028；cover 段 L409-450/L549-579/L887-968 已在 74/76 号移植）、`agents/short-fiction.ts`（533 行：六代理 + tagged-block 解析 + 校验/渲染）、`agent/agent-tools.ts` L1383-1469（createShortFictionRunTool + assertShortRunCharsPerChapter + summarizeCoverGenerationError）、`studio/api/server.ts` L1636-1648（short_run 装配分支）、`interaction/action-envelope.ts`（shortRunCharsPerChapterRange/Error）

---

## 一、背景

77 号后确认意图执行器 11/11 全通，但 short_run 的执行分支仍是 67 号的占位（"Unsupported confirmed action"）。本轮补齐短篇 web 小说生产域本体：可恢复三段链（outline → 全稿 → 包装 + 封面），使 short_run 成为最后一个接入真实 runner 的确认意图。提示词层（13 个 build 函数）已在前序号就绪（`prompts/short_fiction.rs`），本轮接上 agent 调用面与编排本体。

## 二、交付

### 1. agent 面 `engine-rs/src/agents/short_fiction.rs`（新 ~600 行 + 9 单测）

- **六代理 LLM 调用**（router 键 = TS `pipeline.createAgentContext` 名）：create_outline（short-outline，0.55/8192）、review_outline（short-outline-review，0.3/4096）、revise_outline（short-outline，0.45/8192，**四消息轮** system/user/assistant(原稿)/user(followup)）、write_draft（short-writer，0.58/估算 maxTokens）、continue_draft（short-writer，0.68，无缺章直接返回原稿）、review_draft（short-draft-review，0.3/8192）、revise_draft（short-revise，0.45，四消息轮）、generate_package（short-package，0.45/4096）。
- **瞬态重试**（retryShortFictionCall）：2 次尝试，关键词 unexpected eof / econreset / socket hang up / terminated / fetch failed。
- **tagged 块解析**（`=== TAG ===` 逐行扫描，对齐 JS 正则语义）：extract_tagged_blocks（块到下一个通用 tag 行 trim）、extract_last_non_empty_tagged_block（续写合并时**最后非空 CONTENT 胜**）、extract_first_heading、markdown 章节回退（`##` 标题前缀剥离需**数字形态**"第1章"——中文数字不剥，TS 同款）、**重复标题块**（CHAPTER N TITLE 出现 ≥2 次时第二个 tag 后全部内容，含重复标题行自身——TS 逐字）、sanitize_chapter_content（首尾 fence 剥离 + 残留 tag 行移除 + trim）。
- **标题归一**：normalize_title（`#` 剥离 + 《》剥壳 + 首非空行）、normalize_chapter_title（zh `第N章`/en `Chapter N:` 前缀剥离 + fallback）、format_chapter_heading（已有前缀原样；无前缀补 `第N章 `/`Chapter N: `）。
- **校验/渲染**：find_empty_short_fiction_chapters、validate_short_fiction_draft_for_final（章数 + 空章，错误文案逐字 "Short-hit draft is incomplete; ..."）、render_draft_markdown（`# 标题` + `## 开篇钩子` + `## 第N章 标题`）、estimate_short_fiction_max_tokens（`max(12288, ceil(N×字数×2.2)+4096)`——f64 与 JS 同值：12000×2.2=26400.000000000004 → 30497）。
- 类型 serde camelCase（draft.json / short-story.json / sales-package.json 磁盘形态）：charCount/openingHook（None 省略，对齐 JS undefined 不序列化）/sellingPoints/coverPrompt。

### 2. runner `engine-rs/src/pipeline/short_fiction_runner.rs`（新 ~560 行 + 7 单测）

- **入口 `run_short_fiction_production`**：normalizeOutputDir（默认 shorts）→ 稳定 storyId resume 探测（final/full.md 存在且 status 非 failed → **already-complete 直接返回**，零 LLM）→ produceShort → 失败时外层 writeJson `{status:"failed", error}`（**无 updatedAt**，TS 逐字）。
- **produce_short 三段链**：
  1. **outline**：boundedInteger（chapterCount 12-18 / charsPerChapter zh 900-1200、en 600-800，缺省 12/1000/650）→ v001 → 审纲（reviews/outline-v001.md）→ 一次修订（v002）。**storyId 断点续跑**：v002.md 已在且非空 → 跳过 outline 全段（进度 "Resuming from existing outline..."）。
  2. **writer**：整稿 → 缺章补写循环 ≤3 轮（每轮进度 `Completing missing short fiction chapters: N, ...`，partial 落盘 v001-partial）→ validate → v001（full.md + draft.json + chapters/0001.md…）→ 审稿（reviews/draft-v001.md）→ 一次修订：**LLM 失败或 v2 校验失败都降级**——reviews/draft-v002-warning.md（双语逐字"第二轮改稿未采用"）+ finalDraft 保持 v1，**任务不 fail**。
  3. **final + 包装**：final/full.md + `{safeFileName(title)}.md` + short-story.json + chapters/ → 包装代理 → sales-package.json/md（双语标题 `## 简介/卖点/封面提示词`）+ cover-prompt.md（空 → "(empty)"）。
- **status.json 语义**（TS 逐字）：生产段失败 → `{status:"failed", error, updatedAt}`；修订降级但完成 → `{status:"complete", warning:"revision skipped: ...", updatedAt}`；**正常完成不写状态文件**（TS 仅 warning 时写）。
- **封面**：cover===false → coverError "disabled"；否则 resolve_cover_generation_request + build_cover_image_prompt(**short 模式**, 语言) + generate_image_from_prompt → final/cover.{png|jpg}；失败仅 coverError 降级。复用 74/76 号 cover 基础设施。
- **路径辅助**（与 script runner 同名不同细节，本地实现零耦合）：slugify_short（**引号删除**再折叠、`short-{ms}` 回退）、safe_segment_short（**不 lower**——保留用户 storyId 大小写；危险字符逐个折叠、空白段折叠单个 '-'）、safe_file_name（危险字符折叠 '_'、空白归一空格）、write_text（**trimEnd + 单换行**——TS 语义）、write_json（pretty + 尾换行）。

### 3. short_run 意图接线（agent_production.rs 三处）

- **装配分支**：direction 兜底链 `payload.direction.trim() || instruction.trim()`（都空 → 502 "确认短篇缺少方向"）；reference/storyId/chapters/charsPerChapter truthy 展开；**cover 显式 false 也展开**（TS `cover !== undefined` 判定）。
- **tool_name**："short_fiction_run"（TOOL_LABELS 已有"短篇生产"；不在 suppress 名单 → response 为结果文本）。
- **execute_short_run**：语言 = `payload.shortRun.language ?? 会话语言`（TS tool 语义）→ **assertShortRunCharsPerChapter 最终断言**（确认卡层只能做 600-1200 并集校验；zh 会话 + 700 这类越界组合在此以双语错误拦截，经统一错误面 502——与 TS throw-inside-execute 同链：快照 error 态 + tool:end）→ runner → 结果文本逐字（五行：completed/Final/Sales package/Cover prompt/Cover image；无图时三行降级说明含 summarizeCoverGenerationError——503/502/api-key 关键词分支 + 300 截断）；details = `kind: short_fiction_created` + result 全字段。

### 测试

- **lib 单测 16 个（+16）**：agents 9（outline/draft/sales 解析回退链、last-non-empty 胜出、markdown/重复标题/fence 清洗、空章校验与错误文案、渲染形态、heading 归一、token 估算与瞬态判定）+ runner 7（slugify/safeSegment/safeFileName/有界整数文案/package markdown/warning markdown/结果路径 camelCase）。
- **E2E `short78_e2e` 3 个（+3）**：全链（mock 按系统提示词关键词分流六代理；writer 首写缺 12 章 → 补章循环 → v001-partial/v001/v002/final 全树 + short-story.json camelCase + 包装三件套 + cover:false 无 status.json + response 非空文本）；修订降级 + 最终语言断言（revise 只回 1 章 → warning 文件 + complete 状态；zh+700 → 502 双语）；resume（预写 final/full.md → already-complete + **零 LLM 调用**计数断言 + direction 兜底 instruction）。
- **67 号旧断言更新**：`unsupported_intent_and_missing_title_fail_with_502` 中 short_run"未支持"断言已过时 → 改名 `short_run_invalid_draft_and_missing_title_fail_with_502`，断言 mock 无效输出下 502 "Short-hit draft is incomplete"。

## 三、parity 要点

1. **可恢复语义**：稳定 storyId 让二次运行从磁盘续作——已完成（非 failed）直接 already-complete 返回（不重跑、不覆盖）；outline v002 在则跳过三段 outline；任何生产段失败写 failed 状态防半成品冒充成稿。
2. **修订降级不 fail**：v2 不完整（LLM 失败或空章）绝不覆盖完整 v1，warning 文件 + complete 状态（含 "revision skipped: " 前缀）。
3. **charsPerChapter 双层校验**：确认卡层（75 号）按 payload.language 做 600-1200 并集；执行层按最终语言（payload ?? 会话）做分段断言——en 600-800 / zh 900-1200，越界双语错误。
4. **markdown 回退 quirk 固化**：TS 正则前缀可选导致 (a) 章节标题剥离仅识别数字前缀（"第一章"不剥）；(b) extractMarkdownChapterContent 任何 N 都取**第一个 `##` 段**。逐字对齐并测试固化。
5. **writer 续写合并**：rawContent 拼接后重解析，同号 CONTENT 取**最后一个非空块**——补章与重写都以最右为准。

## 四、偏差备案

1. **coverBaseUrl/coverEndpoint/coverModel/coverSize/coverApiKeyEnv 不透传**：TS tool 的这些参数只来自 LLM params（确认卡装配不填），Rust cover 解析走项目配置 + env（INKOS_COVER_*）等价路径；LLM 直呼 params 的覆盖形态随 agent 聊天工具面（暂缓清单）后续评估。
2. **normalizeOutputDir 词法校验差异**：TS 走 safeChildPath("/", normalized) resolve 校验；Rust 用 parts 含 ".." 拒绝（script runner 同款）——确认卡链路 outDir 恒缺省（"shorts"），无行为差异。
3. **"确认短篇缺少方向" 502 分支经 HTTP 不可达**：路由层空 instruction 400 在前（TS 同序），装配分支保留为防御性代码（测试经单测面覆盖兜底链，resume 测试断言 args.direction == instruction）。
4. **markdown 章节内容回退的首段 quirk**（parity 要点 4b）为 TS 固有行为，已逐字保留并固化测试，不视为缺陷修复项。

## 五、暂缓件（沿 77 号清单滚动）

- sceneReconciler + regenerate 变体/checkpoint 面、play en 提示词、play_step 聊天工具
- 单章写作中途截断、/agent model 校验、resumeFrom、fetchWithProxy、attachments 归一化、模型四层解析
- LLM 直呼 short_fiction_run 工具的 params 覆盖面（coverBaseUrl 等）

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **1067** 通过（+16） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **132** 通过（+3，另 1 个旧断言更新） |
| `cargo test --features export-bindings --lib` | **1226** 通过（+16） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：POST /agent 的 short_run 确认卡接入真实生产链（此前为占位错误）；确认意图执行器 11/11 全部指向 Rust 域本体；短篇产物树（shorts/{storyId}/outline|reviews|drafts|final + status.json）Rust 端落盘。
- **下一步（79 号候选）**：sceneReconciler + regenerate 变体/checkpoint 面；或 play en 提示词 + play_step 聊天工具；或散件收尾（单章截断/model 校验/resumeFrom/attachments/模型四层解析）。
