# InkOS Desktop 快速开始

> 5 分钟上手指南　|　版本 v0.2.0（对齐 164 号「默认 Rust 引擎直启」后的现状，叙事基准为根 README）

## 1. 获取应用

### 方式一：安装包

从 [GitHub Releases](https://github.com/lalanbv/inkosDesktopforRust/releases) 下载（v0.2.0 起由本地脚本打包、手动上传）：

- macOS：`.dmg`（Apple Silicon / Intel），双击打开后拖入 Applications
- Windows：`.msi` / `.exe`
- Linux：`.deb` / `.AppImage`

安装后默认使用**包内 Rust 引擎直接启动，无需安装 Node.js、无需联网下载引擎**；仅当显式切到 Node 回退后端时才会首启下载 Node bootstrap。

### 方式二：从源码运行（开发者）

```bash
# 1. 构建上游前端产物（需要 Node 22+ 与 pnpm 10.x）
./scripts/desktop-build-inkos.sh

# 2. 构建 Rust 引擎
cd engine-rs && cargo build --release

# 3. 拉起桌面壳
cd src-tauri && cargo run
```

完整环境要求与打包发版流程见根 README「从源码运行」「构建与发布」两节。

---

## 2. 首次启动

1. **选择项目**：窗口打开 picker 页——新建项目 / 打开已有 InkOS 项目目录（含 `inkos.json`）/ 最近项目
2. **引擎启动**：选定项目后桌壳自动拉起引擎并导航到工作台。默认 Rust 引擎（`inkos-engine-server`）秒级就绪；状态栏可见引擎与连接状态

> 引擎后端可在设置面板切换（rust / node），也可用环境变量 `INKOS_ENGINE_BACKEND` 覆盖；Rust 二进制缺失时自动回退 Node 并告警，诊断命令回显实际生效后端。

---

## 3. 创建第一本书

### 方式一：通过 Studio Chat

1. 在对话框输入：
   ```
   创建一本玄幻小说，书名《吞天魔帝》，预计 100 章，每章 3000 字
   ```
2. 系统确认后创建书籍，生成世界观设定、主要角色、卷章大纲

### 方式二：通过新建入口（对话式建书）

1. 侧栏 / ⌘K 命令面板点击「新建书籍」——进入建书对话流
2. 在对话中描述书名 / 题材 / 目标章节数 / 每章字数
3. （可选）上传创作简报 Markdown 文件
4. 系统生成确认卡，确认后建书并生成世界观设定、主要角色、卷章大纲；
   建书完成即写入导演灵感卡（书籍详情「导演驾驶舱」可直接生成方向候选）

> 提示：创建书籍前需先在设置中配置模型服务与 API Key（密钥经系统 Keychain 加密同步到项目 `.inkos/secrets.json`）。

---

## 4. 写第一章

点击「写下一章」后系统自动完成整个流水线：

```
1. 规划章节意图 —— 基于大纲、作者意图、当前进度
2. 编排上下文   —— 选择相关状态、伏笔、角色记忆
3. 生成草稿     —— Writer Agent 生成正文（消息级打字机呈现）
4. 自动审计     —— 多维度质量检查
5. 必要修订     —— 修复审计发现的问题（可配）
✓ 章节完成
```

- 左侧章节列表点击查看正文；生成过程可在底部任务流实时查看
- 想专注码字可开**专注模式**；对照修改可开**章节读写分屏**

---

## 5. 继续写作

在对话框输入 `连续写 5 章` 即可批量续写；生成中的任务可随时**停止**。审计发现问题但未自动修复时，进入待审阅草稿：批量通过 / 重写 / 手动修改。

---

## 6. 导出作品

- **整本书**：书籍详情 → 导出 → TXT / EPUB / Markdown
- **单章**：章节详情 → 右上角菜单 → 导出当前章

---

## 常见问题

### Q: macOS 提示「无法打开，因为来自身份不明的开发者」？

右键点击应用 → 「打开」→ 确认打开；或终端执行：

```bash
xattr -cr /Applications/InkOS\ Desktop.app
```

### Q: 如何配置 API Key？

设置面板 → 模型服务配置：选择服务商、粘贴 API Key、测试连接、保存。密钥加密存储于系统 Keychain 并与 `.inkos/secrets.json` 双向同步（0600 权限）。

零配置接入：模型配置页的**「自定义服务」区块已置顶**——点置顶虚线卡填 Base URL + API Key，测试连接会自动发现模型并匹配协议。

### Q: 引擎一直起不来怎么办？

1. 设置面板确认引擎后端选择（默认 rust）
2. 诊断命令 / 诊断面板查看 `engine_backend`（实际生效后端）与端口、引擎版本
3. Rust 引擎异常时可切 `node` 回退后端对照；详细排查见 [TROUBLESHOOTING.md](./TROUBLESHOOTING.md)

### Q: Keychain 权限错误（macOS）？

```bash
security unlock-keychain ~/Library/Keychains/login.keychain-db
```

或首次弹窗时点击「始终允许」；Keychain 不可用时应用降级为警告，不阻塞使用。

### Q: 写作速度慢怎么优化？

1. 换更快的模型（设置 → 模型服务，选 flash 类快速模型）
2. 降低每章目标字数
3. 减少修订轮数（修订门槛可配）

---

## 下一步

- 📖 完整 [用户指南](./USER_GUIDE.md)
- 🔧 [故障排查](./TROUBLESHOOTING.md)
- 🗂 变更与演进史见 [变更记录文档/](../变更记录文档/)（以根 README 与最新变更记录为准）

---

**更多能力**：短篇生成、封面、开放世界互动、多模型路由、文风仿写、插件系统（[docs/plugin-system.md](./plugin-system.md)）——桌面壳原生面（托盘/插件窗/更新）发布前人工核对见 [docs/DESKTOP_WALKTHROUGH.md](./DESKTOP_WALKTHROUGH.md)——详见[用户指南](./USER_GUIDE.md)与上游产品 README。

聊天页还会显示**上下文计量徽章**（当前会话 token 体量）；技能能力包在 项目设置 → Agent Skills 导入，导入/删除即时生效。
