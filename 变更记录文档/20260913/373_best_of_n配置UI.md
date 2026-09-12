# 373 号：G11 配置 UI 收官——best-of-n 端点与书籍级开关

- 日期：2026-09-13
- 类型：端点 + UI（双端镜像）
- 规划依据：371/372 号 G11 批的配置 UI 收尾件

## 做了什么

1. **Rust 端点**（books_state_routes.rs，沿 chapter-review-mode 先例）：
   - `GET /api/v1/books/:id/best-of-n`——读 book.json `governance.bestOfN`
     （缺省 `{enabled:false}`）；
   - `PUT /api/v1/books/:id/best-of-n`——body {enabled, candidates?,
     minScore?}；candidates clamp 2–3、minScore clamp 0–100；governance
     缺省创建；原子写回（write_file_atomic，372 号同款）；404 语义对齐
     review-mode 先例。
2. **TS 端点**（server.ts 同款 GET/PUT，读写 book.json）。
3. **UI**（BookDetail）：工具条新增「best-of-N」开关按钮（GitBranch 图标，
   与 autoBeats 开关同形态）——开启即多版选优；乐观更新失败回退。

## 过程教训

- TS 端点插入用跨分号大正则匹配既有块，提前截断破坏 chapter-review-mode
  GET 路由（server.test.ts 抓出 INTERNAL_ERROR）——git checkout 干净基线
  后改用 Edit 工具精准单点插入重做。**大文件插入禁用跨分号大正则。**
- Rust 路由注册 `get().put().with_state()` 链完整性、返回二元组一致性由
  编译器逐处抓出修正。

## 验收

- studio：tsc 干净；vitest **92 文件 / 802 用例**全绿（server.test.ts
  全量回归，含被破坏后重建的 review-mode 路由）。
- engine-rs：cargo check 干净。

## 里程碑

- **G11 best-of-N 完整收官**（371 契约层 + 372 接线 + 373 配置 UI）。
- 二轮规划 R1–R9 与择机项 G11 全部落地。330–373 四十三笔待推送。
