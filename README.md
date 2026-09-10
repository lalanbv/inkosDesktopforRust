<p align="center">
  <img src="assets/logo.svg" width="120" height="120" alt="InkOS Logo">
  <img src="assets/inkos-text.svg" width="240" height="65" alt="InkOS">
</p>

<h1 align="center">InkOS Desktop<br><sub>基于 Tauri 2 原生壳与 Rust 全量引擎的故事创作 AI Agent 桌面客户端</sub></h1>

<p align="center">
  <a href="https://github.com/lalanbv/inkosDesktopforRust/releases"><img src="https://img.shields.io/badge/version-0.2.0-blue" alt="desktop v0.2.0"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-AGPL%20v3-blue.svg" alt="License: AGPL-3.0"></a>
  <a href="https://github.com/Narcooo/inkos"><img src="https://img.shields.io/badge/上游-Narcooo%2Finkos-8B5CF6?logo=github" alt="上游 inkos 仓库"></a>
  <img src="https://img.shields.io/badge/engine-Rust%20直启%20%2B%20Node%20回退-orange" alt="引擎后端">
</p>

<p align="center">
  <a href="README.en.md">English</a> | 中文 | <a href="README.ja.md">日本語</a>
</p>

---

## 这是什么

[InkOS](https://github.com/Narcooo/inkos) 是一个面向故事创作与多语言翻译的 AI Agent 系统：长篇连载、独立短篇、剧本剧作、互动影游、开放世界和长文翻译，都从同一个工作台开始（产品能力详见上游 README）。

**本仓库是 InkOS 的桌面客户端分支**（fork-and-own mono-repo），在同一个仓库里包含：

| 组成 | 内容 |
| --- | --- |
| `src-tauri/` | **Tauri 2 桌面壳**：窗口 / 托盘保活 / 引擎进程监督 / Keychain 密钥同步 / 原生通知 / 自动更新 / WASM 插件系统 / 多项目管理 |
| `engine-rs/` | **Rust 全量引擎**：`packages/core` 的完整移植（16 业务域、`/api/v1/*` 全量 REST 面 + SSE + 静态面 + CORS），独立 `inkos-engine-server` 可执行文件 |
| `packages/{core,cli,studio}` | 上游 inkos v1.8.0；**桌面 UI 主体在 `packages/studio`**，四期 UI 优化直接在此演进 |
| `scripts/desktop-*` | 本地构建 / 引擎打包 / 发版脚本（本项目无 CI，全部本地脚本化） |

桌壳默认**直接拉起 Rust 引擎（零 Node 运行）**；Rust 二进制缺失或显式配置 `node` 时，自动回退 Node sidecar（`packages/cli/dist` + 自包含 Node bootstrap，首启下载）。

<p align="center">
  <img src="assets/studio-dashboard.png" width="760" alt="InkOS Studio 开始创作入口">
</p>

## 当前状态（2026-08）

| 项 | 状态 |
| --- | --- |
| 桌面壳版本 | v0.2.0（163 号，2026-08-24 完成打包与三重冒烟） |
| 引擎后端 | 默认 Rust 直启（164 号绞杀者终切），Node sidecar 降级为回退路径 |
| 引擎更新通道 | 按运行态后端分流，Rust / Node 双资产通道 + 健康预检（166 号） |
| 桌面 UI | 四期优化收官（150~163 号）：命令面板、标签页、专注模式、对照分屏等，十维复评 2.4 → 4.2 |
| CI | 已全量移除 GitHub Actions（149 号），发版走本地脚本 + Release 手动上传 |

## 桌面端能力

### 引擎双后端

| 后端 | 进程 | 健康探测 | 静态面 |
| --- | --- | --- | --- |
| `rust`（默认） | `inkos-engine-server`（包内资源，零 Node） | `/api/v1/health` | `engine-rust/static/` |
| `node`（回退） | `node engine/dist/index.js studio` | `/` | engine 包内 SPA |

选择序：环境变量 `INKOS_ENGINE_BACKEND`（rust\|node）> 设置面板「引擎后端」下拉（settings.html / TOML）> 默认 `rust`。Rust 二进制 miss 时自动回退 Node 并告警，**启动不阻断**；诊断命令回显运行态生效后端（`engine_backend: rust / node / unknown`），与配置意图区分。

### 系统集成

- **密钥安全**：系统 Keychain（macOS Keychain / Windows Credential Manager / Linux Secret Service）与项目 `.inkos/secrets.json` 启动期双向同步 + 文件监听回写 + 三层防回环 + 0600 权限恢复；keychain 不可用时降级为警告，不阻塞使用
- **后台不打扰**：SSE 旁路订阅引擎事件（`write:complete` / `book:created` 等），窗口失焦时原生通知 + 托盘角标
- **托盘保活**：关窗隐藏入托盘；托盘退出 / SIGINT / SIGTERM / Cmd+Q 均走幂等清理（杀进程组、释放资源）
- **自动更新**：tauri-plugin-updater + Ed25519 签名；应用与引擎均可检测-下载-安装-失败回滚，引擎按生效后端选 Rust / Node 资产
- **loopback 加固**：macOS pf / Linux iptables / Windows netsh 占位实现，非特权启动自动降级并告警
- **可观测性**：tracing 日志、panic hook 崩溃上报、诊断命令（含生效后端、端口、引擎版本）

### 桌面化 Studio UI（150~166 号四期 + 收尾）

- **⌘K 命令面板**（39 命令）与 **⌘P 快速打开**（书 / 章 / 会话 / 设置四类）、**⌘/** 快捷键速查
- **标签页多任务**：VSCode 式预览语义、⌘数字切换、pin、持久化、深链
- **四区工作台**：活动栏 + 侧面板（宽度拖拽记忆 / 树过滤 / ⌘B 折叠）+ 右侧 dock + 底部任务流与日志 + 状态栏（书·章·字数 / SSE / daemon 状态）
- **专注模式**、聊天**消息级打字机**、**章节读写对照分屏**（宽度记忆 + 上/下一章跳转）
- 主题 light / dark / auto 跟随系统 + 舒适 / 紧凑密度档；reduced-motion 与键盘可达性（a11y）随行
- 通知中心与任务停止；macOS 融合标题栏 / 交通灯 / 窗口状态记忆 / 菜单栏入口同源

### 插件系统

WASM 沙箱插件 + 声明式权限 + marketplace（详见 [docs/plugin-system.md](docs/plugin-system.md)）。

## 从源码运行

### 环境要求

| 工具 | 版本 | 备注 |
|------|------|------|
| Rust | stable | `rustup` 安装 |
| Node.js | 22+ | 构建上游 packages 需要 |
| **pnpm** | **10.x（必须）** | pnpm 11 不再读 `pnpm.overrides`，`--frozen-lockfile` 必败；`corepack enable && corepack prepare pnpm@10.34.5 --activate` 锁版本 |
| macOS | Xcode CLT | dev 模式签名 |

### 步骤

```bash
# 1. 构建上游产物（pnpm install + core/cli/studio 全量构建）
./scripts/desktop-build-inkos.sh

# 2. 构建 Rust 引擎（dev 下壳层直接探测 engine-rs/target/）
cd engine-rs && cargo build --release

# 3. 可选：组装 Rust 引擎资源目录（打包 .app 必需；纯 dev 可跳过）
./scripts/desktop-package-rust-engine.sh

# 4. 拉起桌面壳
cd src-tauri && cargo run
```

预期：窗口打开 picker 页（项目选择 / 新建 / 最近项目），选定项目后自动启动引擎并导航到 `http://127.0.0.1:<port>/`。dev 模式下 Rust 引擎按 `app_data/engine-rust` → `resource_dir/engine-rust` → `engine-rs/target/{release,debug}` 序探测；静态面取 `packages/studio/dist`。

## 安装包

从 [GitHub Releases](https://github.com/lalanbv/inkosDesktopforRust/releases) 下载（v0.2.0 起由本地脚本打包、手动上传）：

- macOS：`.dmg`（Apple Silicon / Intel）
- Windows：`.msi` / `.exe`
- Linux：`.deb` / `.AppImage`

系统要求：macOS 11+（Big Sur）/ Windows 10 1809+ / Ubuntu 20.04+、Debian 11+、Fedora 35+ 等主流发行版。

安装后默认使用包内 Rust 引擎，**无需安装 Node.js**；仅在回退 Node 后端时首启下载 Node bootstrap。

## 构建与发布（本地脚本，无 CI）

```bash
./scripts/desktop-build-inkos.sh                            # 1. 前端与 Node 产物
INKOS_ENGINE_PROD=1 ./scripts/desktop-package-engine.sh    # 2. Node 引擎自包含包
./scripts/desktop-package-rust-engine.sh                   # 3. Rust 引擎资源目录
cd src-tauri && pnpm dlx @tauri-apps/cli@2 build           # 4. 打 .app / .msi / .deb 等
# macOS dmg：hdiutil UDZO + shasum -a 256；随后 git tag + Release 页手动上传
```

其他脚本：`package-rust-engine.sh`（独立 Rust 引擎 tarball 发布物，含 sha256 伴生文件）、`desktop-gen-updater-key.sh`（updater Ed25519 keypair，pubkey 写入 tauri.conf.json）。

## 测试

```bash
cd src-tauri && cargo test                 # 单测 + 集成测 + doctest（mock，全绿）
cd src-tauri && cargo test -- --ignored    # 真实 sidecar / keychain 集成测（需真机）
cd engine-rs && cargo test                 # 引擎单测 + golden 差分 + strangler duel
pnpm test && pnpm typecheck                # studio / core / cli 的 vitest 与类型检查
cd src-tauri && cargo llvm-cov --workspace --html --output-dir target/llvm-cov/html   # 覆盖率
```

## 文档

- 📖 [用户指南](docs/USER_GUIDE.md) / 🚀 [快速开始](docs/QUICK_START.md) / 🔧 [故障排查](docs/TROUBLESHOOTING.md)
- ♿ [i18n 与无障碍](docs/i18n-a11y.md) · 🧩 [插件系统](docs/plugin-system.md) · 🔒 [安全审计](docs/security-audit.md) · ✍️ [签名采购](docs/signing-procurement.md) · 📦 [SEA 可行性](docs/sea-feasibility.md)
- 🛠 模块级文档：[src-tauri/README.md](src-tauri/README.md)（桌壳）、[engine-rs/README.md](engine-rs/README.md)（引擎移植目标与纪律）、[src-tauri/tests/README.md](src-tauri/tests/README.md)（测试指引）
- 🗂 [变更记录文档/](变更记录文档/)（逐变更归档，编号连续）与 [开发时SpecCoding'sPlan/](开发时SpecCoding'sPlan/)（设计规划）

> 注意：`docs/` 下部分文档早于 164 号「Rust 引擎默认直启」切换；USER_GUIDE / QUICK_START 已于 172 号、TROUBLESHOOTING 已于 179 号对齐复审。任何疑问以本 README 与最新变更记录为准。

## 开发约定

- **变更记录**：每次修改归档到 `变更记录文档/{YYYYMMDD}/{编号}_标题.md`，编号连续（当前至 179 号）
- **不写 workflow**：GitHub Actions 已全量移除（149 号），此后不新增任何 workflow 及配套脚本，发版走本地脚本
- **与上游的关系**：早期坚持「零修改上游文件」以降低合并冲突；150 号 UI 四期起，`packages/studio` 由本分支直接演进（fork-and-own），不再回合并上游
- 桌壳模块保持单一职责（src-tauri 各模块 < 500 行约束）；引擎移植遵循 1:1 复刻 + golden 差分 + 契约 duel 纪律（详见 engine-rs/README.md）

## 致谢与许可证

- 上游项目：[InkOS](https://github.com/Narcooo/inkos)（Narcooo）及其贡献者；InkOS 的 agent 运行时构建在 [pi](https://github.com/badlogic/pi-mono)（`@mariozechner/pi-ai` / `@mariozechner/pi-agent-core`）之上
- 赞助上游：感谢 [字节火山引擎](https://www.volcengine.com/activity/ai618?utm_source=OWO&utm_medium=devrel-1&utm_campaign=hw&utm_term=inkos&utm_content=hw) 赞助 InkOS（火山方舟 Agent/Coding Plan 国模套餐，支持 GLM-5.3、Kimi-K3、DeepSeek 等模型）
- 许可证：[AGPL-3.0](LICENSE)，与上游一致
