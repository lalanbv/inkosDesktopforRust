# inkos-desktop（Tauri 桌面壳）

inkos 的 Tauri / Rust 桌面客户端骨架。本目录是 M1 里程碑的产物，覆盖：
开窗 → 拉起 sidecar → 健康探测 → WebView 加载 SPA → loopback 加固占位。

## 模块速览

| 模块 | 职责 |
|------|------|
| `src/config.rs` | 跨任务共享常量（默认 Studio 端口、健康探针超时/间隔、CLI 入口相对路径） |
| `src/paths.rs` | 跨平台目录解析（`PathResolver` trait + `AppPaths` 实现） |
| `src/supervisor.rs` | sidecar 监督：`pick_free_port` + `build_launch` + `spawn`（进程组）+ `kill_tree` + `health_probe` |
| `src/lifecycle.rs` | `SidecarState`（Tauri managed state）+ `cleanup_sidecar`（退出清理） |
| `src/isolation/` | loopback 加固：`LoopbackGuard` trait + macOS pf / Linux iptables / Windows netsh 占位实现 |
| `src/main.rs` | Tauri 二进制入口：setup 拉起 sidecar、Exit 清理、loopback guard 接入 |
| `src/lib.rs` | 库入口，导出公开模块（供集成测与 doctest 使用） |

所有模块均 < 400 行（单一职责约束）。

## 运行（M1）

> 仓库是 **mono-repo**：`src-tauri/` 是桌面壳，仓库根是 inkos 上游（`packages/`、根 `package.json` 等）。下文命令默认从仓库根执行。

### 环境要求

| 工具 | 版本 | 备注 |
|------|------|------|
| Rust | stable | `rustup` 安装 |
| Node.js | 22+ | inkos CLI 入口 `packages/cli/dist/index.js` 需要 |
| **pnpm** | **10.x**（必须） | ⚠️ **pnpm 11 不兼容**：不再读 `package.json` 的 `pnpm.overrides`，`--frozen-lockfile` 必败。锁版本：`corepack enable && corepack prepare pnpm@10.34.5 --activate` |
| macOS | Xcode CLT | dev 模式签名（M1 不做公证，留 M3） |

### 步骤

```bash
# 1. 安装并构建 inkos（产物落到 packages/cli/dist/ 与 packages/studio/dist/）
./scripts/desktop-build-inkos.sh

# 2. 拉起桌面壳
cd src-tauri && cargo run
```

预期：窗口打开显示「启动中…」stub 页，几秒内自动跳转到 `http://127.0.0.1:<port>/`（默认从 `:4567` 起探测首个空闲端口）。关闭窗口即清理 sidecar 进程组（不留孤儿占端口）。

### 何处会降级

- **loopback 加固**：macOS `pfctl` / Linux `iptables` / Windows `netsh advfirewall` 均需特权。非特权启动时 guard.lock 失败，stderr 输出 `loopback guard: lock port=<p> 失败 ... 降级继续`，sidecar 仍绑 `0.0.0.0`（同网段可访问）。完整运行时强制待 M2（macOS SMJob/launchd）/ M3（Linux setuid、Windows WFP）。
- **端口选择**：`pick_free_port` 探测瞬间空闲，不保证 spawn 时仍空闲（TOCTOU）。若被抢，`health_probe` 30s 内拿不到 2xx，窗口 title 显示「inkos 启动超时 (30s)，见日志」。

## 测试

```bash
cd src-tauri && cargo test                  # 单测 + 集成测（mock）+ doctest，全绿
cd src-tauri && cargo test -- --ignored     # 包含真实 inkos sidecar 集成测（需先跑构建脚本）
```

**稳定性**：单测/集成测/doctest 共 38 个（34 lib + 2 integration mock + 2 doctest），连续 5 次全量跑全绿，无 flaky。

## 覆盖率

```bash
cd src-tauri && cargo llvm-cov --workspace --html --output-dir target/llvm-cov/html
open target/llvm-cov/html/index.html
```

工具：`cargo-llvm-cov`（macOS 原生支持，比 tarpaulin 更稳）。

| 范围 | 行覆盖率 |
|------|---------|
| **纯函数核心**（config + paths + supervisor + lifecycle） | **93.27%** |
| 全量（含 main.rs 二进制入口 + isolation 三平台 lock/release I/O 路径） | 53.12% |

差距说明：`main.rs` 需 Tauri 运行时无法单测（由 `real_inkos_sidecar_serves_spa` 集成测端到端覆盖）；`isolation/*/lock|release` 需 root/管理员权限，CI 与本机均不可达。规则生成的纯函数 100% 覆盖。详见 `变更记录文档/20260806/M1冒烟验证.md` §7。

## 零修改纪律

仓库根是 inkos 上游内容（`packages/`、根 `package.json`、根 `README*`、根 `.gitignore`、既有 `.github/`）。**桌面壳的所有产物只进 `src-tauri/`、`scripts/desktop-*`、新增 `.github/workflows/desktop-*.yml`**，永不修改 inkos 根文件——这样 `git merge upstream/master` 近零冲突。Rust 构建忽略用 `src-tauri/.gitignore`（不追加到根 `.gitignore`）。

## 协议

AGPL-3.0，与 inkos 主仓一致。
