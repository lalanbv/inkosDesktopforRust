# 268 号：自主持续开发第 5 轮——rust audit 脚本化 + 冷启动基线 + release 启动 e2e 破冰

- 日期：2026-09-10
- 分支：develop
- 关联：263/264 号（wasmtime 升级——本批 audit 脚本化的直接动因）、266 号（wasm fixture）、213 号（性能压测先例）
- 编号衔接：原编 267，与并行会话 6b3341c6（HEAD 全栈端点冒烟扫）撞号，让位改 268
- 推送核验：HEAD 44750168；本批待提交（audit-rust.mjs、package.json、rustbin.rs、wasm 执行测试 fixture 双路径 + 266/267 号记录）

## 一、批A：Rust 依赖漏洞审计脚本化

`scripts/audit-rust.mjs`（新）+ `package.json` `audit:rust`：

- engine-rs 与 src-tauri 两把 Cargo.lock 全扫（RustSec advisory-db）；任一 vulnerabilities → 摘要输出 + 非零退出；`--strict` 使 unmaintained/yanked 警告也计失败；可传目录参数只扫指定 crate。
- cargo-audit 未安装 → 明确报错退出（码 3），审计缺失不静默绿灯。
- **工程细节**：cargo-audit 的 yanked 联网核验错误会**直写 fd2 穿透 execFileSync 管道**（实测 336 行刷屏），经 `sh -c "cargo audit 2>/dev/null"` 在子进程内部丢弃——advisory 结论全在 stdout，stderr 无可保留信息。
- 验证：两 crate 全绿（0 漏洞）、网络抖动场景收敛为单行提示、非法场景退出码正确。

## 二、批B：引擎冷启动基线（213 号未覆盖面）

`gen-perf-fixture.py` 生成 60 章（3.1MB）/ 500 章（26MB）fixture，实测 production bin 冷启动（3 次取稳态）：

| 场景 | health 就绪 | 首个 /books 查询 |
|---|---|---|
| 60 章 | 25–76ms（首启 76ms 为磁盘冷读） | 27–39ms |
| 500 章 / 26MB | **25ms** | **31–32ms** |

**结论：冷启动无优化必要**——桌面壳"进程起→可用"体验在 500 章规模下仍 <100ms（engine bin 直启形态）。作为基线记录在案，后续版本可对比回归。

## 三、批C：release 启动 e2e 破冰 + release 专属警告修复

1. `cargo build --release` 首次本地全量构建成功，暴露 **2 个 release 专属警告**：`rustbin.rs` 的 `dev_repo_root` 仅用于 `#[cfg(debug_assertions)]` 块，release 下未使用——改下划线前缀参数名（debug 块内引用合法，调用方无感）。
2. `e2e_startup` 两个 `#[ignore]` 用例（需 release binary）本地破冰运行：`test_startup_smoke` / `test_startup_logs_version` **2/2 通过**——真实桌面二进制启动、日志初始化、版本记录全链验证。

## 四、验证汇总

| 门禁 | 结果 |
|---|---|
| src-tauri cargo test（debug 全量） | ✓ 576 通过 0 失败 |
| src-tauri clippy --all-targets | ✓ 0 |
| src-tauri release build | ✓ 0 警告 |
| e2e_startup（release，--ignored） | ✓ 2/2 |
| pnpm audit:rust（两 crate） | ✓ 0 漏洞 |

## 五、遗留

1. e2e_secrets（keychain 授权框阻塞）与 e2e_updater（需网络）维持 ignore——ignore 理由成立，非债。
2. 历史瘦身、推送、两项默认值、ja A/B——待用户决策（承接 264/265 号）。
