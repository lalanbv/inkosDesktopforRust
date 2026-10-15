# 712 号：全量验收轮（全量测试+代码审查+模拟真人走查）——实锤并修复 Rust 生产腿 Dashboard 整页短路

> 日期：2026-10-16（707-711 会话五提交的全量验收轮；704/706 同型）
> 范围：packages/studio（Dashboard.tsx works 域降级 + Dashboard.test.tsx 双向回归锁）

## 一、全量测试矩阵（707-711 五提交全树）

| 门禁 | 结果 |
| --- | --- |
| gate:ts 八步 | 全绿 exit 0 ×2（711 树 + 712 修复树各一轮） |
| cargo:testgate | 1933 + 584 双零漂移（711 号树） |
| clippy:gate | engine-rs 0 + src-tauri 0 |
| bench:gate | 带内通过（title_dedup 双项改进 -19.8%/-14.4% 在案） |
| review-matrix rust 腿 | **7/7 严格全绿**（B/C/D/F/G/G2/E，32-42s/场景） |
| review-matrix node 腿 | 软失败记账 exit 0（2.0 腿无 fork 审改旋钮，706 定线） |
| contract-diff | 44 面 0 分歧（gate 内 + 独立对跑双确认） |
| bindings | 186 全绿 |
| studio vitest | 670/670（含 712 新增双向回归锁） |

## 二、代码审查（逐 hunk 复审 707-711 diff）

radar.ts 纯增量无删改（2.0 scan() 未动，diff 零负行实证）；嫁接面 fork 逐字性抽查（series-id/timeline/extract/apply/review-mode/backup 与 fork 原实现逐字对齐；两处声明偏离：series-id PUT 原子写升级、extract 缺 Rust 侧「Target book not found」守卫=fork 原貌）；Rust 惰性语义复核（OnceCell memo 保交错报告顺序、Cow 借用 is_uppercase 前哨、duel 守卫前置与 dist 写入时序无冲突）；探针脚本 statusOnly/BOOK_ID 动态 body 均审无缺陷。两处既有协议差维持备案（review-mode 非 garbage writing 输入的 char-spread 边缘、radar analyze 无 body 落 2.0 scan 分支的 skill 门槛差——均非本轮引入，契约面探针不可达）。

## 三、模拟真人走查（Rust 引擎生产形态 + 浏览器黑盒）——实锤一处整页级 BUG

walkthrough-env rust 腿（生产形态：引擎静态托管 studio dist + mock LLM + fixture 数据）+ IAB 黑盒。六面点验：

1. **首页**：❗实锤 BUG（见下节）；修复后书卡/写作数据（近 30 天 2,466 字·完成章 2·通过率 100%）/运行遥测（累计 10 成功）/备份与恢复面板全部活体在位（截图留档）；
2. **书详情工作台**：四按钮/上下文面板（章节 01 镜中醒来 61 / 02 风起 2,405）/伏笔池 4 条彩签分类（悬念/情感/物品/世界观）✓；
3. **雷达页（708 面）**：免费扫榜+分析按钮空态门控正确禁用+历史回放——**信号卡 82%/65% 置信度、拥挤度（中等/蓝海）徽标、差异化机会、对标书、「从该选题开书」全要素活体渲染**（截图留档）；
4. **时间线页（707 面）**：情节线×章节矩阵渲染；**编辑态真人链路**——点单元格→改标题（镜中醒来→镜中醒·改）→保存→API 回读 `plotlines[0].cells[0]="镜中醒·改"` 落盘实证 ✓；
5. **审核模式（709 面）**：API 三态投影活体回读 `{bookMode:null, mode:"auto", projectMode:"auto"}` ✓（UI 开关在工具栏折叠区，组件测试覆盖）；
6. **备份面板（710 面）**：面板活体在位+「导出备份」点击链路正常（下载落盘属桌面壳行为，IAB 环境备案；端点已由契约对跑 statusOnly 200/200+双侧 roundtrip 测试锁定）。

### BUG：/works 404 短路 Dashboard 整页（Rust 生产腿自 688 号起）

**症状**：首页主区只有「Failed to load library / 404 Not Found」错误卡；书卡片、写作数据（R13）、运行遥测（R26）、备份恢复（R18/386——**710 号刚在 node 腿回收的面板，在 Rust 生产腿本来就不可达**）全部消失。

**根因**：`Dashboard.tsx` 的致命早退 `if (error || worksQuery.error)` 把 `/works` 的失败（695 号声明的上游新增未迁域，Rust 腿恒 404）与核心数据面 `/books` 的失败同等对待——works 恒 404 ⇒ `worksQuery.error` 恒真 ⇒ 整页早退。706 号备案只记了「首页预览 404」表象，影响面实为整个 Dashboard 主体（本次模拟真人走查实锤）。

**修复**：致命早退收窄为 `if (error)`（/books 失败才致命）；works 域按 706 号「双栈 pairing UI 按最弱引擎 defensively 类型化」教训优雅降级为无 otherWorks 分组（`otherWorks` 本就 `?? []` 容错）。

**双向回归锁**（Dashboard.test.tsx +2）：works 域 404 时主体可达（书卡在位、无错误卡、空态不误判）；/books 自身失败仍致命（与降级面区分锁定）。既有四测的统一 mock（books/works 同值）恰是此形态无覆盖的温床——分路径 mock 补齐。

**活体复验**：重建 dist 后 Rust 腿首页错误卡消失，四面板全在位（截图留档）。

## 四、教训

1. **「已知 404」的备案要写影响面而非表象**——706 号把「首页预览 404」当渲染层噪音备案，实际它经由 `worksQuery.error` 早退短路了整页四个面板；模拟真人走查的价值正是抓「每层单看都绿、合起来用户不可达」的缺陷。
2. **统一 mock 是分路径失败形态的覆盖盲区**——books/works 共用一份 mock 返回值时，works 特有的失败形态（上游新增域 Rust 腿恒 404）永远测不到；多数据源组件的测试要按 path 分mock。
3. **UI 走查要覆盖「回收面的对偶腿」**——710 号在 node 腿回收了备份面板，但该面板在 Rust 生产腿自 688 号起就不可达；「面在位」要双引擎分别活体确认，不能以契约对跑（API 层）代替 UI 层可达性。
4. **IAB 点击全超时的逃生通道**——渲染层命中测试拦截时，`evaluate` 内 JS click 是可靠兜底；截图与 DOM 快照分工（结构 vs 视觉+遮挡判断）。
