# 180 · W-C4-a 只读时间线视图 + C3-a 系列字段与 Dashboard 分组

- 日期：2026-09-07
- 模块：packages/studio（pages/BookTimeline.tsx + dashboard-book-groups.ts 新增、App.tsx、use-hash-route/nav/commands/breadcrumb/use-i18n、BookDetail/Dashboard、shared/contracts.ts）、packages/core（models/book.ts + index.ts + models.test.ts）、engine-rs（models/book.rs + 12 处初始化点补字段 + golden_leaf.rs）
- 类型：feat（产品级；首个 C 系 backlog 落地）
- 关联：[W-C3-C4 需求分解 v1](../../开发时SpecCoding'sPlan/inkosDesktop/05_UI体验设计/W-C3-C4_产品级功能需求分解_v1.md)（179 号）、对标 Plottr #10 / Sudowrite #1

## 一、范围

按 179 号分解文档落地两个可完全自主的切分：

- **W-C4-a 只读时间线**：书籍时间线页（回顾型网格，行=情节线/列=章），数据源 `GET /books/:id` 的 chapters/index.json，单条「主线」兜底；零引擎契约改动。
- **C3-a 系列数据面**：book.json 加 `series: { name, order }`（缺省省略），双端 schema 同步 + Dashboard 分组渲染。

## 二、实现

### 时间线页（BookTimeline.tsx）

- 路由 `#/book/:id/timeline`（与 settings 同款嵌套形态）：use-hash-route 类型/parse/routeToHash/HASH_PAGES 四点 + nav.ts `toBookTimeline` + App.tsx 渲染分支 + BookDetail 入口按钮 + recentToRoute case + breadcrumb 穷举 case。
- 网格：列头（情节线 + 章号）+ 每线一行，单元格（标题/字数/状态色）点击跳章节阅读器（`nav.toChapter`）。章多时横向滚动。
- 状态 13 种归并 5 组展示色（done/review/failed/wip/imported），`data-tone` 属性暴露给测试。
- 情节线推导独立 `plotlines` useMemo——C4-b 引入 `story/timeline.json` 后在此扩展多线，组件骨架不变。

### 系列字段（C3-a）

- **TS**：`BookSeriesSchema`（name min1 / order int min1）+ BookConfigSchema 加 `series` optional；index.ts 导出 BookSeries/BookSeriesSchema。
- **Rust**：`BookSeries` 结构体 + `BookConfig.series: Option<BookSeries>`（`skip_serializing_if`——缺省时序列化完全省略，与 TS `optional` 输出等价，books 列表 DTO 双端零差）。12 处测试/装配初始化点补 `series: None`。
- **Dashboard 分组**：`groupBooksBySeries` 纯函数（泛型 `T extends WithSeries`，兼容本地与 contracts 两处 BookSummary 定义）——系列聚组按卷序升序、组间按最小 order、散书归末尾无组头组；组头「系列名 · N 本」+ 书卡卷序徽标 `系列名 · #order`。

## 三、验证

| 项 | 结果 |
| --- | --- |
| studio typecheck（双 tsconfig） | 干净 |
| studio vitest 全量 | **748/748 绿**（新增 7：时间线 4 + 分组 3） |
| packages/core vitest | models.test.ts 98 绿（含 series optional/roundtrip/非法拒绝） |
| engine-rs cargo test 全量 | 全绿：1233 lib（+series roundtrip 1）+ 194 + 70 + 8 |
| engine-rs clippy --all-targets -D warnings | 零告警 |
| src-tauri cargo test | 全绿 466+ |
| INKOS_DUEL=1 duel | 8/8 真跑绿（books 列表缺省无 series 字段，双端等价不受影响） |
| 浏览器级验证（vite→Rust 引擎直连，fixture 双系列书+散书） | ①`/api/v1/books` series 透出（散书省略字段）；②Dashboard 渲染「测试系列」组头 + 「系列 · 2 本」+ 卷序徽标 #1/#2 + 散书平铺；③时间线页网格完整（列头/主线行/3 个可点击单元格 + 统计行）；④BookDetail「时间线」按钮 → 点击跳转 → 页面标题命中 |

## 四、过程记录

1. fixture 里手写的 chapters/index.json 被引擎忽略并按 markdown 正文重建（status 重算 ready-for-review/wordCount 重计）——引擎有 index 自愈机制，手写 index 不是可靠输入源；时间线数据链路不受影响（chapters 数组正常）。
2. BookConfig 字面量初始化点散布 12 处（含 golden_leaf 测试 fixture）——加必填字段前先 grep 初始化点评估爆炸半径；`Option` + `skip_serializing_if` 是双端加法变更的安全形态（缺省即不可见）。

## 五、遗留

- **C4-b**：`story/timeline.json` 多线数据面 + write-next 可选生成（默认关闭，需产品决策已登记）；组件 plotlines 推导已预留。
- **C3-b/c**：回填引擎管道与向导化（写入粒度需产品决策已登记）。
- 书卡「0 章」显示为 `chaptersWritten = next-1` 语义（approved 才计入 next）——非本批范围，如需调整另立批次。
- 推送须在 Fork 图形端执行（既有约定）。
