# 396 号：daemon 生命周期状态机生产实测 + 395 号收尾态交叉评审

日期：2026-09-14
类型：真实用户路径验证（零代码批）+ 并行会话收尾态交叉评审

## 一、395 号收尾态交叉评审（272 号边界规程）

395 号（commit cad20f43）落库的并行会话三件，逐件复核：

1. **scripts/audit-rust.mjs（新，97 行）**——代码形态合格：缺 cargo-audit
   以码 3 退出不静默绿灯；漏洞非零退出；`--strict` 才计 warning；yanked
   联网核验超时折叠为提示（网络抖动非漏洞）；经 `sh -c` 丢 stderr 防
   yanked 刷屏。**真跑验证**：`npm run audit:rust` → engine-rs 0 漏洞、
   src-tauri 0 漏洞，接线可用。
2. **package.json 死入口移除声称**——验证属实：三包
   （core/studio/cli）`grep -c '"lint"'` 全 0（lint 入口确实无目标）；
   `scripts/studio-e2e-benchmark.mjs` 确实不存在（2026-06 上游已删）。
3. **rustbin.rs `dev_repo_root`→`_dev_repo_root`**——改名与调用点一致：
   main.rs L704/L714 传入局部变量 `dev_repo_root`，签名收
   `_dev_repo_root`；`#[cfg(debug_assertions)]` 块内仍正常使用，release
   档警告消除成立。

结论：三件收尾态全部合格，无遗留。

## 二、daemon 生命周期状态机生产实测

### 背景

daemon 面共三条端点（ops_routes.rs L473-575，TS 真值 server.ts
L5191-5244）：GET /api/v1/daemon、POST /daemon/start、POST /daemon/stop。
327 号测的是 notify/webhook 事件链（daemon:chapter→webhook），生命周期
状态迁移本身此前无生产实测。**无 pause/resume 端点**——daemon 只有
start/stop 两态（双端一致）。

### 生产环境

release bin `engine-rs/target/release/inkos-engine-server` + 
walkthrough-mock（端口 1241，位置参数非 `--port`）+ 引擎 1242 +
`INKOS_BUILTIN_GENRES_DIR` 绝对路径 + 空工作区（无书无 daemon 配置段——
DaemonConfig::from_raw 有完整默认值：writeCron `*/15 * * * *`、radarCron
`0 */6 * * *`）。

### 状态机九步矩阵（全过）

| 步 | 操作 | 结果 | TS 契约锚 |
|---|------|------|-----------|
| 1 | GET（初始） | `{"running":false}` | L5191-5195 |
| 2 | POST stop（未运行） | 400 `Daemon not running` | L5237-5239 |
| 3 | POST start | 200 `{"ok":true,"running":true}` | L5230 |
| 4 | GET（启动后） | `{"running":true}` | 同 1 |
| 5 | POST start（重复） | 400 `Daemon already running` | L5198-5200 |
| 6 | 写循环首轮空转（无书，3s 观察） | 引擎日志无 panic/error | run_write_cycle L193 |
| 7 | POST stop | 200 `{"ok":true,"running":false}` | L5243 |
| 8 | GET（停止后） | `{"running":false}` | 同 1 |
| 9 | POST stop（重复） | 400 `Daemon not running` | 状态机闭环 |

### SSE 广播实测

`GET /api/v1/events?sessionId=…` 后台监听，九步全程抓到且按序：
`daemon:started`（start 时，对齐 TS L5220）→ `daemon:stopped`（stop 时，
对齐 TS L5242）。真实用户 UI toast 依赖的两广播双端一致。

### 观察非偏差

TS start 后 `scheduler.start()` 异步失败有「stop + daemon:stopped +
daemon:error(bookId:"scheduler")」恢复路径（L5221-5229）；Rust 无等价
路径——Rust 侧写/radar 任务经 `tokio::spawn` 启动无异步失败点，任务期
错误各自走 daemon:error 广播（radar 用 bookId:"radar" L529-533，写循环
在 run_write_cycle 内部按书广播）。失败面不等价但失败面不可达，判观察
非偏差，备案。

### 教训

walkthrough-mock 端口是**位置参数**（`node scripts/walkthrough-mock.mjs
1241`），`--port 1241` 会被 `Number()` 吃成 NaN 启动失败（本次实测踩到，
mock.log RangeError 可诊断）。

## 三、门禁

零代码批：无新增测试、无需 INKOS_DUEL 全测。生产实测九步 + SSE + 真跑
audit:rust 即本批验证证据。
