# 460 号：Analytics 数据分析页深链修复（hash 路由缺分支）+ 页面走查

- **日期**：2026-09-15
- **类型**:fix(studio) —— 路由深链缺陷修复（208/405 同款第三实例）+ 读面走查
- **关联**：208 号（章节深链）、405 号（radar 深链修复先例）、449 号（静态面热加载）
- **提交**：`packages/studio/src/hooks/use-hash-route.ts` + `use-hash-route.test.ts` + 本记录

## 1. 缺陷：Analytics 页深链/刷新回落 Dashboard（208/405 同款第三实例）

走查发现：直达 `#/book/:id/analytics` 或在数据分析页刷新，渲染的是 **Dashboard**。定位 `use-hash-route.ts` 三件套对 analytics 的缺位：
1. `parseHash` 无 `book/:id/analytics` 分支 → 解析落入默认 `{page:"dashboard"}`；
2. `routeToHash` 无 analytics case → 深链写入为空串；
3. `HASH_PAGES` 集合未含 "analytics" → `setRoute` 不写 URL。

按钮链路（`nav.toAnalytics` → route 对象）因 `setRouteState` 始终执行而可用，但 URL 不更新且刷新丢失——与 208 号（chapter 深链）、405 号（radar 深链）完全同款的缺陷第三实例。

**修复**：三件套补齐——parseHash 加 `^book/([^/]+)/analytics$` 分支（decodeURIComponent）、routeToHash 加 analytics case、HASH_PAGES 增 "analytics"。

## 2. 走查与验证

- 新增 3 路由单测：analytics 解析/URL 编码书名解码/hash 往返（use-hash-route.test.ts 34 测全绿）；
- 全量：studio vitest **106 文件 854 用例全绿**（+3）+ tsc 净 + dist 重建；
- 真机：`#/book/镜花水月/analytics` 直达渲染 ✓——三统计卡（总章数 2 / 总字数 123 / 平均字数 61）与状态分布条（audit-failed 50% / ready-for-review 50%）与 `/analytics` API 逐项对齐（截图在案）；
- **449 号热加载顺带实证**：dist 重建后引擎未重启，重载即服务新 bundle。

## 3. 遗留

- **待推送：433–460 共 28 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 备注结论：hash 路由三件套模式（parse/routeToHash/HASH_PAGES）在新增页面时易漏——本次 analytics 为第三实例，建议后续循环对 HashRoute 联合类型做穷举一致性测试（编译期或运行时遍历）。
- 下一个编号自 **461** 起。
