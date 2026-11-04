# 887 号 · 工程轮：M3a——短篇纯函数层 + serde 契约 + fail-closed 短篇分支兑换

> 886 设计 §三 M3a 落地（LLM-free 面，全部金标准可锁）。新增 ≈530 行 Rust 纯函数层
> + 110 行 Observation 契约 + TS/Rust 双端金标准测试；**884 号 fail-closed 短篇两
> 分支正式兑换**（translation-*/story-graph 保留并收窄声明）。

## 一、交付面

### production/short_fiction.rs（新域，2.0 形态纯函数层）

- **serde 契约**（zod/TypeBox strict ↔ deny_unknown_fields 互逆）：ShortFictionChapter/
  BatchDraft/SalesPackage/Measurements/AuthorScope/RevisionPlan/Stage/ProductionState/
  Delivery/Target 全族 + Observation（models/observation.rs 新建，zod strict 契约
  + validate）。
- **纯函数 2.0 形态**（886 处置表：与旧形态有实质差者新写，旧文件不动）：
  measure/render_sales_package/render_draft_markdown（**跳过未完成章**——旧形态渲染
  标题行）/format_chapter_heading（**空标题抛错**——旧形态回退+前缀检测）/
  validate_short_fiction_draft_for_final（**U+FFFD 腐化检测**+title/hook 契约+min/max
  options——cjk 纪律生产同款）/find_incomplete/opening_hook_length_contract
  (ceil=mul.div_ceil)/validate_opening_hook/estimate_max_tokens/build_chapter_batches/
  safe_output_budget/all_chapter_numbers/merge_short_fiction_batch（**current 存在即
  不回退 batch 的 opening_hook 语义**）/short_review_sources/assert_short_author_scope
  （**title 保护在 wholeManuscript 早退之前**——整稿授权不含改名）/check_short_package_tool
  （TypeBox 无 additionalProperties → 额外键放行）/render_sales_package_from_value。
- **状态机**（short-production-state.ts）：shortInputHash（canonical 键递归排序→
  sha256，serde Value BTreeMap 天然键序）+ read/write_short_production_state
  （ENOENT→None/parse 校验/stringify+"\n" 原子提交）+ version literal(2) 校验。
- 计数底座复用 utils::length_metrics（strip_markdown_metadata 逐条对齐，零重复）。

### artifact_validation 兑换（884 → 887）

- **顶部 schema 快验**：short-manuscript=ShortFictionBatchDraft strict parse（失败
  ARTIFACT_INVALID）；short-package=check_short_package_tool（失败 ARTIFACT_INVALID）。
- **sales-package.json 分支**：校验 → rawContent=render → **三件写**（sales-package.json
  保留 document 额外键 + sales-package.md + cover-prompt.md）。
- **authority 分支**：parse → charCount 重算 → validateForFinal(expected=len) →
  连续性检查 → markdown 再渲染 → drafts/v001/{draft.json,full.md} + final/full.md +
  已注册 final .md 投影族（排除 chapters/三豁免/full.md）+ chapters/{final,drafts}
  双目录四位编号多写；错误统一 catch→ARTIFACT_INVALID(authority)（TS 同款）。
- **adopt title_override 接通**：short-story.json 采纳时 storyTitle→work 标题覆写
  （884 占位注释兑现；parse 必成——validatedArtifactWrites 已 strict 过）。
- translation-*/story-graph fail-closed 保留（范围收窄声明）。

### 范围外修正

- core index 补导出四纯函数（ShortFictionBatchDraftSchema/measure/buildBatches/
  formatHeading——TS 测试消费面）。

## 二、测试（金标准双端逐字共用）

- **TS 腿** `packages/core/src/__tests__/short-fiction-cross-engine.test.ts` 7 用例：
  strict 双拒（未知键/缺 charCount）/渲染金标准串（GOLDEN_RENDERED_MARKDOWN 逐字）/
  度量 openingHook=7/章标题格式/批次同参数同结果/变异两类（TEXT_CORRUPTED 码面+
  chapters=[1]；INCOMPLETE=[2]）/未完成章不渲染（2.0 形态差锚定）。
- **Rust 腿** production/short_fiction.rs `mod tests` 11 用例（同 fixture 同串）+
  端点级兑换测试（work_domain_m1b.rs `adopt_regenerates_short_manuscript_projections`
  改写自 884 号 fail_closed 测试）：adopt authority → 200 + **storyTitle 覆写**
  （work title=镜中雨）+ 七投影落盘 + full.md 金标准串逐字 + 清单 checksum=
  **重算后 draft JSON 字节**（非候选原文——TS write(authority, stringify(draft))
  同款语义，charCount/rawContent 已变，首版断言误用候选 hash 已修正）+ rawContent
  注册内容断言。

## 三、验证清单

testgate 2023+584 / clippy 双 0 / gate:ts 八步全绿 / 交替互证 27/27 回归 /
TS 金标准 7/7 / Rust 模块 11/11 / 端点 11 用例 / U+FFFD 干净。
备案：测试自坑×3（批次输入缺 6、scope 断言漏 title-independence 语义、
json! 紧凑形态 replace 教训复用 885）；**下一轮 = M3b submit_structured 基座**
（base.ts 224 行精读为前置）。**下一号自 888 起；出口不变；用户侧待办=Fork 推送
21 个提交。**
