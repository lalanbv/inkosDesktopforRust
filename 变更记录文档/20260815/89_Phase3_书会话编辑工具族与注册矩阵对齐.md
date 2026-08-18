# 89 号变更记录：书会话编辑工具族 + 注册矩阵对齐

## 一、背景

87 号交付 sub_agent 四代理、88 号补 reviser/architect.revise 后，agent-session 聊天工具面仅剩 book/edit 分支的确定性编辑工具族（87 号变更记录"下一步"次选）未接入。本轮（89 号）移植六件工具——write_truth_file / rename_entity / patch_chapter_text / replace_chapter_text / delete_latest_chapter / generate_cover——并把注册矩阵对齐 TS agent-session 真值表（含 84/85 号遗留的三处偏差修正）。

## 二、交付

### 1. `interaction/edit_controller.rs`：补两个事务（89 号核心移植）

- **`execute_entity_rename`**（TS `executeEntityRename` 逐字语义）：
  - 新值含 `/` 或 `\` 即拒（`Invalid rename target "..." : entity names cannot contain path separators.`）；
  - `collect_editable_files`：递归遍历书目录；snapshots 与点目录（如 chapters/.trash）跳过——冻结历史与废弃稿不可改写；仅 .md/.json/.ya?ml/.txt；
  - `plan_entity_file_renames`：文件名含旧值 → 全替换；目标已存在即整体中止（避免内容半重写）；`base.split(old).join(new)` = `str::replace`；
  - 内容替换：regex 转义旧值全局替换（`NoExpand` 字面量替换语义）；touched 集合插入序去重（对齐 JS Set）；重命名后 from 键换 to 键；
  - 零命中 → `No occurrences of "{old}" were found in "{book}".`；
  - summary：`Renamed {old} to {new} across {n} files (k file(s) renamed on disk).`。
- **`execute_chapter_local_edit`**（TS `executeChapterLocalEdit`）：
  - 前置守卫 `Chapter-local edits require targetText and replacementText.`；
  - **三级目标替换**（`replace_chapter_target_text`）：① 精确子串全替换 → ② 弹性空白（目标按空白切 ≥2 段以 `\s+` 连接的全局正则）→ ③ 近似段落定位（`replace_approximate_paragraph`：NFKC + lower + 仅留字母数字归一；段长 ≥24 且在目标 0.35x–3x；dice 二元组相似度——包含关系取长度比；阈值 0.72，低于 0.86 须领先次优 0.06；JS `\S[\s\S]*?(?=\n\s*\n|$)` 惰性前瞻正则复刻为 `paragraph_spans` 手写扫描 + `\n\s*\n` find_at）；
  - 未命中 → `Target text was not found in chapter {n}.`；
  - 归档（Agent 版本源）→ 覆写 → 清 runtime 工件（保留 user-brief）→ 索引 audit-failed + `[warning] Manual text edit requires review before continuation.`；
  - summary：`Patched chapter {n} and marked it for review.`。
- `ExecutedEditTransaction.chapter_number` 改 `Option<u32>` + `skip_serializing_if`——entity-rename 无 chapterNumber 键（TS 序列化面逐字），chapter-replace REST 形态不变（Some）。
- 单测 +2：entity-rename（内容替换 + 角色卡文件重命名 + snapshots 不可改写 + 无命中/分隔符错误文案）与 chapter-local-edit 三级替换全路径（精确/弹性空白/近似段落/未命中/前置守卫）。

### 2. `interaction/book_edit_tools.rs`（新建，六件聊天壳）

- `assert_safe_truth_file_name`：TS `assertSafeTruthFileName` 逐字——trim → 补 `.md`（大小写敏感 endsWith）→ 扁平白名单 15 项 + outline 白名单 4 项 + `roles/(主要角色|次要角色|major|minor)/[^/\]+\.md` 正则；非法名 `Invalid truth file name: "..."`。
- `tool_write_truth_file`：resolve → 白名单 → `ensure_control_documents` → `safe_child_path` → mkdir 父目录 → 落盘；成功文本 `Updated "{fileName}" for "{bookId}".`；**失败面为普通文本** `write_truth_file failed: {msg}`（TS try/catch 语义，非错误卡）。
- `tool_rename_entity` / `tool_patch_chapter_text` / `tool_replace_chapter_text`：文本 = 事务 summary；replace 用 Agent 版本源（TS 工具路径缺省）；章节号缺省/非整数 → `Chapter NaN not found.`。
- `tool_delete_latest_chapter`：复用 `chapter_delete::delete_latest_chapter`（Latest/Chapter/NaN 三态请求）；文本 `Deleted latest chapter {n} from "{book}", preserved it in trash, and rolled story state back to chapter {m}.`；details `{kind: chapter_deleted, bookId, deletedChapter, title, trashedFiles, rolledBackTo, discarded}`（TS `...result` 全键）。
- `tool_generate_cover`：复用确认面 `execute_generate_cover`（74/76 号 cover 基础设施，文本/details 逐字 `Cover generated for "{title}".` + `cover_generated`）；selling_points 经 `normalize_selling_points` 分号/分号全角/换行切分。
- `deterministic_tool_schemas()`（五件 schema 逐字）+ `generate_cover_schema()` + `execute_book_edit_tool` 分发器。
- 单测 2：白名单（补 .md 大小写语义/角色卡/非法集/错误文案逐字）与 schema 形状。

### 3. `server/agent_route.rs`：注册矩阵对齐 TS 真值表

按 TS agent-session `buildTools` 分支序推导（chat/short/script/storyboard/film/play 按 sessionKind 先返回，bookId 不影响这些分支）：

| 会话 | propose | research | import | sub_agent | 编辑五件 | generate_cover |
|---|---|---|---|---|---|---|
| chat（±book） | ✓ | ✓ | ✓ | ✗ | ✗ | ✗ |
| short/script/storyboard/film | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| play 有世界 / 无世界 | ✗ / ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| book-create 无书 | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ |
| book / book-create（有书） | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ |
| edit（有书） | ✗ | ✗ | ✗ | ✗ | ✓ | ✗ |

三处 84/85/87 号遗留偏差修正：book/edit 会话不再注册 propose_action；research 收窄至 chat/book-create/book（edit 与 short 系不再注册）；import 扩至 book 会话（TS bookTools 含 importChaptersTool）；sub_agent 收窄至 book/book-create（chat+book 不注册）。
`ChatToolRouter` 六级分发：propose → research → import → sub_agent → **书会话编辑工具族** → play → 项目文件工具。

### 4. 测试

- E2E `mod sub89_e2e`（2 测试）：book 会话 patch（精确替换 + 索引复核标记 + 文件改写）与 delete_latest（.trash 保留 + 快照回滚 + details）全链 + book 会话注册真值表（六件 + sub_agent + research + import 齐备、无 propose）；edit 会话注册确定性子集（五件齐备，sub_agent/generate_cover/research/import/propose 全不注册）。
- 全量 E2E 无回归（150 → 152 过，既有注册断言与新矩阵兼容）。

## 三、parity 要点

- 六件工具的 schema、守卫文案、成功/失败文本、details 键集逐字对齐 TS。
- edit-controller 两事务的文件遍历规则（snapshots/点目录排除）、错误文案、summary 格式（含单复数 file/files）逐字。
- 三级替换算法的归一化（NFKC/lower/仅字母数字）、长度阈值（UTF-16 码元域）、dice 二元组、0.72/0.86±0.06 阈值与段落切分语义逐字。

## 四、偏差备案

1. **`replace_all` 替换串语义**：JS `content.replace(matcher, newValue)` 的替换串 `$&` 等有展开语义；Rust 用 `NoExpand` 字面量（含 `$` 的新值不被展开）。LLM 供给边界下字面量是安全侧。
2. **generate_cover 端点覆盖参数**：TS 聊天 schema 另有 coverBaseUrl/coverEndpoint/coverModel/coverSize/coverApiKeyEnv 五个可选直通参数；Rust 封面链从 inkos.json 解析配置（与确认面同 reductions），schema 未列这五参数。需要运行时覆盖时先落项目配置。
3. **必填参数缺失的极端输入**：TS 对缺 fileName/oldValue 等产生 TypeError 面或 undefined 穿透（如 `Invalid truth file name: undefined`）；Rust 以等义错误文案拦截（`Chapter NaN not found.` / `rename_entity requires oldValue and newValue.`）。
4. **JS Set 序 vs readdir 序**：touched_files 顺序两侧都源自 readdir 顺序（平台相关），插入序去重语义一致，不做跨平台排序承诺。
5. **chapter-local-edit 版本源**：TS 传 `"agent"`；Rust `ChapterVersionSource::Agent`（`.versions/NNNN/` 归档前缀一致）。

## 五、暂缓件（滚动）

- edit-controller 余下 kind：truth-file-edit（聊天面走 write_truth_file 白名单路径已覆盖主用途）、focus-edit、chapter-rewrite、planEditTransaction 规划面。
- book 会话 forecast 三件（create/get/select narrative forecast——独立域，未移植）。
- film-authoring / translation_create / short_run / script_create / storyboard_create / interactive_film_create 确认单工具面（TS confirmed 分支整组替换工具集；Rust 确认面走 propose_action + agent_production 执行器，聊天面单工具注册随确认面收敛轮次评估）。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1117** 过（+4：edit_controller 2 + book_edit_tools 2） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **152** 过（+2：sub89 两件） |
| `cargo test --features export-bindings --lib` | **1276** 过（+4） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过（首次并行跑有 12 例资源抖动，复跑全绿） |

## 七、影响面与下一步（90 号候选）

agent-session 聊天工具面至此**按 TS 真值表全数对齐**（book 会话 13 件、chat 8 件、edit 9 件、play 5 件）。下一轮候选：

1. **首选：narrative forecast 三件套**（create/get/select——book 会话注册矩阵最后缺口；契约源 agent-tools.ts 的 createNarrativeForecastCreateTool/GetTool/SelectTool，含 LLM 投影链与分支选择落盘）。
2. 其次：确认单工具面收敛（short_run/script_create 等聊天注册 + architectCreateOnly 变体）。
3. 既有备案落地：PDF 文本抽取（83 号）、play zh 提示词逐字（82 号）、resumeFrom 增量续放与 importMode=series（85 号）、generate_cover 端点覆盖参数。
4. 散件收尾：单章写作中途截断、/agent model 校验、resumeFrom REST、fetchWithProxy、attachments 归一化、模型四层解析。
