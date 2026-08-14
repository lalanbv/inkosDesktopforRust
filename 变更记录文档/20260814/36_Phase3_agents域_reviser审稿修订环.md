# 36 号｜Phase 3 agents 域：reviser 审稿修订环（write-next 内环合龙）

> 里程碑：writeNextChapter 的 audit→revise 内环核心移植。reviser.ts（714 行）
> 全量：auto 模式输出分流（patch-only / rewrite-only / allow-full 三态路由）+
> legacy 五档修稿模式 + governed 治理装配（工作集裁剪 + 记忆证据块 + 缩减控制块 +
> 表格合并回写）。至此 planner → composer → writer → reviser 数据面全通。

## 背景

35 号合龙 plan→compose 链后，writeNextChapter（auto 模式默认走 audit→revise 环）
的最后 agents 域缺口是 reviser。其全部依赖（governed-working-set /
spot-fix-patches / narrative-control / governed-context / context-filter）在 32 号
前已就绪，本号零新增依赖。

## 改动

### 新建 `engine-rs/src/agents/reviser.rs`（~1100 行，含测试）

**类型与端口**
- `ReviseMode`（Auto 默认 / Polish / Rewrite / Rework / AntiDetect / SpotFix）
- `ReviseOutput`（revisedContent / wordCount / fixedIssues / updatedState /
  updatedLedger / updatedHooks / tokenUsage）
- `ReviserChat` trait + `ReviserCtx { project_root, builtin_genres_dir, prompt_store }`
- `ReviseOptions`（chapterIntent / chapterMemo / chapterIntentData /
  contextPackage / ruleStack / lengthSpec 可选治理入参）

**编排入口 `revise_chapter`**
- 十路并行真相文件读（current_state 回退链 / ledger / hooks / style_guide /
  volume_map / story_frame / 角色矩阵 / 摘要 / 双正典）
- genre/language/rules 加载 + style_guide 缺失回退 legacy book_rules 正文
  （Phase 5 hotfix 2：story_frame frontmatter 的空 body 不是可用指南）
- **governed 三全判定**（intent+package+ruleStack）→ hook/摘要/矩阵工作集裁剪
- 系统提示词（auto/legacy 双路）+ prompt pack（longform.reviser）追加
- user prompt 装配：分层问题清单 + 状态卡 + 资源账本 + 叙事净化块链
  （hookDebt/hooks/volumeSummaries 经 sanitizeNarrativeEvidenceBlock）+
  缩减控制块或卷纲 + 世界观（仅 legacy）+ 矩阵 + 摘要 + 双正典 + 文风指南 +
  字数护栏（仅 legacy）+ 待修正章节
- chat(0.3) → `parse_reviser_output` → governed 表格合并回写 → wordCount
  （lengthSpec 计量模式优先，否则 JS `length` 语义 = UTF-16 码元数）

**auto 输出分流 `resolve_auto_output_mode`**
- repairScope 优先：structural → rewrite-only；scoped 占满 blocking 且全 local →
  patch-only
- 否则分类正则计数（21 条：9 局部 + 12 结构）：任一结构 → rewrite-only；全局部 →
  patch-only；混合/未知 → allow-full；仅 info → patch-only（提示不驱动路由）
- patch-only：补丁应用率 ≥50% 才接受；rewrite-only：拒绝补丁不回退（结构问题
  无法安全补丁）

**纯函数（golden 守门）**：`build_tiered_issue_list`、`parse_reviser_output`
（tag 提取 + 三态路由）、`build_auto_system_prompt`（zh/en 全文 + 路由指令 +
长度硬约束）、`build_legacy_system_prompt`（模式描述 + 输出格式双形态）、
`build_reduced_control_block`、`mode_description`（五档幅度逐字）

## parity 要点（移植难点）

1. **tag 提取正则的 lookahead**：TS `=== TAG ===\s*([\s\S]*?)(?==== [A-Z_]+ ===|$)`
   → Rust `(?:=== [A-Z_]+ ===|\z)` 消费式终止符等价还原（捕获组语义一致）
2. **模板换行微观结构**：legacy 系统提示词的 `${lengthGuardrail}\n${spotFixExtra}`
   两占位符间有一个换行——首版合并成 `{guardrail}{extra}` 丢一个 `\n`，golden
   `legacy-system-prompt-spot-fix` 差分抓出后修正
3. **golden 输入的完整反序列化**：Rust `LengthSpec` 必填 `normalizeMode`，TS 侧
   构造的对象无此字段 → 差分侧静默 None → 硬约束行缺失；TS dump 输入补字段
   （教训：golden 输入必须满足消费方完整 schema，`.ok()` 吞错是差分盲区）
4. **`??` 是 null/undefined 检查非空串检查**：governed 证据块的
   `hooksBlock ?? fallback` 在块为空串时保留空串（不回退），Rust 用
   `and_then + unwrap_or_else` 而非 `filter(!is_empty)` 对齐
5. **wordCount 双语义**：无 lengthSpec 时 = `revisedContent.length`（UTF-16 码元）；
   有 lengthSpec 时 = `countChapterLength(countingMode)`
6. **`Math.max` 除法下溢**：补丁接受率 `appliedPatchCount / patches.length >= 0.5`
   → 整数域 `count * 2 >= len` 等价（避免浮点）
7. **auto 模式字数约束嵌在 REVISED_CONTENT 描述里**（routing 指令 + 硬约束行），
   legacy 才用第 8 条护栏 + 字数护栏块——两路互斥，逐字移植
8. **`governedMemoryBlocks?.hookDebtBlock ?? ""`**：无 contextPackage 时块为空串
   参与 user prompt 拼接（非省略）

## 验证

- **lib 单测**：793 passed / 0 failed（+9 新单测：分层清单、分流矩阵七态、tag
  提取、legacy 回退、rewrite-only 拒补丁、patch-only 应用、auto/legacy 提示词
  骨架、缩减控制块形状）
- **golden 差分**：72 域全绿（+1 域 9 例：parse 四态 + auto/legacy 提示词四态 +
  缩减控制块）。差分实战抓出 2 个真实 bug（模板换行丢失、LengthSpec 静默反序列化
  失败）——机制第五次证明有效性
- **export-bindings**：952 passed
- **TS 全量**：185 文件 / 1798 测试全绿（leaf dump 重生成）
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **37 号**：agents 域数据面全通——开始 runner 编排移植（`_writeNextChapterLocked`：
  plan[持久化复用] → compose → write → settle → audit→revise 环 → 落盘）+ task-store
  异步模型 + `/api/v1/books/:id/write-next` 路由挂载（strangler 切换首个核心端点）。
  备选：polisher.ts / length-normalizer.ts（normalize 链，auto 模式的字数收尾）。
