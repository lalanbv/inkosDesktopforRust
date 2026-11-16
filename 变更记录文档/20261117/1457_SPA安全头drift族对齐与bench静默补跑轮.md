# 1457 号：SPA/assets 安全头 drift 族对齐与 bench 静默补跑轮

## 目标与范围

/goal 自主持续开发续轮。双增量：①实施 1455 备案 drift 族——TS server
静态面安全头对齐 Rust（spa_fallback/serve_asset 均带 nosniff/
no-referrer/DENY 三头，TS 双面缺失）；②bench 静默窗口补跑（1448 定性
制会话内逐轮重试 + 1456 依赖刷新后基准重验价值）。

过程纪要：开工时并行会话 1456（SSE 缓存头补洞轮）在途未提交
（server.ts+测试+文档工作树三文件）——本轮原 scope 同文件，按
「防并行暂存物被卷入」纪律**等待其落库**（57a24fa9）后才动 server.ts；
期间先跑 bench 首轮被卫兵拒（1min 4.63/18 核=26%>20%，257/1448 纪律
不强跑）。

## 结果

①**静态面安全头双面对齐**（server.ts 静态面，与 API 中间件区不重叠）：

- `/assets/*` handler：裸 `new Response` 头对象补三安全头（裸 Response
  面不进 preparedHeaders——1455 定案，直接构造头）；**无 no-store**
  （内容寻址可长缓存，对齐 Rust serve_asset 四头语义）
- SPA fallback：before-response `c.header` ×3（1455 活体定案可靠形），
  与已有 no-store 同挂载点（对齐 Rust spa_fallback 五头语义）

②**活体探针四面全绿**（真起 server curl——hono-header-after-next-race
记忆纪律：header 面单测不算数）：

- `/` 与深路径：no-store + nosniff + no-referrer + DENY 四头 ✓
- `/assets/*.css`：三安全头、**无 no-store** ✓
- `/api/v1/profiles`：no-store、无三安全头 ✓——分层策略与 Rust 精确
  一致（api_no_store 只管缓存头，安全头只在静态面）

③**bench 静默窗口补跑通过**（1456 依赖刷新后基准重验）：卫兵两轮裁
定——首轮拒（26%>20% 静默等待 180s）→次轮放行（3.08/18=17%）→28 项
基准门禁通过 exit 0。涨跌散布形态（最大 +12.5% paragraph_scan /
-17.2% projections_summaries，方向不一）=criterion 采样噪声+依赖微扰
（jiff/nix/rand patch）混合，门禁裁定可接受，不过度归因不洗基线。

④**验收链**：typecheck 双配置零错 + gate:ts 十腿全绿 exit 0（build
19.1s/typecheck 19.2s/test 109.0s/audit:npm 2.2s/rust-bin 0.2s/
node-fallback-smoke 35.4s/engine-contract-diff 36.0s/export-epub-smoke
35.0s/work-alternation 5.1s/check-routes 0.1s）——改动树全谱零回归。

⑤1455 备案 drift 族清偿：TS 静态面安全头缺口消除；Rust 侧对应测试锁
（static_routes.rs:380-391 三头断言+:411-427 no-store）在位为基准。
1456（并行 SSE 补洞）+1458（并行浏览器级真人走查）与本轮静态面改动
互补构成 no-store/安全头完整谱系。

零新缺陷，探针用完即删零残留，U+FFFD 净。

出口=下一巡检周期/earendil-mz 单包合并；用户侧待办=Fork 推送(含
955-1458)+正式打包三脚本重烤。下一号自 1459 起。
