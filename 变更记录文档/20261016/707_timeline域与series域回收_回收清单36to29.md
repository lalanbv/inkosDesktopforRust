# 707 号：timeline 域 + series 域回收（第五批，回收清单 36→29）

> 日期：2026-10-16（701-705 批量回收模式续号）
> 范围：packages/core（index 导出）/ packages/studio（server.ts 11 路由 + 行为测试 5 测）/ scripts（contract-diff 域表同步）

## 一、批次构成（11 条 HTTP 路由 = 清单 7 条目，全部零 SQLite 文件读写面）

1. **书籍时间线**（181 号 C4-b）：`GET/PUT /api/v1/books/:id/timeline`——GET 缺文件/坏载荷一律 `{timeline:null}`（可选面不制造 404 噪音）；PUT TimelineSchema 校验（version literal 1 + plotline id 唯一 refine + bookId 与路由一致），原子替换写 `story/timeline.json`（尾随换行）。
2. **时间线自动节拍开关**（189 号）：`GET/PUT /api/v1/books/:id/timeline-auto-beats`——book.json `writing.autoTimelineBeats` 读写；开启置键、关闭删键且 writing 空则整体移除（缺省省略语义）；原子替换写（截断的 book.json = 整本书不可加载）。
3. **书级系列绑定**（R20/390）：`GET/PUT /api/v1/books/:id/series-id`——book.json seriesId 读写，`isValidSeriesId` snake_case slug 守卫，null/缺省=解绑删键。
4. **系列正典共享存储**（R20/390）：`GET/PUT /api/v1/series/:seriesId/canon`——`.inkos/series/{seriesId}.json`，`parseSeriesCanonFile` 整包校验（version/seriesId/entries；坏条目跳过；name 码元序），GET 缺文件回落空基线 `{version:1, seriesId, entries:[]}`。
5. **系列书回填三面**（184 号 C3-b）：`POST .../extract`（守卫链：缺 sourceBookId/同书/源书不存在/无 canon 文件 → 400；LLM 抽取草稿原子落盘检查点）+ `GET .../existing`（`{content}` | `{content:null}`）+ `POST .../apply`（overwrite/merge 190 号写入粒度，(category,title) 去重，markdown 头部与 Rust render_backfill_markdown 逐字）。

## 二、引擎模块+index 导出+路由三件套

- core index 补导出（2.0 合并时被裁、模块在树，705 号同型倒查补齐）：`TimelineSchema`+类型（models/timeline.ts，181 号资产）、`isValidSeriesId/parseSeriesCanonFile/SERIES_CANON_VERSION 类型`（utils/series-canon.ts，**Rust series_canon.rs 注释明示的 TS 契约真源**、golden 向量双端共享）、`writeTextAtomic`（utils/atomic-write.ts，199 号单文件原子替换，Rust write_file_atomic 同构——EPERM/EBUSY 线性退避）。
- server.ts 11 路由 fork 原实现逐字对齐；仅两处有据偏离：①series-id PUT 由普通 writeFile 升级 `writeTextAtomic`（199 号书写面纪律 + Rust 侧本就原子；契约面不可观测）；②fork 的 state.bookDir 换当前 server 的 `join(root,"books",id)` 同构（701 批既有形态）。loadRawBookConfig 以 fork 同名 helper 局部化（原始 book.json 直读——loadBookConfig 强类型投影会裁掉 writing.autoTimelineBeats/seriesId 等未建模字段）。

## 三、contract-diff 域表同步与探针设计

- 域表 7 条目 fork-only→SHARED_FACE（36→29）；instShared 补 `:seriesId` 实例化（`series_sample` 合法 slug——fork-only 时代的 `series-sample` 连字符对双端都是 400 Invalid series id，且 Rust 错误体小写 `invalid` vs fork 大写 `Invalid` 会打成假分歧；合法 id 让 GET 走真实空基线全 body 对比）。
- 探针形态三备案：①PUT timeline body 用真实 BOOK_ID 动态构造——静态假 id 双双 400「bookId 与路由不符」掩盖快乐路径；②PUT auto-beats 用 `enabled:false` 幂等探针（键缺省=零净写入）——`enabled:true` 会置位夹具 book.json 污染后续 GET 基线；③extract/apply 空载荷探针只打守卫链（400/400）——真实载荷触发 LLM 调用不属契约面。
- **活体独立对跑：共享面 36 对比 / 0 分歧（新入 11 面：timeline GET/PUT 200、auto-beats GET/PUT 200、series-id GET/PUT 200、canon GET/PUT 200、existing 200、extract 400/400、apply 400/400）；fork-only 29/29 声明确认；上游新增 9/9+2；exit 0。**

## 四、门禁终态

| 门禁 | 结果 |
| --- | --- |
| studio vitest | 658/658（含新增行为测试 5 测） |
| typecheck | 四包 0 错误（gate:ts 内） |
| gate:ts 八步 | 全绿 exit 0 |
| contract-diff 独立对跑 | 36 面 0 分歧 exit 0 |
| bindings | 186 全绿 |
| rust-bin | 与 engine-rs 源码同源（本轮零 Rust 改动） |

## 五、教训

1. **探针实例值要先过双端校验器再入域表**——`:seriesId=series-sample` 连字符在双端都是 400，但错误体大小写差（fork `Invalid` vs Rust `invalid`）会把守卫面打成假分歧；合法实例值让守卫面退居后台、契约面（空基线全 body）上前。
2. **带副作用的写面探针要设计成幂等**——`enabled:false`+键缺省=零净写入；`enabled:true` 型探针会在双腿留下状态，污染同轮后续 GET 基线（本轮 GET 先于 PUT 排列 + 幂等载荷双保险）。
3. **动态段探针 body 不能静态造假 id**——PUT timeline 的 bookId 必须用探针已解析的 BOOK_ID 动态构造，否则双端一致的 400 恰好掩盖快乐路径（705 号「真实 id 实例化是入场券」的 body 侧延伸）。
4. **fork-only 清单条目 ≠ 均可回收**——health/debug/tools 属 Rust 超集端点（62 号加法面），回收对象要按「fork 契约全史有此键」甄别，域表声明域与迁移域语义不同。
