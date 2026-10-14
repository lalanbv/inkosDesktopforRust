# 699 号：chaptersWritten 深水项勘误与语义锁定（迁移路径上 487 契约不回退）

日期：2026-10-15。类型：test(studio)+docs/audit。前置：698 号（7291db99）。

## 一、勘误（本号核心产出）

697/698 号把 chaptersWritten 分歧表述为「fork 487 持久化进度链语义 vs 2.0 work manifest 驱动」——**代码级勘误：该表述不成立**。双端实现同源同语义：

- TS：`loadStudioBookListSummary` → `chaptersWritten = getNextChapterNumber(id) - 1`，而 `getNextChapterNumber` = `resolveDurableStoryProgress({bookDir}) + 1`（state/manager.ts:428，持久化进度链=index/files 一致性校验+连续前缀）；
- Rust：books_state_routes.rs:94 `chaptersWritten = next.saturating_sub(1)`，`next` 同为其持久化进度链镜像。

**双端 `chaptersWritten = next - 1` 逐字同构**。698 实测的 0 vs 1 / 1 vs 2 分歧全部是**执行深度与播种形态差**（写链跑没跑、连字符形态计不计），不是语义分歧——697 号记录中的「2.0 work manifest 驱动」系当时勘察不深的误判，随本号勘误。

## 二、语义锁定（迁移路径回归锁）

`work-migration-books.test.ts` 扩展：迁移播种补 `chapters/index.json`（进度链要件），断言迁移后 /books 的 `chaptersWritten === 1`——**487 号持久化进度链语义在 work 迁移路径上不回退**。此前 697 的该测试只锁书注册（/books 列出+bookCount），本号补上摘要语义面。

## 三、结果

- 测试 1/1 绿（迁移→注册→摘要语义全链）。
- 深水项处置定案：chaptersWritten 语义**无需对齐实现**（本就同构）；可见分歧已由 698 的双腿对称执行消解；本号以回归锁固化。697 号记录中的误判表述以本号勘误为准。

## 四、教训

1. **「A 驱动 vs B 驱动」的表述要下到函数级验证**：697 号从响应分歧反推语义差异，把执行深度差误判为实现语义差——审计表述的每一句归因都应有 file:line 证据，否则勘误成本会转嫁给后续号。
2. **迁移路径的语义回归锁**：迁移（migrateLegacyWorks）注册的书，其摘要语义必须与迁移前一致——一个 `chaptersWritten === 1` 断言把 487 号契约钉在迁移路径上，成本一行、守卫永久。
