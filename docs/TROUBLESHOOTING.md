# InkOS Desktop 故障排查

> 常见问题诊断与解决方案。
>
> **本文已按当前形态全面复审（179 号，2026-09-07）**：默认 **Rust 引擎直启**（164 号，包内资源零下载）；Node sidecar 仅作回退。涉及 174–178 号新能力（任务检查点恢复、停止写作、bench 门禁）的条目见「任务与写作问题」。叙事以根 README 与最新变更记录为准。

## 目录

- [启动问题](#启动问题)
- [引擎问题](#引擎问题)
- [任务与写作问题](#任务与写作问题)
- [权限问题](#权限问题)
- [更新问题](#更新问题)
- [性能问题](#性能问题)
- [数据问题](#数据问题)
- [日志收集](#日志收集)

---

## 启动问题

### 应用无法启动（无响应）

**症状**： 双击图标后无反应，Dock/任务栏闪烁后消失

**诊断步骤**：

1. **检查应用日志**（tracing 输出，目录为 `应用数据目录/inkosDesktop/logs`）

   ```bash
   # macOS
   tail -50 ~/Library/Application\ Support/inkosDesktop/logs/*.log

   # Windows（PowerShell）
   Get-Content "$env:APPDATA\inkosDesktop\logs\*.log" -Tail 50

   # Linux
   tail -50 ~/.local/share/inkosDesktop/logs/*.log
   ```

2. **查找 panic 信息**（panic hook 会把崩溃快照写入 `crashes/` 子目录）

   ```bash
   grep -i "panic" ~/Library/Application\ Support/inkosDesktop/logs/*.log
   ls -lt ~/Library/Application\ Support/inkosDesktop/crashes/
   ```

**常见原因与解决**：

| 错误信息 | 原因 | 解决方案 |
|---------|------|---------|
| `init_logging 失败` | 应用数据目录无写权限 | 检查 `~/Library/Application Support/inkosDesktop/`（Win: `%APPDATA%\inkosDesktop`，Linux: `~/.local/share/inkosDesktop`）权限 |
| `WebView2 not found` (Windows) | 缺少 WebView2 运行时 | 下载安装 [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) |
| `libwebkit2gtk not found` (Linux) | 缺少 GTK 依赖 | `sudo apt install libwebkit2gtk-4.1-0` |
| `Address already in use` | 默认端口 4567 被占用 | 壳层会自动探测下一个空闲端口；若手工固定端口被占，请释放该端口（见「引擎问题 → 端口」） |

### 启动时闪退（崩溃转储）

**症状**： 应用启动几秒后崩溃

**诊断**：

```bash
# macOS: 系统崩溃报告
open ~/Library/Logs/DiagnosticReports/
ls -lt ~/Library/Logs/DiagnosticReports/ | grep -i inkos

# 应用内 panic 快照（panic hook 产物）
ls -lt ~/Library/Application\ Support/inkosDesktop/crashes/
```

**解决**：

1. **重置应用数据**（先备份）

   ```bash
   mv ~/Library/Application\ Support/inkosDesktop \
      ~/Desktop/inkosDesktop-backup-$(date +%Y%m%d)
   # 重新启动应用（会创建默认配置）
   ```

2. **检查磁盘空间**

   ```bash
   df -h ~   # 确保至少有 500MB 可用空间
   ```

3. **更新到最新版本**：从 [GitHub Releases](https://github.com/lalanbv/inkosDesktopforRust/releases) 下载覆盖安装。

---

## 引擎问题

> 后台知识：壳层默认直接拉起 **Rust 引擎**（`inkos-engine-server`，包内 `engine-rust/` 资源，零 Node）；仅当 Rust 二进制缺失或显式配置 `node` 时走 Node sidecar 回退。选择序：环境变量 `INKOS_ENGINE_BACKEND`（rust\|node）> 设置面板「引擎后端」> 默认 `rust`。**侧栏「环境诊断」页可随时查看当前生效后端**（`engine_backend: rust / node / unknown`）。

### 端口

- 引擎默认端口 **4567**（`DEFAULT_STUDIO_PORT`）。壳层启动引擎前用 `pick_free_port` 从默认端口起探测空闲端口，被占时自动后移——**一般无需手工处理**。
- 手动排查端口占用：

  ```bash
  lsof -i :4567   # macOS/Linux
  netstat -ano | findstr 4567   # Windows
  ```

### Rust 引擎启动失败（默认链路）

**症状**： 窗口打开但停在 picker/加载页，诊断页 `engine_backend` 显示 `unknown` 或告警提示已回退 Node

**诊断**：

```bash
# 1. 看壳层日志中的引擎段（supervisor / rustbin 关键字）
grep -iE "rustbin|engine|supervisor" ~/Library/Application\ Support/inkosDesktop/logs/*.log | tail -30

# 2. 确认 Rust 引擎资源目录存在
ls ~/Library/Application\ Support/inkosDesktop/engine-rust/   # updater 替换后的运行态副本（若更新过）
# 打包形态在应用资源内：InkOS Desktop.app/Contents/Resources/engine-rust/
```

**常见错误**：

| 现象 | 原因 | 解决 |
|------|------|------|
| `engine_backend: node`（非主动配置） | Rust 二进制缺失/损坏，自动回退 | 重装最新版（覆盖安装即恢复包内 `engine-rust/`）；或检查 `engine-rust/inkos-engine-server` 是否有执行权限 |
| 健康探测超时（30s） | 引擎起但 `/api/v1/health` 不通 | 查引擎日志段有无 panic；确认杀毒软件未拦截 `inkos-engine-server` |
| updater 更新引擎后启动失败 | 替换后的 `app_data/engine-rust` 损坏 | 删除 `app_data/engine-rust/`（壳层会回退到包内资源；`.bak` 备份可手工还原） |

### Node 回退链路问题

> 仅适用于回退链路。Node 引擎代码（`packages/cli/dist`）随应用打包在本地，**不需要下载**；bootstrap 需要下载的只是 **Node 运行时**（nodejs.org 官方 LTS 22.x，SHASUMS256 校验）。

**症状**： 回退链路上状态提示引擎未就绪

**诊断**：

```bash
# 1. Node 引擎目录（项目内）
ls -la /path/to/project/.inkos/engine/
# 应包含 manifest.json 与引擎产物；cat manifest.json 可看版本

# 2. Node 运行时缓存（bootstrap 产物；应用缓存目录）
ls ~/Library/Caches/com.inkos.desktop/node/   # macOS
ls "$env:LOCALAPPDATA\com.inkos.desktop\node" # Windows

# 3. 手动验证（有系统 node 时）
cd /path/to/project/.inkos/engine/ && node dist/cli/index.js studio --port 4567
```

**常见错误**：

| 错误 | 原因 | 解决 |
|------|------|------|
| bootstrap 下载失败 | 无网 / nodejs.org 不可达 | bootstrap 失败会自动回退系统 `node`（PATH 中装好 Node 22+ 即可）；或恢复网络后重启 |
| `ENOENT: no such file` | `.inkos/engine/` 不完整 | 重装应用（引擎代码随包分发，不单独下载） |
| `MODULE_NOT_FOUND` | 引擎产物损坏 | 同上 |

### CORS / 远程访问

- 引擎带**回环守卫 + CORS 回环反射**（170/173 号）：仅回环 Origin（localhost / 127.0.0.1 / [::1]）或 `INKOS_ENGINE_ALLOWED_ORIGINS` 白名单可访问；远端 Origin 直接 403。从局域网其他设备访问被拒属**预期安全行为**；确需放行时设置白名单环境变量（值为允许的完整 Origin，逗号分隔）。

---

## 任务与写作问题

> 174–178 号引入的任务检查点与停止能力。write-next 的检查点/停止随 `sessionId` 激活：书籍详情页/首页发起的写作使用稳定伪会话 `book:{书id}:write`；聊天工作台内 agent 发起的生产任务有独立（且更早就有）的检查点机制。

### 写作任务在应用重启后显示「已中断」

**现象**： 任务运行期间应用/引擎重启，任务卡显示 error 终态「任务已中断：Studio 服务在任务运行期间重启，任务未能继续。请重新发起。」

**说明**： 这是**预期行为**（174 号重启对账）——旧进程随任务一起消失，快照被改写为中断终态而不是永远转圈。章节内容不会被破坏（管线在安全点落盘，未完成的章不写入章节索引）。直接重新发起即可。

**手动清理残留快照**（如需）：

```bash
# 任务快照按会话存放（.inkos/tasks/，文件名为 URL 编码的 sessionId）
ls /path/to/project/.inkos/tasks/
rm "/path/to/project/.inkos/tasks/book%3Amy-book%3Awrite.json"
```

### 「停止写作」点了之后任务还在跑

**现象**： 书籍详情页点「停止写作」后，任务过一会儿才停（Rust 引擎），或提示未停止（Node 回退端）

**说明**：

- **Rust 引擎（默认）**：停止是**阶段边界语义**——在途的 LLM 调用完成后，管线在下一个安全检查点退出（不会硬杀导致章节写一半）。每路 LLM 调用可能耗时数十秒，请稍候；停止后任务卡显示 `Operation aborted` 终态，章节索引不受影响。
- **Node 回退端**：管线不支持取消，「停止」诚实返回未停止、任务会跑完（176 号语义）。需要立即停可重启应用。
- 聊天工作台内 agent 生产任务的「停止」走确认任务中止链路，与上述 REST 面相互独立。

### 技能导入后列表没有出现？

技能导入/删除会通过实时事件**自动热刷新**所有已打开的技能列表，无需手动刷新。若仍未出现：

1. 确认导入的是含 `SKILL.md` 的文件夹（AgentSkills / OpenClaw 兼容格式），导入时诊断信息会显示解析失败原因；
2. 该技能若被标记为模型专用（`user-invocable: false`），不会出现在用户列表（仅供 Agent 自主调用）；
3. 项目设置 → Agent Skills 顶部的诊断区会列出解析失败的定义文件。

### 任务完成后书籍列表/详情未刷新

写作完成事件（`write:complete`）经 SSE 推送驱动刷新。若 SSE 断连（状态栏「实时」标识变灰）：

1. 检查引擎是否存活（环境诊断页 / `curl http://127.0.0.1:<port>/api/v1/health`）
2. 切换页面或刷新窗口触发重连；重连时服务端会补发对账后的任务快照（174 号），写作态会自动恢复

---

## 权限问题

### Keychain 访问被拒绝 (macOS)

**症状**： 提示无法访问 Keychain 或密码保存失败；日志出现 `keyring Entry::new(service=inkosDesktop, …) 失败`

**背景**： LLM 密钥存放在系统 Keychain，service 名为 **`inkosDesktop`**（macOS Keychain / Windows Credential Manager / Linux Secret Service），启动期与项目 `.inkos/secrets.json` 双向同步。Keychain 不可用时应用**降级为警告，不阻塞使用**（仅文件存储）。

**解决**：

```bash
# 方法 1: 首次访问弹窗时点「始终允许」

# 方法 2: 解锁登录 Keychain
security unlock-keychain ~/Library/Keychains/login.keychain-db

# 方法 3: 删除旧条目后重新授权（service 固定为 inkosDesktop）
security delete-generic-password -s "inkosDesktop" -a "<业务key>"
# 查看现存条目：
security dump-keychain | grep -A2 inkosDesktop
```

### 文件读写权限错误

**症状**： 「无法写入项目文件」或 `Permission denied`

**诊断与解决**：

```bash
ls -la /path/to/project/
touch /path/to/project/.write-test && rm /path/to/project/.write-test

# macOS/Linux: 修复所有权与写权限
sudo chown -R $USER /path/to/project/
chmod -R u+w /path/to/project/

# Windows: 右键项目目录 → 属性 → 安全 → 编辑，给当前用户「完全控制」
```

---

## 更新问题

> 应用与引擎都用 tauri-plugin-updater（Ed25519 签名）。引擎资产按生效后端分流：Rust 引擎替换 `app_data/engine-rust/`（166 号），替换前留 `.bak` 备份。

### 自动更新失败

**诊断**：

```bash
grep -i "update" ~/Library/Application\ Support/inkosDesktop/logs/*.log | tail -20
df -h /Applications   # 至少需要 200MB
```

**解决**：

1. **手动更新**：退出应用 → 从 [GitHub Releases](https://github.com/lalanbv/inkosDesktopforRust/releases) 下载最新版覆盖安装。
2. **清理更新缓存**：`~/Library/Caches/com.inkos.desktop/updates/`（Win: `%LOCALAPPDATA%\com.inkos.desktop\updates`）。

### 更新后无法启动 / 引擎更新后异常

```bash
# Rust 引擎回滚：还原 updater 备份
ls ~/Library/Application\ Support/inkosDesktop/
mv ~/Library/Application\ Support/inkosDesktop/engine-rust \
   ~/Library/Application\ Support/inkosDesktop/engine-rust.broken
mv ~/Library/Application\ Support/inkosDesktop/engine-rust.bak \
   ~/Library/Application\ Support/inkosDesktop/engine-rust

# 应用整体回滚：从 Releases 下载上一个稳定版本覆盖安装
```

---

## 性能问题

### 应用卡顿或响应慢

**诊断**：

```bash
top -pid $(pgrep -f "inkos-desktop")   # macOS（Linux 用 pidof）
tasklist /FI "IMAGENAME eq inkos-desktop.exe"   # Windows
```

**优化方案**：

1. **清理日志**（tracing 日志在应用数据目录）

   ```bash
   find ~/Library/Application\ Support/inkosDesktop/logs/ -name "*.log" -mtime +30 -delete
   ```

2. **降低日志级别**：`RUST_LOG=warn` 环境变量启动（默认 info）。
3. **使用更快的模型**：设置 → 模型配置，切换到低延迟模型；写作耗时主要在 LLM 侧，与引擎性能无关。

### 引擎基准与性能回归

引擎热路径（敏感词审计扫描、SSE 广播分发）有 criterion 基准与本地门禁（177 号）：`pnpm bench:gate`。怀疑引擎层性能回退时运行它，与入库基线对比；写作耗时问题优先排查 LLM 服务端。

### 内存占用过高

1. **重启应用**（释放 webview 与引擎缓存）。
2. 检查是否同时开着多个大书项目（多项目 = 多引擎进程）。

---

## 数据问题

### 项目损坏无法打开

**诊断与修复**：

```bash
# 验证项目配置格式
jq . /path/to/project/inkos.json

# 修复前先备份整个项目目录
cp -r /path/to/project /path/to/project.backup
# 手动修正 inkos.json 后重试
```

### 章节数据丢失

**诊断**：

```bash
# 章节正文与索引
ls -la /path/to/project/books/*/chapters/
cat /path/to/project/books/*/chapters/index.json | jq .
```

**说明**： 引擎写章采用「安全点落盘」——只有完整通过撰写/审计/落盘装配的章才进入章节索引；中断的任务不会产生半章文件。引擎对真相文件更新使用原子替换（写临时文件后 rename），进程崩溃不会留下半写状态。若索引与正文不一致，可用 git 等版本管理恢复（建议对项目目录做版本管理）。

---

## 日志收集

### 收集完整诊断信息

提交 Issue 前，请收集以下信息：

```bash
mkdir ~/inkos-diagnostics-$(date +%Y%m%d) && cd ~/inkos-diagnostics-$(date +%Y%m%d)

# 1. 系统信息
uname -a > system-info.txt && sw_vers >> system-info.txt   # macOS

# 2. 应用版本：应用内「环境诊断」页可直接查看
#    （版本/平台/路径/engine manifest/最近崩溃/生效引擎后端）

# 3. 最近日志
tail -1000 ~/Library/Application\ Support/inkosDesktop/logs/*.log > recent-logs.txt

# 4. panic 快照（如有）
cp ~/Library/Application\ Support/inkosDesktop/crashes/*.json ./

# 5. 项目配置（移除敏感信息！）
cp /path/to/project/inkos.json ./
# 手动删除 apiKey 等字段；.inkos/secrets.json 永远不要附带

# 6. 打包
cd .. && tar -czf inkos-diagnostics-$(date +%Y%m%d).tar.gz inkos-diagnostics-$(date +%Y%m%d)/
```

### 启用详细日志

```bash
# macOS
RUST_LOG=debug RUST_BACKTRACE=1 /Applications/InkOS\ Desktop.app/Contents/MacOS/inkos-desktop

# Windows (cmd)
set RUST_LOG=debug && set RUST_BACKTRACE=1 && inkos-desktop.exe

# Linux
RUST_LOG=debug RUST_BACKTRACE=1 inkos-desktop
```

---

## 联系支持

如果以上方案都无法解决问题，请到 **[GitHub Issues](https://github.com/lalanbv/inkosDesktopforRust/issues)** 提交：

- 附上诊断报告压缩包（**务必先移除 secrets/apiKey**）
- 描述复现步骤与预期/实际行为
- 标注操作系统、应用版本（环境诊断页可查）与生效引擎后端

---

**相关文档**：
- [用户指南](./USER_GUIDE.md)
- [快速开始](./QUICK_START.md)
