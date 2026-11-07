# 905 号 · quality-trend 分歧定性轮：20 轮悬案闭环 + 差分器豁免精准化（三域活体首次全绿 72/0）

> 884 备案「共享面 quality-trend issueCount 1 分歧=既有非本轮待下轮定
> 性」悬置 20 轮后本轮闭环。活体取证（INKOS_DIFF_BODY_PREVIEW 双端响
> 应对照+keep 现场三连查）定案双端数据源结构差；豁免从整段剪除精准化
> 为字段级（结构面纳入比对）+补 :id 模板键（豁免此前从未命中）；**三域
> 活体分歧报告 71/1→72/0 首次全绿**。零生产代码改动（差分器单文件）。
> 验证：差分器 ✗=0、gate:ts 八步全绿（diff 步 36.4s 基线）、clippy 双 0、
> testgate 2044+584、U+FFFD 净。

## 一、活体取证链（三轮迭代）

1. **双端真值**（INKOS_DIFF_BODY_PREVIEW=4000 跑中响应）：node
   points=[{ch1,0,true},{ch2,0,true}] scoredChapters=0 无
   overallScore；rust points=[{ch1,0,88},{ch2,1,88}]
   scoredChapters=2 averageScore=88——四字段差异全景。
2. **keep 现场三连查**（两轮独立复跑同形态）：跑后现场 rust 账本
   review_metrics **0 行**+手动起引擎 curl points=[]——**现场终态≠
   比对时刻态**（收尾重置，与 899「--keep 终态已被 reject 级联改写」
   同族教训）——取证必须以跑中响应为准，现场考古对跑中态无效。
3. node 现场数据实体（index.json 仅 ch1 种子行无 observations+memory.db
   空壳无表）与响应 2 points 矛盾同解：终态被收尾重置。

## 二、定性（勘误 815，闭环 884）

- **双端数据源结构差=设计内**：node 正典派生（808 号：2.0 原生书无
  review_metrics 写链，账本缺席走 index observations 计数；
  scoredChapters/averageScore/overallScore 诚实空=协议扩展登记）；
  rust 账本路径（815 record_chapter_review_metric 写链）。
- **issueCount 语义不同源**：rust=审稿+**机检合成**合流（write_next
  run_post_write_checks 经 Arc 注入审核环+title-dedup/paragraph-shape
  多源 push 后 record 计数）；node=纯 review observations
  （submitSourcedReview 记录面）。ch2 1 vs 0 的 1=机检维度产物。
- **勘误 815 §三**：「mock 两协议审改内容不对称」归因不成立——mock
  审稿 JSON 恒 88 分恒零 issue（AUDIT_SCORES 全链未注入实证），与该
  分叉无关；815 登记的「mock 记录工具支（需状态机）」出口撤销。
- **豁免失效副发现**：该端点在差分器存在两种清单形态——BOOK 具体名
  段（豁免命中）与 :id 模板共享面扫描段（**独立比对段无豁免机制，豁免
  从未命中**，71/1 的 1 即此段每跑必报；884/885 备案的「豁免维持」
  实为「该段无豁免、报告面放行不判 gate」）。

## 三、落地（差分器单文件，零生产代码）

- 豁免精准化：`[["trend"]]` 整段剪除 → 字段级四路径（averageScore/
  scoredChapters/points/*/issueCount/points/*/overallScore）——**结构
  面（points 集合/chapter/passed/failingChapters/urgency/currentChapter）
  从此纳入比对**，等价监控面从 0 扩到全结构。
- 双键修正：BOOK 具体名键+`:id` 模板键同 paths（685 号「集合以模板串
  为键」口径）。
- 共享面扫描段接入 WAIVERS 机制（此前无豁免通道；命中后结构面比对、
  仅豁免字段剪除）。
- 定性注释全量入豁免表条目（活体证据+勘误链+取证方法），后续触点
  可溯源。

## 四、验证汇总

- 差分器：quality-trend ✗→✓，全报告 **✗=0 首次全绿（72/0）**；
  works 活体+legacy 面全绿维持；复跑两次稳定。
- gate:ts 八步全绿（test 99.8s/diff 步 36.4s 基线）；clippy:gate 双 0；
  cargo:testgate 2044+584（零 Rust 改动按纪律复跑）；U+FFFD 净。

## 五、后续出口

- 884/815 遗留悬案全部清偿（issueCount 分歧定性+豁免精准化；数值分
  协议缺口维持上游面登记）。work 域 L0-L3 全链收官状态维持。
- 出口清空回归常备：上游协议扩展或外部事件（pi1.0 key）或新真实需求
  或下一巡检周期。

**下一号自 906 起；出口清空回归常备；用户侧待办=Fork 推送 39 个提交。**
