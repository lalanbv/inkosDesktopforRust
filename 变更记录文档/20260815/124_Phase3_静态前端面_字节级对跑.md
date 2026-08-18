# 124 号变更记录：静态前端面（/assets/* + SPA 回退）——sidecar 静态服务的 Rust 对应物 + 双端字节级对跑

## 一、背景

123 号补上 bin 装配层盲区后，继续排查切换后前端可达性：TS sidecar `startStudioServer` 除 API 外还**服务前端静态面**（server.ts L6820-6857）：

- `GET /assets/*`：`staticDir + 请求路径` 读文件，扩展名映射 content-type（js/css/svg/png/ico/json，其余 `application/octet-stream`），读失败 404；
- SPA 回退：`GET *` 非 `/api/v1/` 路径 → index.html（**启动时读一次缓存**），`/api/v1/*` 一律 404。

Rust bin 此前**完全没有静态服务**——浏览器直连模式下（studio 跑在 sidecar 端口的单进程架构），API base 切到 Rust 后页面与资产全部 404。切换条件③（前端切换）在浏览器模式下的**阻断级缺口**。

## 二、交付

### 1. `src/server/static_routes.rs`（新模块，TS 语义逐字对齐）

- `with_static_face(router, Option<PathBuf>) -> Router`：合并到既有全量路由；None = 纯 API 模式（Tauri 壳内嵌前端场景；TS 无此开关恒挂 dist，Rust 侧显式化）。
- content-type 映射逐字（含"无扩展名 → 整名不中映射 → octet-stream"同款落点）；index 启动期一次读缓存（TS 同款）；`/api/v1/*` 不吞回退（404）；非 GET 不落回退（TS `app.get("*")` 语义）。
- 安全加固：通配段 percent 解码后含 `..` 直接 404（TS 端 URL 解析归一的等价面）；percent 解码用已有 `percent-encoding` 依赖。
- 8 项 lib 单测：映射表逐项 / 缺失 404 / traversal 拒绝 / SPA 深链 / API 前缀穿透 / 非 GET 404 / None 纯 API / 无 index 仍服务资产。

### 2. bin 接入 `INKOS_STATIC_DIR`

浏览器直连模式指 `packages/studio/dist`；未设为纯 API 服务。bin 头部文档同步。

### 3. duel 第 7 测试：`bin_process_static_face_duel`

真实 bin（CARGO_BIN_EXE + INKOS_STATIC_DIR）与 TS sidecar 对**同一 dist 目录**静态面双端对跑：

- `/` 与深链 `/editor/chapter/2`（SPA 回退）：状态码 + **body 字节级一致** + content-type 忽略大小写一致（两侧框架 charset 大小写差异属表示层）；
- `/assets/app.js`：字节一致 + content-type 精确 `application/javascript`；
- `/assets/missing.js`：双端 404；
- `/api/v1/health`：静态回退不干扰已注册 API 路由。

新增 `get_raw` 辅助（状态码 + content-type + body）——静态面比 JSON 面多一个 content-type 维度。

## 三、验证基线（124 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1165** 过（+8 静态面） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | 182 过 |
| `cargo test --features export-bindings --lib` | **1324** 过（+8） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **7/7** |

## 四、对切换条件的影响

条件③（前端切换）两种形态均闭环：Tauri 壳内嵌前端（纯 API，无需静态面）与浏览器直连（INKOS_STATIC_DIR=packages/studio/dist，与 sidecar 字节级等价——duel 第 7 测试为验收门）。runbook 切换 SOP 补一行：浏览器模式 bin 须带 `INKOS_STATIC_DIR`。

## 五、下一步候选

1. 实切条件到位后真实流量切换（duel 7/7 为前置门）。
2. 按需轮（用户指定功能缺口或缺陷修复）。
