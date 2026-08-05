//! inkosDesktop 二进制入口：拉起 Tauri 窗口 → spawn sidecar → 健康探测 → 导航到 SPA。
//!
//! 设计要点：
//! - 不用 `#[tokio::main]`：Tauri 2 自带事件循环，异步用 `tauri::async_runtime::spawn`。
//! - sidecar `Child` 存进 `SidecarState`（Tauri managed state），在 `RunEvent::Exit`
//!   触发 `cleanup_sidecar`，保证关窗即清进程组（不留孤儿占端口）。
//! - WebView 加载方案：setup 异步块内 `health_probe` 通过后，用
//!   `WebviewWindow::eval("window.location.replace('http://127.0.0.1:<port>/')")`
//!   在 webview 内做客户端导航。CSP 已在 `tauri.conf.json` 设 null；Tauri 2 默认不拦
//!   顶层 location 导航（区别于 fetch/XHR 受 CORS 限制）。
//!
//! Tauri 版本：2.11.5（见 `Cargo.lock`）。

use anyhow::Context;
// Manager trait 在作用域里才能用 `app.get_webview_window` 与 `app_handle.try_state`。
// Tauri 2 把这些方法放在 Manager trait 上（而非 inherent），必须显式导入。
use tauri::{Manager, RunEvent};

use inkos_desktop::config;
use inkos_desktop::lifecycle::{cleanup_sidecar, SidecarState};
use inkos_desktop::paths::AppPaths;
use inkos_desktop::supervisor;

fn main() {
    tauri::Builder::default()
        .manage(SidecarState::new())
        .setup(|app| {
            // 复制一份句柄进异步块，用于把 spawn 后的 Child 写入 managed state。
            let app_handle = app.handle().clone();
            // 主窗口由 tauri.conf.json 声明，这里取已存在的实例用于 eval 导航。
            let window = app
                .get_webview_window("main")
                .context("tauri.conf.json 声明的 main 窗口未找到")?;

            tauri::async_runtime::spawn(async move {
                let outcome = async {
                    // mono-repo：submodule_root = 仓库根（packages/cli/dist 在此）。
                    // 用 `CARGO_MANIFEST_DIR`（编译期烘焙的 src-tauri/ 绝对路径）的 parent 解析，
                    // 与 Task 5 集成测运行时 `std::env::var("CARGO_MANIFEST_DIR")` 等价。
                    // 不能用 `std::env::current_dir()`——`cargo run` 会把 cwd 设为 src-tauri/
                    // 而非仓库根，导致 packages/cli/dist/index.js 路径错位（实测踩过）。
                    let manifest_dir = env!("CARGO_MANIFEST_DIR");
                    let submodule_root = std::path::Path::new(manifest_dir)
                        .parent()
                        .context("CARGO_MANIFEST_DIR 应有父目录（仓库根）")?;

                    // project_root 用 temp 目录，inkos studio 会自动初始化 minimal project。
                    let project_root = std::env::temp_dir().join("inkos-m1-demo");
                    // inkos studio 启动时 cwd 必须存在（Command::current_dir 在 dir 缺失时
                    // 直接返回 NotFound，不会进入子进程）。先确保目录在。
                    std::fs::create_dir_all(&project_root)
                        .with_context(|| format!("创建 project_root 失败: {}", project_root.display()))?;
                    let paths = AppPaths::new(
                        project_root.to_path_buf(),
                        submodule_root.to_path_buf(),
                    )
                    .context("解析 AppPaths 失败")?;

                    let port = supervisor::pick_free_port(config::DEFAULT_STUDIO_PORT)
                        .context("pick_free_port 在 [4567, 5567) 区间无空闲端口")?;

                    let spec = supervisor::build_launch(&paths, port, "node");
                    let child = supervisor::spawn(&spec).context("spawn sidecar 失败")?;

                    Ok::<(u16, std::process::Child), anyhow::Error>((port, child))
                }
                .await;

                match outcome {
                    Ok((port, child)) => {
                        // 把 Child 存入 managed state，供 RunEvent::Exit 时清理。
                        if let Some(state) = app_handle.try_state::<SidecarState>() {
                            state.insert(child);
                        } else {
                            // 理论上 .manage 注册了必然能取出；走不到这条分支。
                            eprintln!("[main] 警告: SidecarState 未注册，无法清理 child");
                        }

                        let healthy = supervisor::health_probe(
                            port,
                            config::HEALTH_PROBE_TIMEOUT,
                        )
                        .await;

                        if healthy {
                            let url = format!("http://127.0.0.1:{}/", port);
                            // 客户端导航：在 webview 内替换 location。失败仅打 stderr，
                            // 不阻塞——用户会看到 stub 页面，可手动刷新。
                            if let Err(e) = window.eval(format!(
                                "window.location.replace('{}')",
                                url
                            )) {
                                eprintln!("[main] eval navigate 失败: {e}");
                            }
                        } else {
                            let secs = config::HEALTH_PROBE_TIMEOUT.as_secs();
                            let _ = window.eval(format!(
                                "document.title='inkos 启动超时 ({}s)，见日志'",
                                secs
                            ));
                            eprintln!(
                                "[main] sidecar 在 {}s 内未健康，未导航 webview",
                                secs
                            );
                        }
                    }
                    Err(e) => {
                        // 启动失败的错误回显到窗口 title，便于用户看到根因。
                        // msg 用单行：把换行替换成空格避免 JS 字符串断行。
                        let msg = format!("{:#}", e).replace(['\n', '\r'], " ").replace('\'', "\\'");
                        let _ = window.eval(format!(
                            "document.title='inkos 启动失败: {}'",
                            msg
                        ));
                        eprintln!("[main] sidecar 启动失败: {:#}", e);
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app_handle, event| {
            // Exit：进程即将退出，确保 sidecar 进程组被清理。
            // 用 Exit 而非 ExitRequested：后者可被拒绝/deref，前者必然触发。
            if let RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<SidecarState>() {
                    let child = state.take();
                    cleanup_sidecar(child);
                }
                // 兜底：如果 managed state 因任何原因没拿到 child（例如 setup
                // 异步块还没跑完用户就关窗），无副作用——此时无 sidecar 可清。
            }
        });
}

/// 编译期断言：传给 `.manage()` 的类型必须是 `Send + Sync`（Tauri managed state 契约）。
/// 类型一旦破坏约束，本条 const 在编译期失败，早于运行期。
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SidecarState>();
};
