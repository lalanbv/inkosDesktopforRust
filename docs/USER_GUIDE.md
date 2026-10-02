# InkOS Desktop 用户指南

> **版本**: v0.2.0　|　**更新日期**: 2026-09-03（172 号对齐 164 号「默认 Rust 引擎直启」后的现状，叙事基准为根 README）

InkOS Desktop 是 InkOS 的桌面客户端分支：Tauri 2 原生壳 + Rust 全量引擎（Node sidecar 回退），提供项目管理、密钥安全、自动更新与桌面化 Studio UI。

## 目录

- [系统要求](#系统要求)
- [安装](#安装)
- [快速开始](#快速开始)
- [引擎双后端](#引擎双后端)
- [项目管理](#项目管理)
- [模型服务与技能](#模型服务与技能)
- [上下文计量](#上下文计量)
- [自动更新](#自动更新)
- [桌面工作台](#桌面工作台)
- [插件管理](#插件管理)
- [故障排查](#故障排查)
- [高级配置](#高级配置)

---

## 系统要求

### macOS
- macOS 11.0 (Big Sur)+，Apple Silicon 或 Intel x64
- 首次使用 Keychain 存储密钥时会弹窗请求授权

### Windows
- Windows 10 (1809+) / Windows 11，x86_64
- 自动检查并提示安装 WebView2

### Linux
- Ubuntu 20.04+ / Debian 11+ / Fedora 35+ 等主流发行版
- 依赖 WebKit2GTK 4.1、libssl、libgtk-3

安装即用包内 Rust 引擎，**无需安装 Node.js**；磁盘占用以引擎 + WebView 运行时为主（预留 500 MB 级余量即可）。

---

## 安装

从 [GitHub Releases](https://github.com/lalanbv/inkosDesktopforRust/releases) 下载（v0.2.0 起本地脚本打包、手动上传）：

- **macOS**：`.dmg`，双击打开拖入 Applications；首次启动右键「打开」绕过未签名警告
- **Windows**：`.msi` / `.exe`；SmartScreen 提示未知发布者时选「仍要运行」
- **Linux**：`.deb`（`sudo dpkg -i` 后 `sudo apt-get install -f` 补依赖）或 `.AppImage`（`chmod +x` 后运行）

> 本项目当前无 Homebrew cask / 包管理器发行渠道；从源码构建与打包流程见根 README「从源码运行」「构建与发布」。

---

## 快速开始

1. **选择或创建项目**：启动后进入 picker 页——打开已有项目目录（含 `inkos.json`）、新建项目或从最近项目进入
2. **引擎自启**：选定项目后桌壳拉起引擎（默认 Rust 直启）并导航到 Studio 工作台
3. **开始创作**：配置模型服务与 API Key → 新建书籍 / 在 Chat 中描述你的书 → 「写下一章」

入门流程（建书 → 写章 → 导出）细节见 [QUICK_START.md](./QUICK_START.md)。

---

## 引擎双后端

| 后端 | 进程 | 健康探测 | 说明 |
| --- | --- | --- | --- |
| `rust`（默认） | `inkos-engine-server`（包内资源，零 Node） | `/api/v1/health` | 绞杀者终切后的默认链路（164 号） |
| `node`（回退） | `packages/cli/dist` + 自包含 Node bootstrap | `/` | 显式配置或 Rust 二进制缺失时启用，首启需下载 |

- **选择序**：环境变量 `INKOS_ENGINE_BACKEND`（rust|node）> 设置面板「引擎后端」> 默认 `rust`
- **自动回退**：Rust 二进制 miss 时自动回退 Node 并告警，启动不阻断
- **诊断**：诊断命令回显运行态生效后端（`engine_backend: rust / node / unknown`），与配置意图区分
- **优雅停机**：Rust 引擎收到 SIGTERM / Ctrl-C 时先向 SSE 订阅者广播 `engine:shutdown` 事件再排空连接退出（172 号）

---

## 项目管理

### 项目结构

标准 InkOS 项目目录：

```
my-novel/
├── inkos.json              # 项目配置（llm 服务项等；缺省键由引擎读侧宽松填充）
├── .inkos/
│   ├── secrets.json        # API 密钥（与系统 Keychain 双向同步，0600）
│   └── engine/             # Node 回退链路的引擎运行时（Rust 链路不用）
├── books/                  # 书籍数据（story/ 状态、chapters/ 正文、drafts/ 草稿）
└── logs/                   # 日志
```

### 切换项目

菜单 / 命令面板（⌘K）→ 切换项目，回到 picker 页；最近项目快速重入。

---

## 自动更新

基于 tauri-plugin-updater + Ed25519 签名（更新源为本 fork 的 Releases）：

- **应用更新**：检测 → 下载 → 安装 → 重启生效；失败自动回滚
- **引擎更新**：按**运行态后端分流**（166 号）——Rust 后端取 `inkos-engine-{ver}-{triple}` 独立包替换 `engine-rust` 目录，Node 后端取 Node 引擎包；安装前健康预检，失败拒绝落地
- **手动更新**：自动更新失败时从 Releases 手动下载覆盖安装（项目与配置保留）

---

## 导演驾驶舱（书籍详情）

- **一句话灵感** + **运行模式**（就绪即停 / 范围写作 / 全书）编辑保存
- **生成方向候选**：基于灵感卡一键生成 3 个备选方向（标题 / 钩子 / 差异化 / 置信度）
- **已选用方向**回显；阶段、已存章节数与续跑建议实时显示
- 建书完成时自动写入灵感卡（stage=directions），失败不阻断建书

## 桌面工作台（四期 UI 演进成果）

- **⌘K 命令面板**（39 命令）与 **⌘P 快速打开**（书 / 章 / 会话 / 设置）、**⌘/** 快捷键速查
- **标签页多任务**：⌘数字切换、pin、持久化、深链
- **四区工作台**：侧面板（⌘B 折叠）+ 右侧 dock + 底部任务流与日志 + 状态栏（书·章·字数 / SSE / 引擎状态）
- **专注模式**、聊天**消息级打字机**、**章节读写对照分屏**
- 主题 light / dark / auto + 密度档；reduced-motion 与键盘可达性随行
- 通知中心与任务停止；关窗入托盘保活，托盘退出走幂等清理

## 模型服务与技能

### 模型服务配置（模型配置页）

- **服务商列表**：38 个预设服务商按分组筛选（聚合 / 海外 / 国产 / 本地 / CodingPlan），支持搜索与「只看已连接」；**「自定义服务」区块置顶**——零配置接入只需点置顶的虚线卡。
- **自定义服务**：填名称、Base URL、API Key 后点**测试连接**——自动发现该端点的模型列表并匹配协议（Chat/Responses）与流式设置；保存后模型持久化。
- **聊天页选模型**：输入框上方模型标签点击切换（同服务的模型目录 + 其他已连接服务）。**未选模型直发**：只要存在已配置服务与密钥（配置文件 `llm.services` 段 + `.inkos/secrets.json`），未选模型的消息也可以直接发送——引擎会自动使用首个可用服务（Rust 引擎与 Node 回退端行为一致）；完全零配置时会提示先完成配置。
- 按任务路由（写作 / 审改 / 修复 / 检测 / 分析各指定模型）在 项目设置 → Agent 模型路由。

### Agent Skills（项目设置 → Agent Skills）

- **导入**：点「选择 Skill 文件夹」导入标准 `SKILL.md` 能力包（兼容 AgentSkills / OpenClaw 目录结构）；静态参考资料一并导入，脚本不会自动执行。
- **即时生效**：导入 / 删除后所有已打开的技能列表**自动热刷新**，无需刷新页面或重启应用。
- **强制启用**：聊天输入框旁「添加 Skill (+)」可为本轮消息强制指定技能；Chat 也会按意图自主调用。
- 部分技能为**模型专用**（不出现在用户列表，仅供 Agent 自主调用）；导入的技能带删除按钮可随时移除。

## 上下文计量

聊天输入框右下角的 **「N tok」徽章**显示本轮请求的上下文体量：

- **estimate**（启发式估算，默认）与 **usage**（模型返回的真实用量锚点修正后）两种来源，悬停可看明细（启发式 / 锚点 / 覆盖率）；
- 接近模型上下文窗口时徽章转**告警色**；会话压缩（R32）会在超限前自动摘要旧对话。

## 插件管理

- 入口：系统托盘菜单「插件管理…」（打开独立管理窗口）。
- 插件分两类入口文件：`.wasm`（WASM 沙箱强隔离：fuel/内存/执行时限四层限制）与其他可执行文件（进程隔离）；安装时逐项审批权限（文件读写 / 网络域名 / 命令白名单等，未声明即无能力）。
- 支持从远程插件注册表安装与更新；连续失败的插件会被自动禁用以保护运行时。
- 开发者向细节（WIT 契约 / 权限模型 / 双路径架构）见 [docs/plugin-system.md](./plugin-system.md)。
- 桌面壳原生面（托盘 / 插件管理窗 / 更新 / 单实例锁）的**发布前人工走查清单**见 [docs/DESKTOP_WALKTHROUGH.md](./DESKTOP_WALKTHROUGH.md)。

---

## 故障排查

详细版见 [TROUBLESHOOTING.md](./TROUBLESHOOTING.md)。常用入口：

### 应用无法启动

1. 查看崩溃日志：macOS `~/Library/Logs/InkOS Desktop/`、Windows `%APPDATA%\com.inkos.desktop\logs\`、Linux `~/.local/share/inkos-desktop/logs/`
2. 重置配置：备份并移除应用数据目录（macOS `~/Library/Application Support/com.inkos.desktop` 等）后重启

### 引擎启动失败

1. 设置面板核对引擎后端；诊断查看 `engine_backend`（实际生效）与端口、引擎版本
2. Rust 链路异常 → 切 `node` 后端对照（回退路径本身即可用）
3. 引擎日志位于应用日志目录；「切 Rust 后端后报错」类问题按既有先例做双端对照（以 Node 行为为准绳，165 号）

### Keychain 权限错误（macOS）

```bash
security unlock-keychain ~/Library/Keychains/login.keychain-db
```

Keychain 不可用时自动降级为警告，不阻塞使用。

### 项目打不开

1. 确认所选目录含 `inkos.json`（或为可新建的空目录）
2. `inkos.json` 缺 `llm` 键时引擎读侧宽松填充（noop 端点），不应报错；若报「expected object」类错误请反馈

---

## 高级配置

### 日志级别

```bash
RUST_LOG=debug /Applications/InkOS\ Desktop.app/Contents/MacOS/inkos-desktop   # macOS/Linux 语法
```

`error` / `warn` / `info`（默认）/ `debug` / `trace`。

### 环境变量速查

| 变量 | 作用 |
| --- | --- |
| `INKOS_ENGINE_BACKEND` | 引擎后端覆盖（rust\|node），优先级最高 |
| `INKOS_ENGINE_LOOPBACK_GUARD` | 引擎回环守卫开关（`0` 关闭；默认开） |
| `INKOS_ENGINE_ALLOWED_ORIGINS` | 回环守卫追加 Origin 白名单（逗号分隔） |
| `HTTP_PROXY` / `HTTPS_PROXY` | 引擎下载与更新检查代理 |

### 配置分层

桌壳配置走分层设置（settings 面板图形化编辑，落 TOML；env > 分层配置 > 默认值），引擎行为类开关（如引擎后端）都在其中。

---

## 获取帮助

- **文档**：本 `docs/` 目录与根 README（叙事基准）
- **问题反馈**：[GitHub Issues](https://github.com/lalanbv/inkosDesktopforRust/issues)
- **演进史**：[变更记录文档/](../变更记录文档/)（逐变更归档、编号连续）

---

**版本历史**: [CHANGELOG.md](../CHANGELOG.md)　|　**许可证**: [AGPL-3.0](../LICENSE)
