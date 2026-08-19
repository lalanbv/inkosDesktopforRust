# 147 · Phase3：选择文件夹取消卡死——主线程对话框死锁修复

- 日期：2026-08-19
- 类型：缺陷修复 + 体验优化
- 影响面：src-tauri 壳层（main.rs 一处命令 + picker/settings 两处前端）
- 用户报障：打开「选择文件夹」后点取消，应用整体卡死（macOS hang report：无响应 26.84s+）

## 一、现象与证据

用户在打包版 inkosDesktop.app（0.1.0）中点击「选择项目目录」，对话框内点「取消」后应用冻结。
系统 hang report（Incident F061DA5F）主线程栈给出决定性证据：

```
Thread "main"（DispatchQueue com.apple.main-thread）
  tauri::ipc::protocol::get::{{closure}}
    → tauri::webview::Webview::on_message
      → inkos_desktop::cmd_pick_project_dialog
        → tauri_plugin_dialog::FileDialogBuilder::blocking_pick_folder
          → std::sync::mpmc::Receiver::recv
            → Thread::park → semaphore_wait_trap   （park 26s+）
```

另证：importance donation 列表出现 `com.apple.appkit.xpc.openAndSavePanelService`——
系统开/存面板服务在等主线程回事件循环。

## 二、根因

`cmd_pick_project_dialog` 原为**同步命令**。Tauri v2 中同步命令经 IPC 自定义协议在
**主线程**上内联执行；`blocking_pick_folder` 内部用 channel `recv()` 阻塞取结果，会把
主线程 park 死。而 rfd（tauri-plugin-dialog 内嵌）在 macOS 上用
`beginWithCompletionHandler` 弹面板，**完成回调恰恰派发回主队列**——主线程被 recv
占住，回调永远进不来，取消与选中同样死锁。这是 Tauri 文档明示的反模式
（blocking API 必须在 async 命令或非主线程使用）。

## 三、修复（src-tauri/src/main.rs）

`cmd_pick_project_dialog` 改为 **async 命令 + 回调桥 oneshot**：

```rust
#[tauri::command]
async fn cmd_pick_project_dialog(
    app_handle: tauri::AppHandle,
) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app_handle
        .dialog()
        .file()
        .set_title("选择 inkos 项目目录")
        .pick_folder(move |picked| {
            // 回调在主线程派发：只 send，不做任何重活。
            let _ = tx.send(picked);
        });
    let picked = rx
        .await
        .map_err(|e| format!("目录选择对话框未返回结果: {e}"))?;
    Ok(picked.and_then(|fp| fp.as_path().map(|p| p.to_string_lossy().into_owned())))
}
```

- async 命令体跑在 Tauri async runtime（非主线程），`rx.await` 是纯异步等待、零线程阻塞；
- 主线程完全空出来转 AppKit 事件循环，面板正常弹出、事件正常处理、回调正常派发；
- 这正是 tauri-plugin-dialog 自家 JS 命令所用的同一模式；
- 返回值 `Result<Option<String>, String>` 对前端 `await invoke(...)` 透明兼容
  （Ok(None) → null → 取消路径静默返回，不弹错误横幅）。

全库审计：`blocking_pick*` 仅此一处自有调用，无同类隐患。

## 四、优化（前端防重入）

原生对话框打开期间连点按钮会叠加多个面板（每次 invoke 一个）。两处入口加 busy 守卫：

- `src-tauri/picker/index.html`：pick 按钮监听器加 `picking` 标志 + `finally` 复位；
- `src-tauri/picker/settings.html`：`installFromDir` 加 `pickingDir` 标志，顺带更新
  注释中过时的 `blocking_pick_folder` 表述。

## 五、验证链

| 项 | 结果 |
| --- | --- |
| `cargo clippy --all-targets`（src-tauri） | 0 警告 |
| `cargo test`（桌面壳全量） | 449+61 全过（19 ignored 为 CI 专用，另跑） |
| `cargo test --release --test e2e_startup -- --ignored` | 2/2（新二进制真实启动） |
| 修复入产物验证 | `rg -a` 确认新错误串「目录选择对话框未返回结果」在 .app 主二进制中 |
| .app 重建 | CFBundleExecutable=inkos-desktop、包内引擎 1.8.0 可运行、adhoc 签名 |
| dmg 重出 | hdiutil UDZO 268M，CRC VALID，shasum -c OK |

交互层验证（点取消不再卡死）需 GUI 实操，属用户侧复验范围——结构性根因
（主线程 park × 主队列回调互等）已消除，模式与插件官方实现一致。

## 六、发布物（覆盖更新）

- `src-tauri/target/release/bundle/macos/inkosDesktop.app`
- `src-tauri/target/release/bundle/dmg/inkosDesktop_0.1.0_aarch64.dmg`（268M）
- sha256：`9fe96430b01abcd4715c58cf184523eacc2af06b0ae209ce0f5bd249f3b3fe23`
- 清理了 145 号遗留的 629M `rw.6751.*.dmg` 中间镜像

## 七、教训

- Tauri v2 同步命令在主线程内联执行：任何 blocking 调用（对话框/channel/长计算）
  都是主线程死锁候选，对话框类必须 async 命令 + 回调桥；
- hang report 的主线程栈是这类问题的第一证据源，应作为 GUI 卡死类报障的标准排查入口。
