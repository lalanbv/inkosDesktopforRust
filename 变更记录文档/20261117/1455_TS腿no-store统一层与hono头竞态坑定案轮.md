# 1455 号：TS 腿 no-store 统一层实施与 hono header 竞态坑定案轮

## 目标与范围

/goal 自主持续开发续轮。实施 1451 备案工程项（差异②）：TS server 无统一
no-store（仅 :5998 project/files 单点），Node 回退形态（壳 fallback 指
TS server 浏览器直连）下 445 同族陈旧缓存问题理论存在。本轮三面对齐
Rust 基准：445 API 面（`api_no_store` 中间件盖 /api/v1）/ 469 SPA 入口
面（index.html no-store）/ assets 对照面（内容寻址可长缓存不盖）。

## 结果

①**API 统一层**（server.ts，cors 之后挂 `app.use("/api/v1/*")`）：
覆盖语义与 Rust `api_no_store` 对齐（端点自设同值被覆盖无害），仅盖
`/api/v1/*`——assets/SPA 不经此层。

②**SPA 入口 no-store**（startStudioServer fallback handler `c.header`
before-response）：对齐 Rust 469 号 `spa_fallback`（index.html 禁缓存
——陈旧入口会让整站跑旧 bundle）。

③**hono 框架坑定案（本轮核心发现）**：初版用 after-next
`c.res.headers.set(...)`（hono 官方 secure-headers 族惯用形），单测
`app.request` 3/3 绿，**但真 HTTP 活体探针 API 面丢头**。最小 repro 四
案例矩阵（Origin 有无 × c.json/裸 `new Response`）二分定案：

- `same-origin` 两形态 ✓ 带出；`no-origin` + c.json ✗ 丢；
  `no-origin` + 裸 `new Response` ✗ 丢
- 机制：hono `cors` 中间件 before-next `set` 触发 `c.res` getter 副作用
  创建占位 Response，handler 返回后 `set res` 拷贝竞态——after-next 对
  原 Response 对象的 mutate 不保证反映到最终 serve 的 Response
- 修复=**重建 Response**（`new Headers` 全量拷 + `new Response(res.body,
  {status, statusText, headers})` 换对象身份）：四案例全绿，且对裸
  Response 端点与 c.json 族全形态可靠

这是 1451 记忆「活体腿须显式起目标栈真服务」的又一实证：**header 面单
测（app.request）绿 ≠ node-server 活体绿**——若只跑单测本轮即假绿
（1429 同族：4569 掩盖 TS 腿故障 8 个月先例）。

④**验收矩阵**：单测 3/3（happy-path 带头 / 未注册 API 路径 404 带头——
Hono use 盖 notFound 严于 Rust axum layer 只盖注册路由的无害差 / 非 API
路径带头对照）+ typecheck 双配置零错 + 活体探针四面全绿（真 server：
`/api/v1/profiles` no-store、`/` 与深路径 SPA no-store、`/assets/*.css`
无 cache-control 对照）+ **gate:ts 十腿全绿 exit 0**（build 20.7s /
typecheck 20.7s / test 111.6s 含新测 / audit:npm 2.8s / rust-bin 0.2s /
node-fallback-smoke 35.4s / engine-contract-diff 36.0s / export-epub-smoke
35.0s / work-alternation 5.1s / check-routes 0.1s）——差分器不比对
header 面零影响，改动树全谱零回归。

⑤**1451 差异②备案转清偿**：TS 腿统一 no-store 落地，Node 回退形态下
445 同族陈旧缓存面消除；1451 三防线面矩阵中数据面（no-store）从
「Rust 活体绿/TS 备案」升格双栈落地。SPA fallback 安全头差异
（nosniff/referrer/frame-options Rust 有 TS 无）=另一 drift 族备案
（非本轮 no-store scope，WKWebView 壳内低风险）。

探针 root×2+repro 用完即删零残留；Mimosa 钩子对 probe 脚本 curl `$1`
保守注入误报备案（1442/1449 同族）；probe 夹具 version 必须 "0.1.0"
（loadProjectConfig zod literal，2.0.0 拒启=1429 schema strict 同族）。
零新缺陷，U+FFFD 净。

出口=下一巡检周期/earendil-mz 单包合并；用户侧待办=Fork 推送(含
955-1455)+正式打包三脚本重烤+bench 静默补跑（可选）+依赖 semver 兼容
刷新窗口（7+25 包备案项）。下一号自 1456 起。
