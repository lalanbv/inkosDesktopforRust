# 发版操作手册（RELEASE RUNBOOK）

> 面向执行**Fork 推送**与**正式打包三脚本重烤**两项待办的操作者。
> 关键教训散在各轮变更记录（1479/1482 等），本手册收敛为一页可执行步骤。
> 最近更新：1493 号轮（2026-11-18）。

## 待办一：Fork 推送（955-1493+）

**背景**：本机 CLI 无 GitHub 凭据（gh 未装/无 SSH/PAT，Fork 令牌应用内
加密）——推送只能在 Fork 图形端操作；fetch 匿名可用。

**步骤**：
1. Fork 中确认目标仓库与本地一致：本地 `git log --oneline -1` 的
   HEAD 提交号应与 Fork 待推送的最末提交衔接（955 号起为待推送范围，
   实际上限以推送当日本地 HEAD 为准）。
2. Fork 图形端发起推送（push master）。
3. **推送后核验**：`git fetch origin` 后 `git rev-list --count
   HEAD..origin/master` 应为 0（本地不落后远端）。

## 待办二：正式打包三脚本重烤

**背景**：桌面应用 UI 与引擎二进制都是**打包时快照**（烤入面），
不是实时源码。当前快照已陈旧三层：
- **1486 依赖刷新**（engine-rs 6 包+src-tauri 44 包 patch 级）——
  烤入的引擎二进制还是旧依赖树产物；
- **1487 fmt 全仓清偿**（6223 处，语义保持但二进制需要重建）；
- **1496 npm 面刷新**（hono 4.13.19 生产依赖+5 条 dev）——TS 侧
  server 框架与构建链产物需随重烤更新（desktop-build-inkos.sh
  全量构建自动覆盖）。

**脚本清单与顺序**（仓库根执行）：

| 顺序 | 脚本 | 职责 |
|---|---|---|
| 1 | `scripts/desktop-build-inkos.sh` | mono-repo 整体构建（packages/ 产物） |
| 2 | `scripts/desktop-package-engine.sh` | 组装 Node 回退腿引擎（`src-tauri/engine/`，inkos 自包含运行时） |
| 3 | `scripts/desktop-package-rust-engine.sh` | 组装 Rust 引擎资源目录（`src-tauri/engine-rust/`）——**内嵌 `cargo build --release` 重编**（第 30 行附近） |

**关键纪律：不要跳过 engine-rs release 重编**。1479 号实证：`cargo
build --release --bin` 不会自动刷新 tauri resource 拷贝——手工只拷贝
static 面会带着数月前的旧引擎二进制打包（当时 watchdog 特性静默缺席
8 个月产物）。**完整跑 `desktop-package-rust-engine.sh` 即自带重编+拷贝
一体**，无手工步骤。

**重烤后验证点**：
1. 烤入面快照健康：`src-tauri/engine-rust/static/index.html` 与
   `packages/studio/dist/index.html` 内容一致（重烤后 A=B 恒等，
   巡检期间的预期漂移自愈）；
2. 引擎二进制新鲜：烤入后启动 app，日志无「引擎二进制陈旧于源码」
   告警（1480 号陈旧守卫会自动比对 mtime）；
3. 全门禁抽样：`pnpm fmt:gate && pnpm clippy:gate`（双 crate 应零告警
   零漂移——1487 后新基线）。

## 附：当前安全基线（2026-11-18）

- audit:rust 双锁 **0 漏洞**（1486 刷新后复证）；
- audit:npm **5 条白名单**全定性（dev 工具链已接受风险，含 1489 号
  picomatch GHSA-fc5q-39xc-vjpf id 钉定——上游发布 4.0.8+ 补丁线后
  随 vite bump 出清）；
- npm outdated：6 条 patch/minor 已刷（1496，hono 4.13.19 含内）；
  跨 major ~20 条 978 钉定制维持；@mariozechner/pi 族 2 条 Deprecated
  备案（earendil 出口联动）；
- testgate 2120+599 / gate:ts 十腿 / clippy / fmt:gate / bench 全绿
  （1487-1491 终证链；1496 npm 刷新后 gate:ts 复绿）。
