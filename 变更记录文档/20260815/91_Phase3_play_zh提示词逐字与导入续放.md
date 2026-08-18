# 91 号变更记录：play zh 提示词逐字 + import resumeFrom 续放（两项备案落地）

## 一、背景

90 号变更记录"下一步"首选为确认单工具面收敛。**勘测结论：无实际缺口**——TS agent-session confirmed 分支（short_run/script_create/storyboard_create/interactive_film_create/translation_create 单工具 + architectCreateOnly）在 Rust 侧由 67-78 号确认面以"确定性直跑"架构等效覆盖：`resolve_confirmed_intent`（button/slash + requestedIntent 判定）→ `run_confirmed_production` → 意图→执行器/工具名映射齐全（play_start/connect_choice/remove_node/draft_structure/translation_create/script_create/storyboard_create/interactive_film_create/generate_cover/short_fiction_run/sub_agent，共 11 件）+ stages/进度持久化/广播，且有 short78 等确认流 E2E。LLM 单工具回环 vs 执行器直跑是 67 号已备案的架构决策，不再重复建设。

据此转入其次候选的**两项既有备案落地**（均为契约级对齐、零新依赖）：
1. play zh 提示词逐字对齐 TS zh（82 号备案遗留）；
2. import_chapters resumeFrom 增量续放（85 号备案遗留）。

PDF 文本抽取（83 号备案）继续搁置：TS 用 unpdf（pdf.js 服务端封装），Rust 需引入重量级 PDF 依赖且抽取质量与 pdf.js 难做差分对齐，值得独立轮次选型。

## 二、交付

### 1. play zh 提示词逐字（`play_runner.rs` 三处）

- **mutator system zh 补两行**（82 号备案的"范例 JSON 行"）：`下面的范例只示结构，不得复用范例里的名称、人名或剧情事实；唯一必须保留的示例 id 是玩家本人 actor_player：` + zh JSON 范例行（玩家角色/相关人物/示例线索/示例钥匙四实体 + 怀疑/持有/持有(physical) 三边 + 示例倒计时——与 TS zh 模板字面量逐字，含 `\\n` 字面转义语义）。
- **renderer system zh 整体替换**为 TS zh 逐字（15 行 base）：73 号自由组织形态 → TS 原文。行为级修正点：
  - guided 分支 suggestedActions 数量语义 **2-4 个 → 0-3 个**（TS 原文，修复行为偏差）；
  - 补齐 TS 的"否定动作即事实"规则、"不催不逼也不借同伴之口逼选"（含 `**多数 beat 根本不该以一个待决问题结束**`）、"正文绝不允许选项清单"、生死关头【错】/【对】对比示例（字面 `\n-` 转义）、Time 权威规则；
  - 输出行 `输出严格 JSON：sceneText, suggestedActions。`（替换 73 号的花括号形态）。
- **renderer user zh 替换**为 TS zh 分块标签形态：`世界设定（始终遵守）：`（premise trim）/`玩家原话：`/`动作：`（**pretty JSON**，替换 73 号单行紧凑 + 非 pretty 形态）/`已应用的本回合变化：`/`当前状态摘要：`；重写约束块保留（双语标签 79 号已逐字）。
- 单测 +1：`zh_agent_prompts_verbatim`（mutator 范例引导行与 JSON 关键片段、renderer 全部关键行 + guided/open 0-3 语义 + 末行、renderer user 分块标签 + pretty JSON + 无 premise 省略块）。
- 既有 renderer user 的 replay 注入测试与 FakeAgents 全链测试零改动通过。

### 2. import_chapters resumeFrom 续放 + importMode 直通

- **`book_create_routes.rs`**：`import_chapters_chain` 拆为薄壳（start_from=1 + Continuation，REST 端点行为不变）+ `import_chapters_chain_with_resume(runtime, book_id, chapters, start_from, import_mode)`：
  - start_from == 1 → Step 1 全量重建（地基生成 + 真相重置 + 空索引 + 快照 0 + 风格指纹）——现状路径；
  - start_from > 1 → **跳过 Step 1**（保留既有地基与早前章节）；
  - 回放循环 `.skip(start_from - 1)`；索引同号替换（resume 语义，既有逻辑）；
  - import_mode 直通 `generate_foundation_from_import`（`ImportMode::Series` 枚举 62 号已有）。
- **`import_chapters_tool.rs`**：移除 resumeFrom>1 与 importMode=series 两处暂缓拦截；importMode 解析（series/缺省 continuation）；文本第三行按 `(resumeFrom ?? 1) === 1` 分支（`Resumed replay from chapter {n}; earlier chapters and the existing foundation were kept.` 逐字）；details.importMode 反映实际模式。
- E2E +1：`sub91_e2e::chat_resume_import_appends_chapter_two_keeps_foundation`——既有书（第 1 章 + 带标记的既有地基）经聊天面 resumeFrom=2 续放：只回放第 2 章（importedCount=1）、**既有地基逐字节保留**（Step 1 跳过的行为证据）、第 1 章保留 + 第 2 章落盘、索引两章、Resumed 文本分支 + details.importMode。
- 兼容修正：三处 play E2E mock 的 renderer 分流关键词从旧首行"场景应答作者"改为稳定前缀"互动小说场景"（zh 首行逐字化为"场景回**应**作者"导致失配）。

## 三、parity 要点

- play 三代理 zh 提示词至此**与 TS zh 双向逐字**（en 82 号、zh 本轮）——中英世界的行为一致性由同一份提示词源保证。
- resumeFrom 语义对齐 TS `importChapters`：startFrom 门控 Step 1、循环跳前、索引同号替换、文本 Resumed 分支、details.importMode（`params.importMode ?? "continuation"`）。
- 既有章节守卫（有章且无 resumeFrom 的逐字错误）不变。

## 四、偏差备案

1. **series 评审环**：TS `importMode === "series"` 走 `generateAndReviewFoundation`（架构师生成 → foundation-reviewer 评审 → 不过则带反馈重生成的循环）；Rust 直通 `generate_foundation_from_import(Series)`（series 提示词分支），评审环未接（与 72 号 reviseFoundation 的容错审核同构，后续可复用其 reviewer 装配）。
2. **回放 prepareWriteInput**：TS 回放前构造 governed input（chapterIntent/contextPackage/ruleStack）；Rust 分析器入参传 None（59 号移植时的既定收窄，非本轮引入）。
3. **render_entity_roster 的 120 码元截断**以 chars 计数近似 UTF-16 码元（BMP 内等价；73 号起既有形态）。

## 五、暂缓件（滚动）

- PDF 文本抽取（83 号）——需独立依赖选型轮。
- series 导入评审环（见偏差备案 1）。
- 散件收尾：单章写作中途截断、/agent model 校验、resumeFrom REST 面、fetchWithProxy、attachments 归一化、模型四层解析。
- sidecar 契约差分扫描（strangler 切换前对齐）。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1126** 过（+1：zh_agent_prompts_verbatim） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **155** 过（+1：sub91 续放全链；三处 mock 关键词修正后 play 系全绿） |
| `cargo test --features export-bindings --lib` | **1285** 过（+1） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（92 号候选）

1. **首选：series 导入评审环**（generateAndReviewFoundation——复用 72 号 foundation-reviewer 装配，补齐 importMode=series 的完整语义；或与 PDF 抽取选型合并为"导入域收尾"轮）。
2. 其次：散件收尾批（resumeFrom REST 面 / /agent model 校验 / attachments 归一化 / 模型四层解析 / fetchWithProxy / 单章截断——多为小参数面）。
3. sidecar 契约差分扫描（迁移收尾的系统对齐基线）。
