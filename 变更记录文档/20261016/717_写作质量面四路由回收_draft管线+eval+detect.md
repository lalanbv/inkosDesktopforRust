# 717 号：写作质量面四路由回收——draft 管线 + eval + detect 双路由（回收清单 11→7）

> 目标线：自主持续开发（TS/Rust/桌面壳）。BookDetail「重修设定/质量评估/
> 反检测」域；ai-tells + book-eval + production-bindings 三模块连片移植。

## 一、连片移植（fork 逐字 + 2.0 适配标注）

| 模块 | 行数 | 说明 |
| --- | --- | --- |
| agents/ai-tells.ts | 161 | 零依赖纯规则（dim 20-23 结构检测；无 LLM） |
| utils/book-eval.ts | 153 | 启发式质量评估（依赖 ai-tells ✓/analytics ✓/chapter-filename ✓） |
| skills/production-bindings.ts | 56 | 生产能力绑定表（R44/566；merge/ids 两件 2.0 已移 activations.ts 同体——只取独有件不重复定义） |

连带类型收敛（Rust 真源镜像）：
- ChapterMetaSchema 补 `auditIssues` 可选键（Rust audit_issues serde default
  空 vec——strict 自毁键预防第五例，存量索引无此键兼容）。
- AnalyticsData 补 `auditPassRate`（fork 逐字 status 基计算：audited=非
  drafted/drafting/card-generated；passed=ready-for-review/approved/published；
  Rust analytics audit_pass_rate 同字段——book-eval qualityScore 加权消费）。

## 二、writeDraft 嫁接（护栏/内核分层第三次应用）

- **壳与输入段 fork 逐字**：book 锁 + 530 号激活透传 + prepareWriteInput +
  WriterAgent.writeChapter + DraftResult 形状。
- **落盘段 2.0 形态**：writer.saveChapter 四参一体 truth/索引（fork 的
  legacy structured state 与叙事记忆 sqlite 同步在 2.0 由 runtime-state
  体系取代）；ChapterMeta newEntry 含 status:"drafted"（714 键）。
- **篇幅告警 2.0 bounds 语义改写**：LengthSpec 双树有意分歧（fork 五区间
  softMin/hardMin 必填 vs 2.0 minChapterLength/maxChapterLength 可选 bounds
  ——不回滚 schema），buildDraftLengthWarnings 按可选 bounds 判缺省不判。
- **激活语义 2.0 化**：PipelineConfig 无 activatedSkills 字段（fork 旧承载）
  ——经 operationContext 注入（server runWithAgentContext 包裹 +
  resolveProductionSkillActivations("longWriting")）。
- core index 导出：analyzeAITells/evaluateBookQuality/
  resolveProductionSkillActivations/findChapterFileByNumber + 类型族。

## 三、四路由嫁接（fork 逐字 + isSafeBookId）

- **draft POST**：fire-and-forget（立即 {status:"drafting"}）+ SSE 广播
  draft:start/complete/error；激活经 runWithAgentContext 注入。
- **eval GET**：evaluateBookQuality 启发式（零 LLM）；chapters 过滤参数。
- **detect/:chapter + detect-all POST**：analyzeAITells 结构规则（零 LLM）。

## 四、探针与域表（11→7，+4 探针）

- eval/detect/1/detect-all：零 LLM 面 **全 body 对比**（fixture 同文双端
  同构——contract-diff 64/0 实证阈值无漂移，ai_tells.rs/book_eval.rs 对偶
  活体互证）。
- draft 表尾（fire-and-forget 后台写第 2 章防污染同轮 GET 基线）状态码。
- 度量：**共享面 64 对比 0 分歧 + fork-only 7/7 + 上游新增 9/9**。

## 五、回归锁（writing-quality-recycle-routes.test.ts，3 测）

1. detect：AI 腔样本命中（转折公式规则）+ 干净章零命中 + 404 + 全量扫描
   排序 + 坏 id 400。
