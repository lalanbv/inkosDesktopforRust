# 148 号变更：状态栏「关闭窗口」后窗口永久唤不回——唤回通道补全

- 日期：2026-08-19
- 模块：src-tauri（main.rs / lifecycle.rs）
- 类型：缺陷修复 + 设计优化
- 关联：147 号（同日，选择文件夹取消卡死）；发布链沿 145 号

## 一、现象

用户报告：在状态栏（macOS 菜单栏 App 菜单的「关闭窗口 ⌘W」）点击 close window 后，
**窗口再也显示不出来**——Dock 图标点击无效，应用进程仍在运行，只能强杀。

## 二、根因分析

关窗走的是既有「关窗→隐藏保活」路径：`CloseRequested` → 未置 `ExitingFlag` →
`prevent_close()` + `window.hide()`。该设计本身正确（托盘常驻应用的标准模式），
但它有一个**未兑现的设计前提：任何时刻都要有办法把窗口唤回**。实测有三处断裂：

1. **macOS Dock 点击无处理**（主因）：run 闭包只处理了 `RunEvent::Exit`，没有
   `RunEvent::Reopen`（对应 `applicationShouldHandleReopen`）。窗口全部隐藏后点
   Dock 图标，App 激活但窗口不现——这正是「再也显示不出来」的直接体验。
2. **托盘「显示窗口」不是全程存在**：`TrayController` 原在
   `wire_observer_and_lifecycle` 里构建，而该函数只在 sidecar 健康探测通过后才
   被调用。picker 阶段 / 健康探测超时期间关窗 → 应用既无托盘、Dock 又无效，
   唤回通道为零。
3. **无防御性兜底**：若未来出现销毁主窗口的路径（当前 CloseRequested 恒被拦截，
   理论不可达），`get_webview_window("main")` 为 None 时托盘 show 分支静默无效。

## 三、修复（三条唤回通道全程可用）

### 1. lifecycle.rs：统一入口 `show_main_window` + `LiveSidecarUrl`

- 新增 `pub fn show_main_window(app)`：show + set_focus；窗口缺失时**防御性重建**
  ——优先直连 `LiveSidecarUrl`（sidecar 已健康的 UI 地址），否则回 picker 首页
  （`WebviewUrl::App("index.html")`，frontendDist 根）。
- 新增 managed state `LiveSidecarUrl(Mutex<Option<tauri::Url>>)`：setup 时 manage
  （None），sidecar 健康探测通过后由 main.rs 写入 `http://127.0.0.1:{port}/`。
  作用：重建窗口不停留在 picker 的「启动中」死页。
- 托盘菜单「显示窗口」原内联 `w.show(); w.set_focus()` 改调 `show_main_window`，
  与 Dock 路径共用同一入口，避免逻辑漂移。

### 2. main.rs：托盘提前到 setup 构建

```rust
// 148 修复：托盘提前到 setup 构建（原在 sidecar 健康探测后才建）。
app.manage(LiveSidecarUrl::default());
let tray = TrayController::build(&app_handle);
app.manage(tray);
```

`wire_observer_and_lifecycle` 不再建托盘（只保留 observer 接线）。副作用评估：
picker 阶段起托盘即存在（未读:0 / 显示窗口 / 插件管理… / 退出），「退出」走同一
`ExitingFlag → app.exit(0) → RunEvent::Exit` 清理链，无新增风险；且 setup 在主线程
构建托盘，比原先在 async runtime 线程构建更贴近 tray-icon 的线程预期。

### 3. main.rs：run 闭包处理 `RunEvent::Reopen`（macOS Dock 点击）

```rust
.run(|app_handle, event| match event {
    RunEvent::Exit => { /* 原 cleanup 链不变 */ }
    // macOS Dock 图标点击（applicationShouldHandleReopen）
    #[cfg(target_os = "macos")]
    RunEvent::Reopen { has_visible_windows, .. } => {
        if !has_visible_windows {
            inkos_desktop::lifecycle::show_main_window(app_handle);
        }
    }
    _ => {}
});
```

`has_visible_windows` 语义遵循 macOS 原生行为：有可见窗口（如插件管理窗）时仅激活
不强行弹主窗；全隐藏时唤回主窗。全路径调用 + cfg 门控避免其它平台 unused import。

## 四、验证链

| 步骤 | 结果 |
| --- | --- |
| `cargo clippy --all-targets` | 0 警告 |
| `cargo test`（全量） | 449 主套件 + 集成/文档全绿，0 失败（与基线一致） |
| `cargo test --test e2e_startup -- --ignored`（真实 release 二进制） | 2/2 通过 |
| `.app` 重建 | CFBundleExecutable=inkos-desktop；包内引擎 `--version`=1.8.0；adhoc 签名在位 |
| 修复编入产物 | `rg -a` 二进制命中「主窗口已重建」「显示窗口」各 1 |
| dmg（UDZO） | `hdiutil verify` VALID；sha256 见下 |

发布物：`src-tauri/target/release/bundle/dmg/inkosDesktop_0.1.0_aarch64.dmg`（257M）
sha256 `2588d23176243a5c80a144abbe25ca94c9cbe959f4e6a5fd836126d6b732b699`
（边车 `.dmg.sha256` 已同步更新，`shasum -c` OK）

## 五、复验指引

装新 dmg 后：①菜单栏/⌘W 关闭窗口 → 点 Dock 图标 → 窗口应立即回来；②任意阶段
（含项目选择页）关窗 → 托盘「显示窗口」→ 窗口回来；③托盘「退出」→ 进程正常退出。

## 六、教训

- 「关窗→隐藏保活」不是一行 `prevent_close + hide`，而是一份**契约**：每一条把
  窗口藏起来的路径，都必须成对提供唤回路径。这次断裂点在「托盘迟到 + Dock 无处理」。
- macOS 托盘常驻应用的标准唤回面：Dock 点击（`RunEvent::Reopen`）+ 托盘菜单 +
  （防御）窗口重建。Tauri 不替你处理 Reopen，必须自己接。
- 托盘等「后置条件满足才建」的 UI 兜底组件是反模式——兜底通道必须先于需要它的
  场景存在（picker 阶段恰恰是最容易关窗的阶段）。
