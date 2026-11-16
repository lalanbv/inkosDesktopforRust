# 1456 号：SSE 缓存头覆盖缺口补洞与 Hono 头回写机制三层定案轮

## 目标与范围

/goal 全量测试新增代码逻辑+代码审查+模拟真人测试。验证对象=1455 改动树
（TS 腿 no-store 统一层：API 统一中间件+SPA 入口 no-store+重建 Response
形态）。本会话与 1455 并行会话撞号（1455 于 01:27 先提交，按 534/536/541
让号先例改号 1456），本会话的独立增量=活体探针暴露 1455 验证缺口并补洞。

## 核心发现：1455 四案例矩阵漏 SSE 形态（真缺陷）

1455 的活体验收矩阵（API/SPA×2/assets 四面）与最小 repro 四案例矩阵
（Origin 有无 × c.json/裸 Response）**均不含 streamSSE 形态**——而
`/api/v1/events` 是仓内唯一 streamSSE 端点，且 hono `streamSSE` helper
**自设 `Cache-Control: no-cache`**（hono@4.13.18 sse.js:51）。

本会话活体探针（dist server 8793/8794 两代形态）实测：API/404/SPA/assets
四面与 1455 一致全绿，**唯 SSE 返回 `no-cache`——1455 重建 Response 形态
盖不住 SSE**。双栈漂移坐实：Rust 腿 SSE 活体（8791 纯 API 形态）为
`no-store`。

## 根因：三层框架机制叠加定案（较 1455 定案更深一层）

1455 定案的机制（cors before-next 竞态）不完整。真链路三层：

1. **Hono `set res` 回写（dispatch 层本体，1455 未识）**：context.js
   `set res(_res)` 在 `this.#res && _res` 时**遍历旧响应头逐键 `set` 覆盖
   新响应**（content-type 豁免）。中间件 `c.res = new Response(...)` 换对象
   身份后，**旧响应上已存在的 `Cache-Control: no-cache` 恰把新头的
   no-store 盖回**。dispatch 层即丢（`app.request` 实测 cc=no-cache），
   非 node-server 独有。1455 四案例全绿纯因 c.json/裸 Response 端点
   **不自设 Cache-Control**（回写链无该键可写）。
2. **@hono/node-server `Response2` 快照重建**：`body` getter 调
   `getResponseCache()`——`delete this[cacheKey]`（丢弃头缓存三元组）+
   按 `#init` **原始头快照**重建 GlobalResponse。中间件读 `res.body` 后，
   旧响应的 headers getter 从此走 #init 快照（no-cache 原值永生）。
3. **触发条件**：端点自设 `Cache-Control≠no-store`（仓内唯一=streamSSE
   的 no-cache）——自设 no-store 的 project/files 单点被早退分支保护。

## 修复：顺序敏感三步（最小面，中间件内 +2 行）

```ts
const body = res.body;                    // ① 先固化 body（此后旧响应头
res.headers.delete("Cache-Control");      //    操作落在重建后的实例上）
const headers = new Headers(res.headers); // ② 摘键——让 set res 回写链
headers.set("Cache-Control", "no-store"); //    无旧值可写
c.res = new Response(body, { ... });      // ③ 再重建
```

两层有效性论证：dispatch 层（原生 Response headers 可变，delete 生效于旧
实例，回写遍历同一实例→无该键）；node-server 层（body 固化触发
getResponseCache 后，delete 落在固化实例 responseCache.headers 上，回写
遍历走同一实例→无该键）。普通端点（无自设 CC）行为与 1455 形态等价
（delete 无键=空操作）；自设 no-store 端点走早退零开销。

## 验收矩阵

- **必红验证**（1007/1436 惯用形）：临时回退摘键步→SSE 锁测恰红
  （`overrides streamSSE's self-set no-cache` 1 failed）+其余 3 测绿
  （不涉改面）→恢复修复复绿 4/4。锁测有效性证明。
- **单测**：api-no-store.test.ts 4/4（新增第 4 测锁 SSE 形态 dispatch 层
  语义；node-server Response2 叠加面由活体覆盖——1455「单测绿≠活体绿」
  教训的双层分工备案）。
- **活体探针（模拟真人测试，生产形态 dist server）**：六面全绿——
  api200 no-store / api404 no-store / **SSE no-store（no-cache→no-store，
  修复核心；content-type: text/event-stream 与 chunked 流形态完好）** /
  SPA `/` no-store / SPA 客户端路由 no-store / assets 无头（缓存资格保留）。
- **双栈对照活体**：Rust 腿带 INKOS_STATIC_DIR（8792，烤入形态二进制
  no-store 面新鲜性核验：mod.rs/static_routes.rs mtime=Nov 11 早于二进制
  Nov 14）六面与 TS 完全同向——1455 未覆盖的 Rust SPA/assets 对照面补全，
  445/469 双栈矩阵收口。
- **gate:ts 十腿**：全绿 exit 0（build 29.6s/typecheck 19.7s/test 135.8s
  含新测/audit:npm 2.7s/rust-bin 69.7s 新鲜度同源/node-fallback-smoke
  35.9s/engine-contract-diff 37.2s/export-epub-smoke 34.9s/work-
  alternation 5.1s/check-routes 0.1s）。
- **U+FFFD**：两改动文件零命中。**清场**：探针进程真残留终检 0+探针端口
  0 占用+tmp root/log 零残留（pgrep 宽匹配 26 命中=系统进程数字串伪影，
  ps 快照定案后精准 PID 清场——903 纪律）。

## 并行会话备注

- 1455（0798ff7a）与本会话并行撞号：其提交卷入本会话在途的测试注释修正
  （Rust 差异定性方向：实测 Rust axum layer 盖 router fallback 404=更宽
  更严，原「Hono 严于 Rust」定性反了——内容自洽入库=增益非冲突，854
  先例形态勘误）。
- 工作树另现 `engine-rs/Cargo.lock`+`src-tauri/Cargo.lock` 在途改动（第三
  并行会话），本号提交严格路径限定三文件避开。

## 教训沉淀

1. **验证矩阵按「端点形态」分轴非按「Origin/响应族」**：helper 自设头的
   端点（streamSSE no-cache）是覆盖语义的独立轴——四案例矩阵漏轴即漏洞。
2. **框架 setter 回写是 mutate 语义的隐形对手**：「换对象身份」逃不过
   「新对象被旧值逐键覆盖」——修复面在旧值摘除不在新值构造。
3. 活体腿是 header 面唯一可靠验收（1455 教训的再实证+扩展：同会话单测
   与活体分层分工须显式备案哪层锁什么）。

## 门禁谱系与出口

HEAD=0798ff7a（1455）+本轮三文件；下一号自 1457 起。出口=下一巡检周期/
earendil-mz 单包合并。用户侧待办=Fork 推送（含 955-1456）+正式打包三脚本
重烤+bench 静默补跑（可选）+依赖 semver 兼容刷新窗口（7+25 包备案项）。
