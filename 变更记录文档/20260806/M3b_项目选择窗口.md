# 变更记录：M3b 项目选择窗口 + 解 C2 + 信号钩子修复

| 项 | 值 |
|---|---|
| 日期 | 2026-08-06 |
| 类型 | feat + fix（M3 第 2 子里程碑） |
| 范围 | projects 数据层 + picker 窗口 + 项目决策流 + 信号钩子运行时修复 |
| 测试 | 全绿（124 lib + 集成；clippy 净）；GUI 启动冒烟通过（picker 流 + SIGTERM cleanup） |
| 设计 | `03_M3设计/M3_总体设计.md` §4 M3b |

## 解除的阻断

- **C2**（审计延 M3）：移除 temp `project_root` 占位。启动读 `app_data/projects.json` →
  `last_opened` 有效则自动复用（快启）；否则主窗口（frontendDist=picker）显示项目选择器
  （最近列表 + 原生目录对话框），用户选定后 `choose_project` 持久化并 spawn sidecar。

## 主要变更

### 新增 `src-tauri/src/projects.rs`（纯逻辑，10 单测）
- `RecentProjects { recent, last_opened }` + `RecentProject { path, name }`。
- `read`（缺失→空，损坏→Err 不静默吞）、`write`（原子 0600，复用 `util::atomic_write_0600`）、
  `record_open`（不可变：置顶 + 去重 + 截断 MAX_RECENT=10 + last_opened 更新 + 空 name 取目录名）、`find`。

### 新增共享 `src-tauri/src/util.rs`
- 提取 `atomic_write_0600`（原 `secrets/jsonio.rs` 私有），secrets + projects 共用（DRY，审计重视此模式）。2 单测。

### `main.rs` 重构（解 C2）
- 提取 `spawn_sidecar_task(app_handle, project_root)`：auto 复用与 picker 选择共用 sidecar 启动入口。
- setup：读 projects.json → `last_opened` 有效则 auto spawn，否则等待 picker。
- 3 个自定义命令（默认可 invoke，无需 capability）：
  - `cmd_get_launch_state` → `{ needs_project, recents }`（picker 据此渲染）
  - `cmd_pick_project_dialog` → 原生目录对话框（Rust 侧 `DialogExt`，不经 webview ACL）
  - `cmd_choose_project` → 持久化 + spawn（CAS 双发防护 + 目录校验）
- `tauri-plugin-dialog` 插件 + `generate_handler!`。

### `picker/index.html`（前端 launch pad）
- withGlobalTauri：`window.__TAURI__.core.invoke` 调命令。
- needs_project=true 显示 picker（最近列表 + "选择文件夹"）；false 显示"启动中"。
- 选定后 `choose_project` → 显示"启动中" → Rust 健康后导航到 sidecar URL。

### `tauri.conf.json`
- `frontendDist: "picker"`、`app.withGlobalTauri: true`、`bundle.resources: ["engine"]`（M3a）。
- 删除孤儿 `stub/` 目录。

## 重要修复（M1/M2 潜在 bug，M3b 真 GUI 启动捕获）

- **`install_signal_hooks` 裸 `tokio::spawn` panic**（lifecycle.rs）：Tauri `setup` 在主线程、
  不在 Tokio 运行时上下文 → "there is no reactor running, must be called from the context
  of a Tokio 1.x runtime"。M1/M2 的冒烟仅测 sidecar curl，从未真启 GUI，故未暴露。
  改用 `tauri::async_runtime::spawn`（Tauri 2 惯用，内部经运行时句柄派发，任意上下文可调）。
  修复后 SIGTERM → cleanup 链路实证触发（启动冒烟日志可见）。

## 兼容性 / 扩展性 / 安全

- **兼容**：picker 为 pre-sidecar 原生窗口，不依赖 SPA/sidecar，解 C2 鸡生蛋；此窗口为
  M3c（Node bootstrap 进度）/ M3d（更新进度）复用基础设施。
- **扩展**：`RecentProjects` 纯逻辑可测；命令模式可加更多 picker 操作。
- **安全**：projects.json 0600（隐私相关路径）；目录校验防无效路径 spawn；双发防护。
- **零修改**：仅 `src-tauri/` 内；dialog 插件为新增依赖。

## 验证

- 全量 `cargo test` 绿（+12 新增：projects 10 + util 2）；clippy --all-targets 0 警告。
- GUI 启动冒烟（macOS）：首次无 projects.json → picker 显示；SIGTERM → cleanup 触发（信号钩子修复实证）。

## 下一步

M3c：运行时 Node 自适应 bootstrap（region-aware mirror + 官方 SHASUMS256 校验 + 本地缓存）。
