# 469 号：daemon/doctor/genres/logs/style/truth 深链批量补齐 + SPA 入口 no-store 加固

- **日期**：2026-09-15
- **类型**:fix(studio) + fix(engine) —— 460 同类缺陷批量清零（6 页）+ SPA 入口缓存头
- **关联**：460 号（analytics 第三实例与备案）、208/405 号（前两例）、449 号（热加载）
- **提交**：`packages/studio/src/hooks/use-hash-route.ts` + `use-hash-route.test.ts` + `engine-rs/src/server/static_routes.rs` + 本记录

## 1. 缺陷：六个页面深链/刷新回落 Dashboard（460 同类批量实例）

系统排查 `App.tsx` 全部 `view.page` 渲染分支 vs `parseHash` 解析分支，差集 6 页：**daemon / doctor / genres / logs / style / truth**（truth 为 per-book 面 `#/book/:id/truth`）。六页在 App 正常渲染但 hash 解析无分支——直达 URL/刷新回落 Dashboard，且 URL 不更新。

**修复**：
- `parseHash` 补 6 分支（daemon/doctor/genres/logs/style 五平铺 + truth 正则 `book/:id/truth` 带 decode）；
- `PAGE_SPEC` 六条 null → 非 null（toHash + writable: true + sample）——461 穷举闸下此为编译强制项，round-trip 穷举测试自动覆盖新页。

## 2. SPA 入口 no-store（449 遗留缓存面收口）

真机验证时发现浏览器 HTTP 缓存旧 index.html（无缓存头）会跑旧 bundle——SPA 入口陈旧使整站跑旧代码。`spa_fallback` 的 200 响应补 `Cache-Control: no-store`（assets 为内容寻址哈希名可长缓存，不受影响）。新增单测 `spa_fallback_index_carries_no_store` 锁定；同步全量 cargo test 39 套件全绿、clippy 双 crate 0 告警。

## 3. 真机验证（新根环境 + 重建 dist）

六页深链逐一验证全部正确渲染：`#/daemon`→守护进程控制、`#/doctor`→环境诊断、`#/genres`→题材、`#/logs`→日志、`#/style`→文风分析、`#/book/镜花水月/truth`→真相文件（hash 保持、非 Dashboard 回落）。

## 4. 门禁

- 路由单测 37 全绿（含 461 穷举 round-trip/键集合断言对 6 新页自动覆盖）；
- 全量：cargo test 39 套件全绿（+1 no-store 测）；clippy:gate 双 crate 0 告警；studio 109 文件 872 用例全绿 + tsc 净 + dist 重建。
- 环境停净零残留。

## 5. 遗留

- **待推送：433–469 共 37 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 下一个编号自 **470** 起。
