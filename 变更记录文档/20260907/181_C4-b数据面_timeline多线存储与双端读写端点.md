# 181 · C4-b 数据面：story/timeline.json 多线存储 + 双端读写端点 + 时间线多线渲染

- 日期：2026-09-07
- 模块：engine-rs（models/timeline.rs 新增、models/mod.rs、server/books_state_routes.rs 端点、server/mod.rs 生产装配）、packages/core（models/timeline.ts 新增 + index.ts）、packages/studio（api/server.ts 端点、pages/BookTimeline.tsx 多线化 + 测试、api/server.test.ts）
- 类型：feat（加法变更；生成端点默认关闭）
- 关联：180 号（时间线视图，预留 plotlines 推导）、179 号分解文档（C4-b 切分）、174 号教训（测试形态 ≠ 生产形态）

## 一、范围

- **timeline.json（version 1）**：`books/{id}/story/timeline.json`，schema = `{ version:1, bookId, updatedAt, plotlines:[{ id, name, cells:[{ chapter, title?, note? }] }] }`——cells 稀疏（只为有节拍的章建条目）；plotline id 唯一；version 为 literal 1（TS `z.literal` ↔ Rust `deserialize_v1`）。
- **双端读写端点（加法）**：`GET /api/v1/books/:id/timeline`（缺文件/坏载荷 → 200 `{"timeline":null}`，可缺失面不制造 404 噪音；坏载荷附 tracing 警告）+ `PUT`（schema 校验 + id 唯一 + bookId 与路由匹配，非法 400）。
- **生成端点默认关闭**：write-next 顺手产出节拍的能力待产品决策（成本/默认开关），本批不实现——数据当前只来自 PUT 或外部写入。
- **BookTimeline 多线化**：timeline plotlines 非空 → 多线网格（planned 紫色 tone、note 副标题、无 title 格用章号兜底标题、列轴取 cells 章号并集）；缺省回退 180 号单线兜底——组件骨架零改动。

## 二、验证

| 项 | 结果 |
| --- | --- |
| engine-rs cargo test 全量 | 全绿：**1237** lib（timeline 单测 3 + 端点集成 1）+ 194 + 70 + 8 |
| engine-rs clippy --all-targets -D warnings | 零告警 |
| INKOS_DUEL=1 duel | 8/8 真跑绿 |
| studio typecheck（双 tsconfig） | 干净 |
| studio vitest 全量 | **751/751 绿**（BookTimeline 6 用例含多线/回退双分支；server.test timeline 端点用例；core mock 透传真实 TimelineSchema） |
| 浏览器级端到端（vite→Rust 引擎直连） | ①GET timeline 返回双线；②时间线页多线渲染：主线行（风起/危机格）+ 感情线行（第2章 初遇——章号兜底标题）、列轴为 cells 章号并集（第2章仅存在于感情线）；③PUT 新增「反派线」→ GET 反映三线；④非法 PUT（bookId 不符/坏 JSON）各回 400 语义 |

## 三、过程教训

1. **集成测试的内部 router ≠ 生产装配链**：timeline 路由最初挂在 books_state_routes.rs 文件内的测试 router 上（该文件 L1431 的 truth 注册也是同款内部 router），集成测试全绿但真 bin 上 404——生产装配在 server/mod.rs 的 router_books 链。这是 174 号「测试传参 ≠ 生产传参」的又一变体：**测试路由 ≠ 生产路由**。新端点必须核对本批实际生效的装配函数；浏览器级冒烟（真实 bin + 真实请求路径）再次证明是装配层缺陷的最后一道闸。
2. **vi.mock 全局 mock 的透传税**：studio 对 `@actalk/inkos-core` 是文件级整体 mock——core 新增导出后，消费它的 server.ts 在测试里拿不到（动态 import 报 No export defined → 500）。解法沿既有先例：`importOriginal` 取真实 `TimelineSchema` 透传进 mock 返回体。跨包新增「被 server 消费的导出」时记得同步测试 mock。
3. TS `z.literal(1)` 的 Rust 等价物用 `deserialize_with` 收敛（version 只认 1），序列化恒输出 1——双端 literal 语义由模型层而非端点层保证。

## 四、遗留

- **C4-c**：时间线单元格手动编辑 UI（PUT 端点已就绪，前端表单/弹层待做）+「从时间线节拍发起写作」接线。
- **生成端点**：默认关闭待产品决策（179 号登记）——决策后以设置项形态接入 write-next 管线。
- W-C3-b/c（回填管道与向导）与 C3 写入粒度决策维持登记。
- 推送须在 Fork 图形端执行（既有约定）。
