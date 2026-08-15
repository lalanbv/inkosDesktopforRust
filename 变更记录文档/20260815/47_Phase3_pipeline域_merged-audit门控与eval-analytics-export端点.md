# 47 号变更记录：merged audit 门控 + /eval /analytics /export 端点

- 日期：2026-08-14
- 范围：engine-rs（pipeline/utils/interaction/server/models/bin）+ E2E
- 主题：全量移植 evaluateMergedAudit（四源合并 + AI-tell 计数 + restoreActionableAuditIfLost + revisionGate 三档）补齐 /revise 审核环；挂载 /analytics、/eval、/export 三端点

## 一、新增模块

### 1. `engine-rs/src/pipeline/merged_audit.rs`（新，~380 行）

移植 `packages/core/src/pipeline/runner.ts` L3593-3698 + L269/L1520：

- **`LlmAuditPort` trait**：LLM 审计注入面（options 全量透传，含 truthFileOverrides）。
  `FullCycleAuditor` 实现之（agent_router.rs 抽出 `run_audit` 私有方法，CycleAuditor
  实现改为委托，零重复）。
- **`evaluate_merged_audit`**：四源合并
  1. LLM 审计（continuity auditChapter，权威源）
  2. AI 痕迹（analyze_ai_tells，AITellIssue→AuditIssue 转换）
  3. 敏感词（analyze_sensitive_words；block 级词强制 `passed=false`）
  4. 长跨度疲劳（analyze_long_span_fatigue，仅进 issues）
  - `revision_blocking_issues` **按构造**排除疲劳（前 3 源拼接切片，非按分类名过滤——
    LLM 报出的同名分类问题仍计入，对齐 TS 注释语义）
  - `blocking_count`（warning+critical）/ `critical_count` 仅按 revisionBlocking 口径
- **`restore_lost_audit_issues`**：post 空响应（failed+零问题）时回退 previous 的 issues；
  `summary` 空串也回退（JS falsy 语义）。
- **`restore_actionable_audit_if_lost`**：restore 生效时连带 revisionBlockingIssues 与
  blocking/critical 计数回退 previous；aiTellCount 保留 next（TS 展开顺序）。
- **`RevisionGate`**（Strict/Lenient/Always，默认 Strict）：
  - `standard()`：三档文案逐字对齐 TS REVISION_GATE_STANDARDS
  - `should_apply(before, after)`：always 恒真 / lenient 仅不变差 /
    strict 不变差且 blocking 或 AI-tell 至少一项改善
  - `parse()`：未知值回退 strict

### 2. `engine-rs/src/utils/book_eval.rs`（新，~400 行）

移植 `packages/core/src/utils/book-eval.ts`（152 行）：

- `ChapterEval`/`BookEval`/`QualityTrendPoint`（serde camelCase）
- `compute_chapter_eval_score`：100 − 审计问题×5 − AI密度×20 − 段落警告×3（clamp 0-100，f64）
- `parse_chapter_range`：`"5"→5..=5`、`"abc"→1..=∞`、`"5-x"→5..=∞`（parseInt NaN 回退语义）
- `duplicate_title_count`：trim+lowercase，Set 计重复
- `compute_hook_resolve_rate`：表格行（行首 `|` 且含 ≥2 个 `|`）跳过分隔线/表头
  （`---`/`hook`/`伏笔`，大小写不敏感），`resolved|已回收|已解决` 计回收
- `evaluate_book_quality`：逐章（AI 密度/短段落警告）→ 伏笔回收率（pending_hooks.md）→
  重复标题（**全量 index**，非过滤窗口）→ compute_analytics（全量）→ qualityScore
  加权（0.3/0.25/0.15/0.2/0.1）→ qualityTrend
- 所有 `.length` 均 UTF-16 码元语义（encode_utf16）

### 3. `engine-rs/src/interaction/export_artifact.rs`（新，~520 行）

移植 `packages/core/src/interaction/export-artifact.ts`：

- txt/md：parts 数组按 TS join 语义拼接（txt joiner `"\n"`、md joiner `"\n---\n\n"`，
  md 头部出现双分隔线为 TS 同款怪癖，测试固化）
- epub：**零依赖手写 stored-zip writer**（Local File Header + Central Directory + EOCD，
  CRC32 表驱动，全条目 method-0 存储；epub 规范仅要求 mimetype 首位且不压缩）。
  EPUB 3 最小结构：mimetype → META-INF/container.xml → OEBPS/content.opf（dc 元数据 +
  nav properties）→ nav.xhtml → chapter-N.xhtml。TS 用 epub-gen-memory（Node），
  字节级不同但结构合规（PK 魔数/mimetype 首位/OPF 内嵌断言测试覆盖）
- `markdown_to_simple_html`：首个 `# ` 标题 + 非 # 行 `<p>` 化（escapeHtml 仅 & < >，& 先替换）
- `chapters_exported` = 过滤后索引长度（**含无文件的章**，对齐 TS）
- approvedOnly 过滤 `status === "approved"`；空章 → `Err("No chapters to export.")`

## 二、run_revise_chain 升级（books_routes.rs）

对齐 TS reviseDraft（runner.ts L1342-1672）主干：

