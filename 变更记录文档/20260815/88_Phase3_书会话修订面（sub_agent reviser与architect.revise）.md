# 88 号变更记录：书会话修订面（sub_agent reviser + architect.revise）

## 一、背景

87 号交付 sub_agent 聊天工具四代理（architect 建书 / writer / auditor / exporter），reviser 与 architect.revise 以拦截文案暂缓（87 号偏差备案），留待独立轮次。本轮（88 号）按 87 号"下一步"首选接入这两个分支，闭合 agent-session 聊天工具注册矩阵的最后缺口。

关键勘测结论：两条域链 Rust 已全有——

- **reviser** → 47 号 `/revise` 审核环（`books_routes.rs::run_revise_chain`：pre merged-audit → 修稿 → post merged-audit（temp 0 + 修稿真相覆盖）→ restore → revisionGate 三档门控 → 落盘 + 索引回写），无需新写链；
- **architect.revise** → 72 号 `reviseFoundation` REST 链（`book_create_routes.rs`：phase 探测 → 备份 → architect 修订 → 容错审核 → Revise 模式落盘）。

因此本轮本质是**接线轮**：域链接权复用 + TS 聊天层文案/details 逐字移植。

## 二、交付

### 1. 域链接权（零逻辑变更）

- `books_routes.rs`：`run_revise_chain`、`ReviseChainError`（含 `is_not_found`/`message`）提 `pub(crate)`——sub_agent reviser 聊天面复用链与错误文本。
- `book_create_routes.rs`：从 REST 处理器中提取 `revise_foundation_chain(runtime, book_id, feedback) -> Result<(), String>`（`pub(crate)`，phase 探测 + 备份 + 内链），REST 端点改为调用它（广播/状态码留在 HTTP 层），聊天面直接复用（不广播，对齐 TS pipeline 方法不发 hub 事件）。

### 2. `interaction/sub_agent_tool.rs` 分支接入

- **守卫修正**：有活动书时 architect 拒绝建书的条件从 `agent == "architect"` 收紧为 `agent == "architect" && !revise`（TS `activeBookId && agent === "architect" && !revise`）——revise=true 放行进入架构稿重写。
- **reviser 分支**（`reviser` 函数）：
  - `resolve_tool_book_id("reviser", bookId, activeBookId)`；
  - 章号：显式值须为 ≥1 整数（0/负数 → 链内错误文案 `No chapters to revise for "{bookId}"`，对齐 TS `targetChapter < 1` 前置守卫）；缺省取 `get_next_chapter_number - 1`（最新章），<1 同报；
  - 模式：`mode ?? "spot-fix"`（五模式 spot-fix/polish/rewrite/rework/anti-detect，instruction 作为修稿 brief）；
  - gate 用 `runtime.revision_gate`（对齐 TS `config.revisionGate ?? "strict"`；rewrite REST 端点的强制 Always 不适用聊天面）；
  - 结果组装（`revision_details_json`）：details 键序 `kind/bookId/chapterNumber/mode/applied/status/wordCount/fixedIssues` + 可选 `skippedReason/revisionDiagnostics`（无值不出现，对齐 JSON.stringify 省略 undefined；revisedContent 不透出——TS ReviseResult 无此键）；
  - 文案逐字：applied → `Revision ({mode}) complete for "{book}" chapter {n}.`；not-applied → `Revision not applied for "{book}" chapter {n}: {reason}.` + 诊断文本块（`revision_diagnostics_text`：空行 + `Revision gate:` + `- Standard/Before/After` 计数行 + `- Remaining issues:` 缩进列表 `  - [severity] category: description (suggestion)`）。
- **architect.revise 分支**（`architect_revise_foundation` 函数）：
  - 无活动书 → `Open the book first before revising its foundation.`（TS 逐字；当前 Rust 注册面下 sub_agent 仅书会话注册，此守卫为防御性 parity）；
  - `feedback ?? instruction`（feedback 空串回退 instruction，见偏差备案）；
  - 复用 `revise_foundation_chain`；
  - 文案逐字双语：zh `Book "{id}" 架构稿已按要求重写。原书的条目式架构稿已备份到 story/.backup-phase4-<时间戳>/。` / en 对应（TS 硬编码 phase4 字样，即便 phase5 书亦然，逐字保留）；无 details（TS textResult 单参）。
- 87 号两处暂缓拦截文案删除。

### 3. 测试