2. eval：auditPassRate status 基（approved 章唯一 audited → 100；仅草稿章
   → audited 空 → 缺省满分 fork 语义）+ qualityScore 加权 + chapters 过滤。
3. draft：fire-and-forget 200 drafting + 坏 id 400 + 后台链竞态等待收口。

## 六、mock 基建（draft 链三工具协议分派）

draft 链在 2.0 是**三段 submitStructured 工具协议**（fork 全为纯文本协议）
——mock 按 tools[] 精确匹配逐支补齐：
1. `submit_chapter_memo`（planner，714 已备）；
2. `submit_chapter_draft`（writer 主体：title+content，复用 WRITER 正文常量
   ——规避 ai-tells 全表的走查正文）；
3. `submit_runtime_state_delta`（settle 结算：factOps/hookOps/章摘要——
   **hookOps.mention 空**：台账空时 mention H01 被校验拒「Unknown hook id(s)」
   ——契约「只收台账既有 id」当场生效）。
- 诊断基建：mock 增 tools 请求日志（`[mock] tools=...`）——链路卡点定位
  从猜到看（三次卡点 memo→draft→settle 各一轮即清）。
- fixture 补播 book_rules 双文件（readBookRules 硬读——完备书目录必备件，
  建书链产物形态；缺省时 draft/write 链 ENOENT 恒败）。

## 七、门禁与活体走查（node 腿，IAB）

- 矩阵：studio 685/685（+3）· core 932/932 零漂移 · gate:ts 八步全绿 ·
  contract-diff 64/0（gate 内复跑确认）。
- **真人链路**：
  - eval API（qualityScore 49/auditPassRate 0——fixture 章无 status 键=
    存量形态计入 audited 且不通过，fork 同输入同输出语义保真）→ UI
    「质量评估」点击 alert。
  - detect API 第 1 章 0 命中——与 mock 正文「规避 ai-tells 全表」设计
    互证（644 号注释活体验证）。
  - **draft 全链**：SSE draft:start → **draft:complete** → 第 2 章「玉符
    异动」drafted 2405 字落盘 → **UI 章节表 02 行「草稿」徽标渲染**（714
    status 键 UI 投影再次活体）。
- 走查小坑：draft 链三次卡点（memo 工具命中后 draft 工具→settle 工具→
  mention 校验）各一轮 mock 日志即清——**工具协议链的分派完备性要用请求
  日志逐段验证，不猜**。

## 八、教训

1. **fork 纯文本协议 vs 2.0 工具协议**是回收面的系统性差异（planner/writer/
   settle 三段全中）——LLM 管线面嫁接先核协议形态再补 mock 分派。
2. LengthSpec 双树有意分歧（bounds 化）——「被裁」与「演进」要 diff 定性，
   演进件做语义改写不回滚。
3. fire-and-forget 路由的测试要等后台链收口（与 afterEach 清理竞态）。
4. mock tools 日志是管线链卡点的最小诊断基建（一段一清）。
5. auditPassRate/auditIssues 的 Rust 真源镜像顺路收敛——启发式评估面的
   字段在 Rust 有对偶即安全补。

## 涉及文件

- packages/core/src/agents/ai-tells.ts、packages/core/src/utils/book-eval.ts、
  packages/core/src/skills/production-bindings.ts（新增 3）
- packages/core/src/pipeline/runner.ts（writeDraft + DraftResult + 告警小件）
- packages/core/src/models/chapter.ts（auditIssues 键）
- packages/core/src/utils/analytics.ts（auditPassRate）
- packages/core/src/index.ts（质量面导出）
- packages/studio/src/api/server.ts（四路由 + 变体去重）
- packages/studio/src/__tests__/writing-quality-recycle-routes.test.ts（新增 3 测）
- scripts/walkthrough-mock.mjs（draft/settle 工具支 + tools 日志）
- scripts/walkthrough-fixture.mjs（book_rules 补播）
- scripts/engine-contract-diff.mjs（SHARED_FACE +4 / FORK_ONLY −4）
