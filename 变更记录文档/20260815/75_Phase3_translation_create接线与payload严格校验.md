# 75 号：translation_create 接线与 actionPayload 严格校验

**日期**：2026-08-15
**阶段**：Phase3 深度收尾（74 号"下一步"首选：translation_create 纯接线 + actionPayload strict 散件）
**契约源**：`packages/core/src/agent/agent-tools.ts` L1495-L1527（createTranslationCreateTool）、`interaction/action-envelope.ts` L41-L194（ActionPayloadSchema 全 12 子域）、`agents/short-fiction.ts` L24-L33（短篇字数常量）

---

## 一、背景

两个收尾件一次交付：其一，translation_create 确认意图——70 号 translation 域本体（createTranslationProjectFromFile）全齐，纯接线；其二，67 号偏差备案的 actionPayload strict 校验——TS 是 zod strict（顶层与 9 个子域拒绝 unknown 键 + 字段类型/枚举/区间），Rust 此前只有"必须为 object"结构守卫。

## 二、交付

### 1. actionPayload 严格校验（agent_route.rs，67 号偏差备案清账）

- **表驱动 12 子域**（ActionPayloadSchema 逐字）：createBook/writeNext/shortRun/playStart/generateCover/scriptCreate/storyboardCreate/interactiveFilmCreate/translationCreate 九个 **strict** 子域（unknown 键拒绝）+ draftStructure/connectChoice/removeNode 三个**非 strict**子域（TS 无 `.strict()`——unknown 键放行）。
- **字段形态六类**：`StrNonEmpty`（z.string().min(1)——空串非法、空白串合法，zod 不 trim）/ `IntRange`（int + min/max）/ `Enum`（platform 四值、language、mode、targetFormat 五值）/ `Bool` / `StrArray`（playStart.suggestedActions 1-4 非空串）/ `Object`（connectChoice.node 是 StoryNodeSchema 结构——形态校验由执行器承担）。
- **shortRun superRefine 联动**：language+charsPerChapter 同时存在时分段校验（zh 900-1200 汉字 / en 600-800 英文词）；无 language 时维持 600-1200 并集（基础 IntRange）——确认卡阶段拒绝非法组合而非任务开跑后抛错。
- 顶层 object + 12 键白名单；非法 → 400 `INVALID_ACTION_PAYLOAD`（含具体 Unrecognized key / invalid value 消息）。

### 2. translation_create 确认意图执行器（agent_production.rs）

- **装配分支**：payload.translationCreate 三必填（filePath/sourceLanguage/targetLanguage——缺 → 502 中文"确认创建翻译项目缺少…"逐字）+ title/segmentMaxChars 可选透传；tool 名 `translation_create`。
- **`execute_translation_create`**：`create_translation_project_from_file`（70 号域本体，与 POST /translations/create 同链——摄取分段不翻译）→ 结果文本**五段逐字**（Translation project "X" created. / ID / Source: kind src->dst / Chapters / Manifest 路径）+ details `{kind:"translation_project_created", projectDir, manifestPath, manifest}`。
- translation_create **不在** suppressManualTextForTool 表 → response 为完整结果文本（E2E 固化）。
- 确认意图累计 **7/11**（write_next/create_book/play_start/draft_structure/connect_choice/remove_node/translation_create）。

### 测试

- **lib 单测（3 个）**：顶层/子域 strict 与非 strict 三态（unknown 键正反 + draftStructure 放行）、字段类型与区间（platform 枚举/chapterCount 1-20/min(1) 空串 vs 空白串语义/suggestedActions 数组边界）、shortRun 联动（zh 700 拒/en 700 过/无 language 并集过）。
- **E2E `translation75_e2e`（2 个）**：**确认全链**（fixture txt → button 意图 → 项目落盘（manifest/chapters/分段/manifestPath）→ 结果文本五段 → response 非空（非 suppress 工具）→ 缺 filePath 502 中文 → 幽灵文件 502）；**strict 端点面**（顶层 bogus 键 400/connectChoice 子域 extra 400/platform 枚举 400/shortRun zh+700 联动 400/draftStructure 非 strict 放行走意图分支）。
- 开发中修正一处 schema 误读：connectChoice.node 误标 StrNonEmpty → Object 形态（74 号 E2E 回归捕获）。

## 三、parity 要点

1. **min(1) 不 trim**：`z.string().min(1)` 允许空白串（长度 ≥1）；真正的 trim 校验在执行器（requirePayloadText 语义）——两层分离逐字。
2. **strict 三态**：顶层 strict + 9 子域 strict + 3 子域非 strict——与 TS `.strict()` 标注逐域一致。
3. **联动分段**：确认卡阶段拒绝 en+1100 类非法组合（TS superRefine 注释语义——"不是任务开跑后才在 runner 里抛错"）。
4. **suppress 名单**：translation_create 不在 suppressManualTextForTool 六工具表 → response 保留结果文本（与 play_start 的空 response 对照）。

## 四、偏差备案

1. **strict 错误消息形态**：TS zod 消息（`Unrecognized key: "x"` 等）与 Rust 消息（`Unrecognized key: x`）引号差异——code 同为 INVALID_ACTION_PAYLOAD，前端按 code 分支。
2. **connectChoice.node 深度校验**：TS 在 zod 层做 StoryNodeSchema.parse（400）；Rust 端点层只查 object 形态，完整结构校验在执行器（502 面）——错误状态码差异（400 vs 502），消息语义一致。

## 五、暂缓件（沿 74 号清单滚动）

- 剩余 4 个确认意图执行器（short_run/generate_cover/script_create+storyboard_create/interactive_film_create——各自依赖 short-fiction runner 域本体与 script-storyboard-runner 588 行）
- sceneReconciler + regenerate 变体/checkpoint 面、play en 提示词、play_step 聊天工具
- 单章写作中途截断、/agent model 校验、resumeFrom、fetchWithProxy、attachments、模型四层解析

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **1039** 通过（+3） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **124** 通过（+2） |
| `cargo test --features export-bindings --lib` | 1198 通过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：POST /agent 的翻译确认卡（前端确认按钮）Rust 端全通；actionPayload 从结构守卫升级为 zod 逐字校验（67 号备案清账）——非法 payload 在 400 阶段拦截。
- **下一步（76 号候选）**：script_create/storyboard_create（script-storyboard-runner 588 行域本体移植 + 双执行器接线——完成后 script/storyboard/interactive-film 三个衍生创作域只剩 interactive_film_create）；或 short_run + generate_cover（short-fiction runner 主体移植 + 74 号 cover 链复用——generate_cover 纯接线级）；或散件（单章截断/model 校验/resumeFrom）。