- 单测（`sub_agent_tool.rs`，3 → 4 个用例内含新断言）：reviser 无章可修（书不存在/显式 0 章，错误文案逐字）、architect.revise 无书守卫、拒绝分支 details/诊断文本逐字（fabricated `ReviseChainResult`：skippedReason/前后计数/剩余问题 suggestion 括号格式、applied 分支无可选键）。
- E2E（`tests/e2e_write_next_contract.rs` 新增 `mod sub88_e2e`，2 测试）：
  - `book_session_sub_agent_reviser_applied_and_persists`：书会话经 `/api/v1/agent` 发"修订一章" → mock 分流（修稿编辑 TAG 输出 + 审计温控：pre 默认温 1 warning、post temp 0 通过）→ 卡片 `Revision (polish) complete for "b1" chapter 1.` + details `chapter_revision`（applied/status ready-for-review/无可选键）+ 章节文件改写（标题保留 + 修订正文落盘）；
  - `book_session_sub_agent_reviser_gate_refusal_keeps_original`：post temp 0 返回 2 warning（变差）→ strict 门控拒绝 → result 含完整诊断文本（`Manual revision kept original chapter: before blocking=1,...` + `Revision gate:` + Standard/Before/After + Remaining issues 缩进行）+ details `revisionDiagnostics`（before/after 计数、remainingIssues）+ **原章文件逐字节保留**。

## 三、parity 要点

- TS `createSubAgentTool` reviser/architect.revise 两分支的守卫序、默认值（spot-fix、最新章）、文本、details 键集逐字对齐。
- 复用即 parity：47 号链已含 "No warning, critical, or AI-tell issues to fix." unchanged 路径（显式 brief 非空时绕过）与三档门控拒绝路径，聊天面直接继承。
- 链错误（如缺章 404 文案 `Chapter not found`）经 `ReviseChainError::message` 透传为聊天错误结果，对齐 TS throw → 工具层 console.error + rethrow 的可见失败语义（Rust 聊天面无 console，error_result 即终态）。

## 四、偏差备案

1. **`feedback ?? instruction` 的空串语义**：TS `??` 仅回退 nullish，`feedback=""` 会把空串传给 reviseFoundation；Rust `field_str` 把空串同样回退 instruction（REST 面本就拒绝空 feedback）。聊天面模型极少传空 feedback，取更安全侧。
2. **章节号非整数**：TS 会把 1.5 透传进链（索引 find 不中 → `Chapter 1.5 not found in index`）；Rust 过滤非整数为缺省（走最新章）。极端输入的防御性收窄。
3. **mode 非法值**：TS 把任意字符串透传（reviser agent 内部处理）；Rust 链内未知模式回退 SpotFix。行为面等价收窄。
4. **TS `result.chapterNumber ?? chapterNumber` 双层回退**：Rust 链恒返回目标章号，无需回退层。
5. **architectCreateOnly 注册面**（短篇建书确认会话的 architect-only schema 变体）不在本轮范围——Rust 确认面走 propose_action actionPayload 路径（84 号），该变体注册随确认面收敛轮次再评估。

## 五、暂缓件（滚动）

- architect.revise 全链 E2E（architect 修订 + 容错审核 + Revise 落盘经聊天面）——72 号 REST E2E 已覆盖链本体，聊天面为薄守卫 + 文案，单测覆盖守卫即够；如需补可仿 sub88 模式加 architect mock。
- TS reviseDraft 链内的 governed artifacts（chapter intent/memo/contextPackage 注入修稿）与 lengthTelemetry/lengthWarnings/normalizeDraftLengthIfNeeded——47 号移植时的既定偏差（链面等价收窄），非本轮引入。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1113** 过（+1：sub_agent_tool 拒绝分支单测） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **150** 过（+2：sub88 applied/gate-refusal） |
| `cargo test --features export-bindings --lib` | **1272** 过（+1） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（89 号候选）

agent-session 聊天工具注册矩阵至此**全数落地**（11 件：文件三件 + material 双件 + propose_action + research_web + import_chapters + sub_agent + play 三件）。下一轮候选：

1. **首选：书会话余下编辑工具族**（agent-session book/edit 分支的 write_truth_file / rename_entity / patch_chapter_text / replace_chapter_text / delete_latest_chapter + generate_cover 壳——契约源 `agent-tools.ts` 对应 createXxxTool，多数为确定性文件操作，域链复用度高）。
2. 其次：既有备案落地——PDF 文本抽取（83 号）、play zh 提示词逐字对齐 TS zh（82 号）、resumeFrom 增量续放与 importMode=series（85 号）。
3. 散件收尾：单章写作中途截断、/agent model 校验、resumeFrom REST、fetchWithProxy、attachments 归一化、模型四层解析。
