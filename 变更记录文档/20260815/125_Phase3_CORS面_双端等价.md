# 125 号变更记录：CORS 面等价——Hono `cors()` 默认参的 Rust 对应物 + 双端真进程验收

## 一、背景

124 号静态面收官后继续排查前端可达性面：TS sidecar 全局挂 `app.use("/*", cors())`（server.ts L2880，Hono 默认参）——**跨源前端**（Tauri 壳 `tauri://localhost`、vite dev server :5173 经 API base 直连）依赖此面通过 preflight。Rust bin 此前无任何 CORS 层——切换条件③在 Tauri/开发模式下的**阻断级缺口**（浏览器直连同源模式不受影响）。

## 二、交付

### 1. `server::sidecar_cors_layer()`（mod.rs）

Hono `cors()` 默认参响应面逐字等价：

| 维度 | Hono 默认参 | Rust 实现 |
|---|---|---|
| Allow-Origin | `*`（**恒设**，不看请求是否带 Origin） | `Any`（tower-http 同款恒设） |
| 方法族 | GET, HEAD, PUT, POST, DELETE, PATCH | 同列表 |
| Allow-Headers | 反射请求的 `Access-Control-Request-Headers`（allowHeaders=[]） | `AllowHeaders::mirror_request()` |
| credentials/expose/max-age | 均无 | 均不设 |
| Preflight | OPTIONS 短路 204 | tower-http 短路 200（均浏览器合法 2xx） |

实施要点：tower-http 既有依赖（`cors` feature 已开），零新依赖；层施加在 bin **最终组合路由**（含静态面）之上——`Router::layer` 包裹整个路由服务，404/405/静态回退面均带头，对齐 Hono `app.use("/*")` 中间件语义。壳层复用（Tauri 内嵌同源场景）可按需省略。

### 2. duel 扩面（静态面测试并入 CORS 块）

真实 bin vs TS sidecar 双端：普通跨源请求 `Access-Control-Allow-Origin: *` 等价；preflight（OPTIONS + 请求方法/头）双端 2xx、方法族含 POST、头镜像含 content-type。

### 3. lib 单测

`cors_layer_adds_headers_only_for_origin_requests`：恒设 `*` + preflight 专属头不漏到普通请求 + preflight 短路与方法族/头镜像断言。（过程中纠正一处自误设：tower-http `Any` 恒设头——与 Hono 默认参一致，非"仅带 Origin 才设"。）

## 三、验证基线（125 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1166** 过（+1 CORS） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | 182 过 |
| `cargo test --features export-bindings --lib` | **1325** 过（+1） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | 7/7（静态面测试并入 CORS 断言） |

## 四、对切换条件的影响

条件③（前端切换）三种形态全闭环：Tauri 壳内嵌（同源直连）、浏览器直连（124 号静态面）、跨源开发/壳模式（本号 CORS 面）。切换 SOP 无新增必填项（CORS 为 bin 内建，无需 env）。

## 五、下一步候选

1. SSE 事件流面勘测（前端实时更新的最后一块传输面——两侧事件名/负载形态对照）。
2. 实切条件到位后真实流量切换。
3. 按需轮（用户指定）。
