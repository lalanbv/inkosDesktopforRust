# 1444 号：Rust 面 deep-scan 分诊终审 + books 面对称补缺轮

## 目标与范围

/goal 逐功能增量续轮。非冗余增量=1442 安全分诊的**覆盖度缺口**：彼时只分诊了
hook 点名的 TS 面（12 条），密封扫描 622H 的 **Rust 面占比从未清点**。方法=
路径精确归因（真源码 `.rs` 后缀 vs 烤入快照路径）。

## 归因澄清（先决）

622H 的面积分布：packages 111 / **src-tauri 2224 / engine-rs 15** / scripts 7。
src-tauri 的 2224 条样本=**`engine-rust/static/api/server.js`**——烤入快照
（打包快照链：studio 构建产物的烘焙副本），即同一 server.js 业务假设的重复
计入，非 Rust 源码发现，随 TS 面处置。**真 Rust 源码发现=15 条**。

## 15 条逐族分诊

| 族 | 条数 | 判定 |
|---|---|---|
| 硬编码凭据（cover.rs:840 / secrets.rs:416 / host_api.rs:1927） | 3 | **误报**：测试夹具固有碰撞（`test-key-` 占位形 / secrets 解析器边界串 / fake env 名值分置——859/861 已备案惯用形），零真凭据，均不在 hook 拦截集 |
| read_file_or_default/empty 路径穿越（writer×4 / mem×3 / 经 1 跳×2 / backup×1） | 10 | 混合：调用点全部**常量文件名 join**；基座 `book_dir(book_id)=books_dir().join(book_id)` **裸 join 无消毒=真缺口（修）**；snapshot_book=测试 helper 误报 |
| 跨文件污点 medium（book_eval / export_artifact） | 2 | 同链，随本修覆盖 |

威胁模型注记：engine API=loopback-only + 回环 Origin 守卫（173 号 W-A4a，
远端 Origin 403）——但本仓自身纪律（genre 632 对称校验、static/ops/style
三处 rejects_traversal 测试）在别处明确拒绝穿越，books 面=同族不一致缺口。

## 修复（books_routes 8 入口对称补缺，TS 契约对齐）

TS 腿早已边界校验（`normalizeApiBookId`/`isSafeBookId` → 400
INVALID_BOOK_ID，server.ts:408）——Rust books 面=不对称缺腿。修复沿用仓内
现成惯用形（`session::is_safe_book_id` 路径安全子集，ops_routes 同形）：

- `book_id_is_unsafe` + `invalid_book_id_response`（400 INVALID_BOOK_ID，
  onError 形状逐字对齐 TS）；
- 8 handler 挂卫兵：plan/settle/compose/consolidate/analytics/eval
  （impl IntoResponse 直返元组）；draft（签名→`axum::response::Response`，
  尾返回 `.into_response()` 等价形）；export（本就 Response 形）；
- **必红锁测** `settle_rejects_path_traversal_book_id`：`..%2F..%2Fescape/plan`
  恰红 400 INVALID_BOOK_ID（genre 632 先例同族、TS server.ts:408 同码）。

## 活体矩阵

- books_routes 模块 **13/13 绿**（编译自纠一处=Value/json! 需全限定
  serde_json）；锁测显式单跑恰绿；
- **全量 testgate：engine-rs 52 目标 2115 passed（+2 含锁测）+ src-tauri 593
  passed**，exit 0；
- clippy:gate 双 crate 0 告警；bench 负载 8.24 备案延续（1443 引证在位）。

## 首跑 flake 备案（非本轮引入）

testgate 首跑 `pending_abort_tests::marker_keys_are_isolated_by_root_and_session`
红（agent_route.rs:2764）→ 隔离 3×6/6 绿 + 全量复跑绿 = **并发 cap 逐出竞态**
（registry cap 逐出最旧标记，并行灌入下误逐出本测的早插标记）。该测试并行
不安全——备案（与我改动零态交互：agent_route 与 books_routes 无共享标记面）。

## 结论

1442 安全分诊 Rust 面覆盖度闭环：15 条=3 误报夹具+10 常量名/基座缺口（其中
book_dir 裸 join 一处真缺口已修）+2 同链 medium。零真凭据、零未处置真阳性。
零新问题论证：守卫只增拒入面（8 入口早退 400），合法 id 零影响（现存 id 均过
is_safe_book_id——模块 13/13+全量 2115 实证）。U+FFFD 净。

出口=下一巡检周期/earendil-mz 单包合并（触发条件制已更新）；TS 腿双栈对齐
复核（engine 面同族排查）已随本修覆盖其消费面；用户侧待办=Fork 推送(含
955-1444)+正式打包三脚本重烤。下一号自 1445 起。