1. **pre merged-audit**（四源）替代旧简化 pre-audit
2. unchanged 早退条件改为 TS 语义：`blocking_count == 0 && ai_tell_count == 0 && !explicit_revision`
3. 修稿驱动问题改为 `pre.audit_result.issues`（四源合并全集）
4. **post merged-audit**：temp 0 + truthFileOverrides（修稿器产出的状态卡/账本/伏笔池
   非占位时覆盖磁盘真相）——FullCycleAuditor 的 LlmAuditPort 实现支持 options 透传
5. `restore_actionable_audit_if_lost(pre, post)` → effective_post
6. **三档门控**（BooksRuntime.revision_gate，默认 strict；bin 经 `INKOS_REVISION_GATE`）：
   - 拒绝 → 200 unchanged + skippedReason（TS 逐字文案：`Manual revision kept original
     chapter: before blocking=N, critical=N, aiTell=N; after ...`）+
     **revisionDiagnostics**（standard/before/after/remainingIssues——前 6 条
     warning/critical，suggestion 空串省略）
7. applied 落盘升级：
   - 标题行**标准重构**（`# 第N章 {title}` / `# Chapter N: {title}`，取 index meta.title，
     不再保留原文件首行）
   - **ledger 回写补齐**（46 号暂缓件，`particle_ledger.md`）
   - **索引回写**（46 号缺失）：目标章 status=`passed ? ready-for-review : audit-failed`、
     wordCount、auditIssues=`[severity] description` 映射；**下游章** status=needs-revision +
     重审提示注入（旧提示先剥再注入）
   - 响应 status 从 `"revised"`（46 号偏差）改为 TS 语义
8. 缺章错误改为 **404**（Node 契约 `{"error": "Chapter not found"}`；46 号为 500）
9. `ChapterStatus` 增补 `NeedsRevision`（`needs-revision`）变体——TS 运行时强转写入但
   union 未声明（14 态）

## 三、新端点（router_books + bin）

| 端点 | 契约 | 要点 |
|---|---|---|
| GET `/books/:id/analytics` | server.ts L3500 | TS 怪癖：缺书 loadChapterIndex 返回 [] → **200 空统计**（404 分支不可达，E2E 固化）；compute_analytics（44 号前已移植）+ 新增 `AnalyticsChapter::from_meta` |
| GET `/books/:id/eval?chapters=` | server.ts L3551 | evaluate_book_quality 全量；缺书 → 200 qualityScore=80（加权空值怪癖，测试固化） |
| GET `/books/:id/export?format=&approvedOnly=` | server.ts L5641 | 200 原始字节 + Content-Type/Disposition 头；失败 500 `{"error":"Export failed"}`；txt/md/epub 三格式 |

BooksRuntime 增 `revision_gate` 字段（三处构造点同步）。

## 四、测试

- merged_audit 单测 8 例（四源合并/疲劳排除/block 强制失败/restore 两分支/门控矩阵/parse 回退）
- book_eval 单测 6 例（含 TS book-eval.test.ts 同款 fixture 用例 + 缺书 score-80 怪癖）
- export_artifact 单测 7 例（txt/md join 语义/approvedOnly/EPUB zip 结构/CRC32 向量）
- books_routes 单测：缺章 404（原 500 修正）/analytics 缺书 200/export 空章 500
- E2E books47_e2e 5 例：analytics 聚合（passRate=100：ready-for-review 也计通过）、
  eval 形状+区间过滤、export 三格式头与体+approvedOnly、**revise 门控拒绝**
  （pre 1 warning → post 3 critical → strict 拒绝 → revisionDiagnostics 全字段断言 +
  章节文件保持原文）
- 既有两处 revise E2E status 断言更新（revised → ready-for-review）

## 五、验证基线（全绿）

- `cargo test --lib`：905（+23）
- `cargo test --test golden_leaf`：76
- `cargo test --test e2e_write_next_contract`：13（+4）
- `cargo test --features export-bindings --lib`：1064
- `cargo clippy --lib --tests --bins`：零警告
- TS：185 文件 / 1798 测试

## 六、暂缓件

- normalizeDraftLengthIfNeeded / buildLengthWarnings / buildLengthTelemetry 未接入
  revise 链（TS applied 响应的 lengthWarnings/lengthTelemetry 字段未返回）——需
  length-normalizer 编排接入，留 48 号
- persistAuditDriftGuidance / syncLegacyStructuredStateFromMarkdown /
  syncNarrativeMemoryIndex / syncCurrentStateFactHistory 未接入（延续 46 号备案）
- reviseControlInput（治理输入 plan/memo/package/ruleStack）仍走 None（legacy 模式，
  延续 46 号）；TS v2 模式的 createGovernedArtifacts 复用待治理链端点化后接入
- EPUB 与 epub-gen-memory 字节级不同（结构合规即可，前端按文件下载消费）

## 七、下一步（48 号候选）

1. books 域剩余：GET `/books/:id/export-save`（POST 落盘变体）、`/books` 列表域
   （GET/PATCH）、`/books/:id/chapters` 章节域 CRUD
2. length-normalizer 接入 revise 链（补 lengthWarnings/lengthTelemetry）
3. sessions/state 配置域端点（Phase 2）
4. Node sidecar books 域按端点下线核对（/analytics /eval /export 已可切换）
