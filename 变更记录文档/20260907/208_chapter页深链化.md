# 208 号：chapter 页深链化——207 号 backlog 首项实施

- 日期：2026-09-07
- 分支：develop
- 关联：207 号（复评发现 chapter 深链缺失）、198 号（章节面包屑书名化——深链页面的呈现基础）
- 推送核验：origin/develop 仍停 6bc65564——**201–207 七提交均未推送**；本批次后本地领先 8。两项默认值无新答复。

## 一、问题（207 号 backlog #3）

`page:"chapter"`（章节阅读器/工作台）是 state-only 路由：`HASH_PAGES` 集合不含、`routeToHash` 无映射——**刷新即回首页、URL 无法分享/收藏**。analytics/truth/doctor 等系统页维持 state-only（doctor 前例，设计如此），但 chapter 是内容页，深链是真实产品缺口。

## 二、实施（use-hash-route.ts 三处 + 测试）

1. **parseHash**：新增 `/^book\/([^/]+)\/chapter\/(\d+)$/` 解析（bookId percent-decode、章号数字化；非数字段不匹配回落 dashboard——不误吞）。
2. **routeToHash**：`case "chapter"` 序列化为 `#/book/{id}/chapter/{n}`（bookId encodeURIComponent）。
3. **HASH_PAGES**：加入 `"chapter"`——setRoute 对象跳转时回写 URL（从工作台/列表点进章节，地址栏同步为深链）。

呈现面零改动即就绪：面包屑（198 号书名化）与标签页（routeTitle→deriveBreadcrumb 的 chapter 分支）此前已处理 chapter 路由对象，深链只是打通 URL 侧。

## 三、验证

- 新测试 `chapter-route.test.ts` 4 用例：解析/中文 id 解码/round-trip（含编码往返）/非数字章号拒绝。
- studio typecheck（双 tsconfig）✓、vitest **89 文件 790 用例**（+4）全过、dist 重建。
- 浏览器复验（生产形态 engine + 新 dist，按 207 号冒烟规范重启后走查）：
  - **深链直开** `#/book/b1/chapter/1` → 章节工作台完整渲染（面包屑「首页 / 夜港风云 / Chapter 1」，Edit/Split/Approve/Reject/Brief 面齐备）；
  - **刷新保持** → URL 不变、仍渲染章节工作台（此前行为：刷新即回书籍列表）。

## 四、结论

chapter 内容页深链闭环：直达/刷新/分享/收藏均可用；hash 双向同步（外部直达与内部跳转一致）。剩余 state-only 页（analytics/truth/doctor/logs 等）维持设计不变。
