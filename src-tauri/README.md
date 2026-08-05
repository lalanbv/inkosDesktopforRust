# inkos-desktop（Tauri 桌面壳）

inkos 的 Tauri / Rust 桌面客户端骨架。本目录是 M1 里程碑的第一个产物，仅包含：

- `Cargo.toml` —— 桌面壳 crate 清单（`inkos-desktop`，edition 2021）
- `src/lib.rs` —— 库入口，导出 `config` 模块
- `src/config.rs` —— 跨任务共享的常量（默认 Studio 端口、健康探针超时/间隔、CLI 入口相对路径）
- `src/main.rs` —— 二进制入口占位，后续任务将在此启动 Tauri 运行时与 sidecar 监督器

## 零修改纪律

本目录是桌面壳专属产物。仓库根目录是 inkos 上游内容（`packages/`、根 `package.json`、根 `README*`、根 `.gitignore`、既有 `.github/`）。**桌面壳的所有产物只进 `src-tauri/`、`scripts/desktop-*`、新增 `.github/workflows/desktop-*.yml`**，永不修改 inkos 根文件——这样 `git merge upstream/master` 近零冲突。

## 测试

```bash
cd src-tauri
cargo test config -- --nocapture
```

预期：2 个单元测试通过（`default_port_matches_inkos`、`cli_entry_is_studio_dist`）。

## 协议

AGPL-3.0，与 inkos 主仓一致。
