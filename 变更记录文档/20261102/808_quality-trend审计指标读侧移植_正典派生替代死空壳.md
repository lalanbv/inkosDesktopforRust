# 808 号：quality-trend 审计指标读侧移植——正典派生替代死空壳，协议级残差定界

日期：2026-11-02。前置：797 六端点分诊 / 807 收口登记（quality-trend `/currentChapter` 分歧：node 200 但 currentChapter 0 / points [] / urgency [] vs rust currentChapter 3 / points [ch2 88] / urgency [H03…]）。

## 一、定性

- 双侧处理器**同形**：都读 `books/<id>/story/memory.db` 的 `review_metrics` + hooks 投影；账本缺席即返回 `{currentChapter 0, points [], urgency []}` 死空壳。
- TS 全仓**无** review_metrics 写入者（仅 `MemoryDBReadonly.listReviewMetrics` 只读）——2.0 原生书的 node 腿永远空壳。rust 在 write_next 审改后记录 `{overall_score, passed, critical/warning/info 计数}`。
- 2.0 审改协议（`submit_chapter_review`→`AuditResult`）**没有** overallScore/passed/严重度——审稿人数值分与 verdict 不可从正典派生。这是真正的存储/协议边界（非路径错）。

## 二、修复（读侧移植）

`packages/studio/src/api/server.ts` quality-trend：
- 账本在位（1.x 迁移书）→ 原路径不变；
- 账本缺席 → **正典派生**：章节索引中带 `observations` 键的行=已审（`reviewChapter` 写入；导入种子无该键=未审）；`issueCount`=assessment "issue" 观察数；`passed`=无 issue 观察（2.0 语义：有 issue 即触发修订）；`currentChapter`=最后已审章+1（账本路径同语义，无已审→1）；`urgency` 从正典 `state/hooks.json`（798 promises 同源）。`scoredChapters/averageScore` 诚实为空。
- 终态排除集 resolved 组 + **superseded 组**（805 撤回语义），rust `ops_routes::get_quality_trend` 同步同集。

`scripts/walkthrough-fixture.mjs`：node 种子去 `observations: []`（792 转 optional 后的旧必需键残迹；带空数组会把未审导入章算作「已审零发现」）。

测试：`quality-trend-canonical.test.ts` 两场景——索引三行（导入无键 / 一 issue+一 observation / 零发现）+ hooks 三态（open/resolved/superseded）→ currentChapter 4、points [{2,false,1},{3,true,0}]、scoredChapters 0、failingChapters [2]、urgency 仅 H01；无账本无索引 → currentChapter 1 活壳。typecheck 0。

## 三、活体（推进度量）

diff 报告模式 71/2 维持，quality-trend 首叉 `/currentChapter`（0 vs 3）→ `/trend/points` 长度（2 vs 1）：**currentChapter 3=3 ✓、urgency 首项同（H03/80/explicit）✓**。残差三件：
1. node 多出 chapter 1 点：TS 2.0 首次 write-next 的 resync 阶段**审计导入章**（日志「审计第1章」）而 rust 不审种子章——执行深度差（698 族）；
2. chapter 2 `issueCount` 0 vs 1：mock 两协议审改内容不对称——rust 腿审稿 JSON 契约（645/646，恒 88 + issues）vs TS 腿 `submit_chapter_review` 直提零发现（发现须先经记录工具，mock 未实现该支）；
3. `overallScore`/`averageScore`/`scoredChapters`：2.0 审改协议无数值分——需上游契约扩展后 TS 才能记录（795 契约面）。

## 四、验证

cargo:testgate engine-rs 47 目标 1970 + src-tauri 25 目标 584 + RustSec 双 0；clippy 双 0；gate:ts 八步全绿（含 engine-contract-diff + legacy 11/11）；权威 pnpm -r test core 178/983 + studio **135/719**（+1 文件 +2 测）+ cli 23/81。

## 五、登记（695 工项余项）

- quality-trend 残差三件（上文）：执行深度差定性维持；mock 记录工具支（与 805 mock 撤回支同类）；数值分协议扩展（上游面，非本仓单方可决）。
- roster-candidates 移植族；TS 计数口径定性；rust 重建去重；UI superseded 徽标。

## 六、教训

1. **「同形处理器」不等于「同数据」**：双侧代码逐字同款时，分歧一定在数据生产侧——先问"谁写这张表"，TS 零写入者即答案。
2. **读侧移植的边界由协议决定**：能从正典派生的（已审章/issue 计数/紧迫度）立刻派生；协议里不存在的（数值分/verdict）不要用启发式伪造——诚实为空 + 精确登记，比假收敛更有价值。
3. 夹具种子携带的「旧必需键」会伪造审改痕迹——schema 字段转 optional 时要回查种子是否该随之删键。
