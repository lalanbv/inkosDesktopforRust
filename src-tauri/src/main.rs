//! inkosDesktop 二进制入口：拉起 Tauri 窗口 → spawn sidecar → 健康探测 →
//! 接入 observer（SSE 路由 → 通知 + 托盘角标）+ lifecycle（托盘 + 信号钩子）。
//!
//! M3b（解 C2）：启动读 `projects.json` → 若 `last_opened` 有效则自动复用（快启）；
//! 否则主窗口（frontendDist=picker）显示项目选择器（最近列表 + 原生目录对话框），
//! 用户选定后 `choose_project` 命令持久化并 spawn sidecar。移除 temp project 占位。
//!
//! 设计要点：
//! - 不用 `#[tokio::main]`：Tauri 2 自带事件循环，异步用 `tauri::async_runtime::spawn`。
//! - sidecar `Child` 存进 `SidecarState`（managed state），`RunEvent::Exit` 触发
//!   `cleanup_sidecar`（进程组 kill，不留孤儿占端口）。
//! - loopback 加固：spawn 之前 `LoopbackGuard::lock(port)`，失败仅 log 不阻塞；
//!   `RunEvent::Exit` 时 `release(port)` best-effort。
//! - WebView 加载：`health_probe` 通过后 `window.eval(location.replace(sidecar_url))`。
//! - 关窗→隐藏保活：`CloseRequested` 除非 `ExitingFlag` 已置位，否则 prevent + hide。
//! - M3b：sidecar 启动逻辑提取为 [`spawn_sidecar_task`]，auto 复用与 picker 选择共用。
//!
//! Tauri 版本：2.11.5。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use serde::Serialize;
// Manager trait 在作用域里才能用 `app.get_webview_window` / `app_handle.try_state` / `app_handle.path()`。
use tauri::{Manager, RunEvent, WindowEvent};
// NotificationExt 才能用 `app.notification()`。
use tauri_plugin_notification::NotificationExt;
// DialogExt（M3b）：项目目录选择对话框（Rust 侧，不经 webview ACL）。
use tauri_plugin_dialog::DialogExt;

mod commands;

use inkos_desktop::config;
use inkos_desktop::engine;
use inkos_desktop::isolation::{platform_guard, LoopbackGuard};
use inkos_desktop::lifecycle::{
    cleanup_sidecar, install_signal_hooks, ExitingFlag, SidecarState, TrayController,
};
use inkos_desktop::observer::notifier::{IsUnfocusedFn, NativeNotifier, NotifyFn};
use inkos_desktop::observer::router::Router;
use inkos_desktop::observer::sse::SseClient;
use inkos_desktop::observer::tray_badge::{IncBadgeFn, TrayBadge};
use inkos_desktop::paths::{AppPaths, PathResolver};
use inkos_desktop::projects::{RecentProject, RecentProjects};
// M2b Task 5：secrets keychain 同步 + 文件监听回写。
use inkos_desktop::secrets;
use inkos_desktop::secrets::store::{KeyringStore, SecretStore};
use inkos_desktop::supervisor;
use inkos_desktop::workspace;

/// loopback guard 句柄 + 锁定的端口，存入 Tauri managed state（同 M1，未改）。
#[derive(Default)]
struct LoopbackGuardState {
    guard: Mutex<Option<Box<dyn LoopbackGuard>>>,
    port: Mutex<u16>,
}

impl LoopbackGuardState {
    fn new() -> Self {
        Self::default()
    }
    fn set_guard(&self, guard: Box<dyn LoopbackGuard>) {
        let mut g = self.guard.lock().expect("LoopbackGuardState guard mutex 中毒");
        *g = Some(guard);
    }
    fn record_port(&self, port: u16) {
        let mut p = self.port.lock().expect("LoopbackGuardState port mutex 中毒");
        *p = port;
    }
    fn take_guard(&self) -> Option<Box<dyn LoopbackGuard>> {
        let mut g = self.guard.lock().expect("LoopbackGuardState guard mutex 中毒");
        g.take()
    }
    fn current_port(&self) -> u16 {
        *self.port.lock().expect("LoopbackGuardState port mutex 中毒")
    }
}

/// secrets.json → keychain 回写 watcher 的关闭句柄（同 M2b，未改）。
struct SecretsWritebackState {
    shutdown: Arc<AtomicBool>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl SecretsWritebackState {
    fn shutdown_and_join(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let mut guard = self
            .handle
            .lock()
            .expect("SecretsWritebackState handle mutex 中毒");
        if let Some(handle) = guard.take() {
            if let Err(e) = handle.join() {
                eprintln!("[main] secrets-writeback watcher join 失败: panic={e:?}");
            }
        }
    }
}

/// M3b：项目选择 + 启动决策状态（setup 写入，picker 命令读取）。
///
/// - `auto_launched`：setup 时 `last_opened` 有效 → true（已自动 spawn）；picker 据此
///   决定显示选择器（needs_project = !auto_launched）还是"启动中"。
/// - `chosen`：`choose_project` 的双发防护（双击/回车），CAS 确保只 spawn 一次。
/// - `recents`：最近项目快照，供 `get_launch_state` 回前端列表。
struct LaunchState {
    projects_path: PathBuf,
    auto_launched: AtomicBool,
    chosen: AtomicBool,
    recents: Mutex<RecentProjects>,
}

/// `get_launch_state` 命令的返回 DTO（前端 picker 据此渲染）。
#[derive(Serialize)]
struct LaunchStateDto {
    needs_project: bool,
    recents: Vec<RecentProject>,
}

fn to_js_string_literal(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    })
}

fn main() {
    // M4a：可观测性初始化（tracing 日志 + panic hook）
    let log_dir = dirs::data_dir()
        .map(|d| d.join(config::APP_DATA_DIR_NAME).join("logs"))
        .unwrap_or_else(|| std::env::temp_dir().join(config::APP_DATA_DIR_NAME).join("logs"));
    let _log_guard = inkos_desktop::observability::init_logging(log_dir)
        .expect("init_logging 失败");

    let crash_dir = dirs::data_dir()
        .map(|d| d.join(config::APP_DATA_DIR_NAME).join("crashes"))
        .unwrap_or_else(|| std::env::temp_dir().join(config::APP_DATA_DIR_NAME).join("crashes"));
    inkos_desktop::observability::init_panic_hook(crash_dir)
        .expect("init_panic_hook 失败");

    tracing::info!("inkosDesktop 启动");

    // M5b：初始化配置管理器
    let app_data = dirs::data_dir()
        .map(|d| d.join(config::APP_DATA_DIR_NAME))
        .unwrap_or_else(|| std::env::temp_dir().join(config::APP_DATA_DIR_NAME));
    std::fs::create_dir_all(&app_data).ok();
    let config_state = commands::config::AppState::new(app_data.clone());

    // M6f：项目索引数据库（SQLite）+ 生命周期管理器。
    // 打开失败不应阻断启动——项目管理是增量功能，其余功能（引擎/工作区/配置）仍可用，
    // 因此这里记录错误并以 None 降级，命令层遇到 None 返回明确的用户可读错误。
    let project_db = app_data.join("projects.db");
    let project_state = match inkos_desktop::project::ProjectManager::new(&project_db) {
        Ok(manager) => Some(inkos_desktop::project::commands::AppState {
            project_manager: std::sync::Arc::new(manager),
        }),
        Err(e) => {
            tracing::error!("项目索引数据库打开失败（项目管理功能不可用）: {e:#}");
            None
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            cmd_get_launch_state,
            cmd_pick_project_dialog,
            cmd_choose_project,
            cmd_check_updates,
            cmd_apply_engine_update,
            cmd_apply_shell_update,
            cmd_get_diagnostics,
            workspace::cmd_list_workspaces,
            workspace::cmd_create_workspace,
            workspace::cmd_switch_workspace,
            workspace::cmd_delete_workspace,
            workspace::cmd_add_project_to_workspace,
            commands::config::get_config,
            commands::config::update_config,
            commands::config::reset_config,
            commands::config::load_workspace_config,
            commands::config::load_project_config,
            commands::config::start_config_watch,
            commands::config::stop_config_watch,
            inkos_desktop::project::commands::scan_projects,
            inkos_desktop::project::commands::add_project,
            inkos_desktop::project::commands::remove_project,
            inkos_desktop::project::commands::update_project,
            inkos_desktop::project::commands::get_project,
            inkos_desktop::project::commands::list_projects,
            inkos_desktop::project::commands::list_recent,
            inkos_desktop::project::commands::list_favorites,
            inkos_desktop::project::commands::toggle_favorite,
            inkos_desktop::project::commands::search_projects,
            inkos_desktop::project::commands::open_project,
            inkos_desktop::project::commands::check_project_health,
        ])
        .manage(SidecarState::new())
        .manage(LoopbackGuardState::new())
        .manage(ExitingFlag::new())
        .manage(config_state)
        .on_window_event(|window, event| {
            // 关窗 → 隐藏保活（除非来自"退出"意图）。
            if let WindowEvent::CloseRequested { api, .. } = event {
                let exiting = window
                    .app_handle()
                    .try_state::<ExitingFlag>()
                    .map(|f| f.is_set())
                    .unwrap_or(false);
                if !exiting {
                    api.prevent_close();
                    if let Err(e) = window.hide() {
                        eprintln!("[main] window.hide 失败: {e:#}");
                    }
                }
            }
        })
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // M6f：仅在数据库打开成功时托管项目状态；失败时命令层的 try_state
            // 返回 None，前端得到「项目管理不可用」而非静默 panic。
            // 注意：Tauri 的 setup 是单一回调（不是回调链），必须合并进这里——
            // 另起一个 .setup() 会静默覆盖本回调。
            if let Some(state) = project_state {
                app.manage(state);
            }

            // =========================================================
            // C10 修复（同 M1+M2）：install_signal_hooks 提前到 spawn sidecar 之前
            // =========================================================
            let sig_app = app.handle().clone();
            let cleanup: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if let Some(flag) = sig_app.try_state::<ExitingFlag>() {
                    flag.set();
                }
                if let Some(obs_shutdown) = sig_app.try_state::<ObserverShutdown>() {
                    obs_shutdown.0.store(true, Ordering::SeqCst);
                }
                if let Some(state) = sig_app.try_state::<SidecarState>() {
                    let child = state.take();
                    cleanup_sidecar(child);
                }
                if let Some(wb_state) = sig_app.try_state::<SecretsWritebackState>() {
                    wb_state.shutdown_and_join();
                }
                if let Some(guard_state) = sig_app.try_state::<LoopbackGuardState>() {
                    let port = guard_state.current_port();
                    if let Some(guard) = guard_state.take_guard() {
                        if let Err(e) = guard.release(port) {
                            eprintln!(
                                "[main] signal cleanup: loopback release port={port} 失败: {e:#}"
                            );
                        }
                    }
                }
            });
            install_signal_hooks(cleanup);

            // =========================================================
            // M3b（解 C2）：读 projects.json → 决策启动模式
            // =========================================================
            let app_data = dirs::data_dir()
                .map(|d| d.join(config::APP_DATA_DIR_NAME))
                .unwrap_or_else(|| std::env::temp_dir().join(config::APP_DATA_DIR_NAME));
            std::fs::create_dir_all(&app_data).ok();

            // =========================================================
            // M5b：初始化配置管理器
            // =========================================================
            let _config_state = commands::AppState::new(app_data.clone());

            // =========================================================
            // M5a（Phase 3）：工作区迁移（Phase 2 → Phase 3）
            // =========================================================
            match inkos_desktop::workspace::migration::migrate_from_phase2(&app_data) {
                Ok(true) => tracing::info!("✅ Phase 2 数据已迁移至默认工作区"),
                Ok(false) => tracing::debug!("跳过迁移：workspaces.json 已存在或无旧数据"),
                Err(e) => tracing::warn!("迁移失败（继续运行）: {:#}", e),
            }

            let projects_path = app_data.join(config::PROJECTS_FILE_NAME);
            let recents = RecentProjects::read(&projects_path).unwrap_or_else(|e| {
                eprintln!("[main] 读取 projects.json 失败，降级空列表: {e:#}");
                RecentProjects::default()
            });

            // last_opened 有效 → 自动复用（快启）；否则等待 picker 选择。
            let auto_project: Option<PathBuf> = recents
                .last_opened
                .as_deref()
                .filter(|p| Path::new(p).is_dir())
                .map(PathBuf::from);
            let auto_launch = auto_project.is_some();
            if let Some(p) = &auto_project {
                eprintln!("[main] 复用上次项目 {}（auto 启动 sidecar）", p.display());
                spawn_sidecar_task(app_handle.clone(), p.clone());
            } else {
                eprintln!("[main] 无有效上次项目；主窗口显示 picker 等待选择");
            }
            app.manage(LaunchState {
                projects_path,
                auto_launched: AtomicBool::new(auto_launch),
                chosen: AtomicBool::new(false),
                recents: Mutex::new(recents),
            });

            // =========================================================
            // M3d：updater 状态（engine + shell 通道）
            // =========================================================
            let repo = std::env::var("INKOS_REPO")
                .unwrap_or_else(|_| "lalanbv/inkosDesktopforRust".to_string());
            let current_engine_version = {
                let manifest_path =
                    resolve_launch_engine(&app_handle).join(config::ENGINE_MANIFEST_FILE);
                inkos_desktop::engine::manifest::EngineManifest::read(&manifest_path)
                    .map(|m| m.engine_version)
                    .unwrap_or_else(|_| "0.0.0".to_string())
            };
            let engine_base = dirs::data_dir()
                .map(|d| d.join(config::APP_DATA_DIR_NAME))
                .unwrap_or_else(|| std::env::temp_dir().join(config::APP_DATA_DIR_NAME));
            app.manage(UpdaterState {
                repo,
                current_engine_version,
                engine_dir: engine_base.join(config::ENGINE_DIR_NAME),
                bak_dir: engine_base.join(config::ENGINE_BAK_DIR_NAME),
                staging_dir: engine_base
                    .join(config::UPDATES_DIR_NAME)
                    .join(config::STAGING_DIR_NAME),
                apply_lock: Arc::new(tokio::sync::Mutex::new(())),
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app_handle, event| {
            // Exit：进程即将退出，确保 sidecar 进程组被清理 + watcher 关闭 + guard release。
            if let RunEvent::Exit = event {
                if let Some(flag) = app_handle.try_state::<ExitingFlag>() {
                    flag.set();
                }
                if let Some(state) = app_handle.try_state::<SidecarState>() {
                    let child = state.take();
                    cleanup_sidecar(child);
                }
                if let Some(wb_state) = app_handle.try_state::<SecretsWritebackState>() {
                    wb_state.shutdown_and_join();
                }
                if let Some(guard_state) = app_handle.try_state::<LoopbackGuardState>() {
                    let port = guard_state.current_port();
                    if let Some(guard) = guard_state.take_guard() {
                        if let Err(e) = guard.release(port) {
                            eprintln!(
                                "[main] loopback guard: release port={port} 失败: {e:#}"
                            );
                        }
                    }
                }
            }
        });
}

/// M3d：解析启动 engine 路径（app_data 更新副本优先 → 否则 dev/prod 源）。
/// setup（读 manifest 版本）与 spawn_sidecar_task（build_launch）共用。
fn resolve_launch_engine(app_handle: &tauri::AppHandle) -> PathBuf {
    let app_data_engine = dirs::data_dir()
        .map(|d| d.join(config::APP_DATA_DIR_NAME).join(config::ENGINE_DIR_NAME))
        .unwrap_or_else(|| std::env::temp_dir().join(config::ENGINE_DIR_NAME));
    if app_data_engine.is_dir() {
        return app_data_engine;
    }
    let dev_engine_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let resource_dir = app_handle.path().resource_dir().ok();
    engine::resolve_engine_dir(resource_dir.as_deref(), dev_engine_root)
}

/// M3d：updater 状态（engine + shell 通道共享）。setup 写入，命令读取。
struct UpdaterState {
    /// 本仓 "owner/repo"（从 env 或默认 origin 解析；fallback 占位）。
    repo: String,
    /// 当前 engine 版本（manifest.engine_version；读失败 → "0.0.0" 视为总需更新）。
    current_engine_version: String,
    /// app_data/engine（updater 替换目标）。
    engine_dir: PathBuf,
    /// app_data/engine.bak（回滚备份）。
    bak_dir: PathBuf,
    /// app_data/updates/staging（下载暂存）。
    staging_dir: PathBuf,
    /// C2 审计修复：apply 串行锁（防双击/并发调用破坏 engine 状态）。
    apply_lock: Arc<tokio::sync::Mutex<()>>,
}

/// `check_updates` 返回 DTO（前端 + 日志用）。
#[derive(Serialize)]
struct UpdatesDto {
    engine: inkos_desktop::updater::ReleaseInfo,
    shell: inkos_desktop::updater::ReleaseInfo,
}

/// M3b：提取的 sidecar 启动任务。auto 复用（last_opened）与 picker 选择（choose_project）
/// 共用此入口。所有失败仅 log + 窗口 title 回显，不 panic（与 M1/M2 一致的降级纪律）。
fn spawn_sidecar_task(app_handle: tauri::AppHandle, project_root: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let window = match app_handle.get_webview_window("main") {
            Some(w) => w,
            None => {
                eprintln!("[main] spawn_sidecar_task: main 窗口缺失，放弃启动");
                return;
            }
        };
        let outcome = async {
            // M3d：engine 解析（app_data 更新副本优先 → 否则 dev/prod 源）。
            let launch_engine = resolve_launch_engine(&app_handle);

            // project_root 由参数传入（auto=last_opened / picker=用户选择）。确保 cwd 存在：
            std::fs::create_dir_all(&project_root)
                .with_context(|| format!("创建 project_root 失败: {}", project_root.display()))?;
            let paths = AppPaths::new(project_root, launch_engine)
                .context("解析 AppPaths 失败")?;

            // M2b：secrets_path = project_root/.inkos/secrets.json。先确保 .inkos/ 在：
            let secrets_path = paths
                .project_root()
                .join(config::SECRETS_DIR_NAME)
                .join(config::SECRETS_FILE_NAME);
            std::fs::create_dir_all(
                secrets_path
                    .parent()
                    .context("secrets_path 应有 .inkos/ 父目录")?,
            )
            .with_context(|| format!("创建 .inkos/ 失败: {}", secrets_path.display()))?;

            let store: Arc<dyn SecretStore> = Arc::new(KeyringStore::new("inkosDesktop"));
            let syncing = Arc::new(AtomicBool::new(false));

            // 同步 keychain ↔ secrets.json。降级：失败仅 log，不阻塞（inkos 仍可读 secrets.json）。
            if let Err(e) = secrets::sync_on_startup(&*store, &secrets_path, &syncing) {
                eprintln!("[secrets] keychain 同步失败，降级明文存储: {e:#}");
            }

            let port = supervisor::pick_free_port(config::DEFAULT_STUDIO_PORT)
                .context("pick_free_port 在 [4567, 5567) 区间无空闲端口")?;

            // =====================================================
            // M3c：运行时 Node 自适应 bootstrap
            // =====================================================
            // 缓存命中即用（后续启动零网络）；否则 region-aware 下载（CN 优先 npmmirror），
            // 官方 SHASUMS256 校验，解压到 app_data/runtime/node/{ver}-{os}-{arch}/。
            // 失败（无网/校验不过/解压失败）→ 回退系统 node（依赖 PATH），log 不阻塞。
            let cache_dir = paths.runtime_dir().join(config::NODE_DIR_NAME);
            let resolver = inkos_desktop::engine::node::BootstrappingResolver::new(
                cache_dir,
                Box::new(inkos_desktop::engine::node::LocaleMirrorSelector),
            )
            .with_progress(Arc::new(|stage: &str| {
                eprintln!("[node] bootstrap: {stage}");
            }));
            let node_bin: String = match resolver.resolve().await {
                Ok(p) => {
                    eprintln!("[main] node bootstrap 完成: {}", p.display());
                    p.to_string_lossy().into_owned()
                }
                Err(e) => {
                    eprintln!("[main] node bootstrap 失败，回退系统 node（依赖 PATH）: {e:#}");
                    "node".to_string()
                }
            };

            // loopback 加固：spawn 之前 lock。失败仅 log 警告、继续启动。
            let guard = platform_guard();
            match guard.lock(port) {
                Ok(()) => {
                    eprintln!("[main] loopback guard: 已 lock port={port}（外部入站被挡）");
                    if let Some(state) = app_handle.try_state::<LoopbackGuardState>() {
                        state.record_port(port);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[main] loopback guard: lock port={port} 失败（多半缺权限），降级继续: {e:#}"
                    );
                }
            }
            if let Some(state) = app_handle.try_state::<LoopbackGuardState>() {
                state.set_guard(guard);
            }

            let spec = supervisor::build_launch(&paths, port, &node_bin);
            let child = supervisor::spawn(&spec).context("spawn sidecar 失败")?;

            Ok::<(u16, std::process::Child, Arc<dyn SecretStore>, Arc<AtomicBool>, PathBuf), anyhow::Error>((
                port,
                child,
                store,
                syncing,
                secrets_path,
            ))
        }
        .await;

        match outcome {
            Ok((port, child, store, syncing, secrets_path)) => {
                if let Some(state) = app_handle.try_state::<SidecarState>() {
                    state.insert(child);
                } else {
                    eprintln!("[main] 警告: SidecarState 未注册，无法清理 child");
                }

                // M2b：spawn sidecar 后启动 secrets writeback watcher。
                let wb_shutdown = Arc::new(AtomicBool::new(false));
                let wb_handle = secrets::spawn_writeback(
                    store.clone(),
                    Arc::new(secrets_path),
                    syncing.clone(),
                    wb_shutdown.clone(),
                );
                app_handle.manage(SecretsWritebackState {
                    shutdown: wb_shutdown,
                    handle: Mutex::new(Some(wb_handle)),
                });

                let healthy =
                    supervisor::health_probe(port, config::HEALTH_PROBE_TIMEOUT).await;

                if healthy {
                    let url = format!("http://127.0.0.1:{}/", port);
                    if let Err(e) = window.eval(format!("window.location.replace('{}')", url)) {
                        eprintln!("[main] eval navigate 失败: {e}");
                    }
                    wire_observer_and_lifecycle(&app_handle, port);
                } else {
                    let secs = config::HEALTH_PROBE_TIMEOUT.as_secs();
                    let _ = window.eval(format!(
                        "document.title='inkos 启动超时 ({}s)，见日志'",
                        secs
                    ));
                    eprintln!(
                        "[main] sidecar 在 {}s 内未健康，未导航 webview（observer 未接线）",
                        secs
                    );
                }
            }
            Err(e) => {
                let raw = format!("inkos 启动失败: {:#}", e).replace(['\n', '\r'], " ");
                let literal = to_js_string_literal(&raw);
                let _ = window.eval(format!("document.title={}", literal));
                eprintln!("[main] sidecar 启动失败: {e:#}");
            }
        }
    });
}

// =========================================================
// M3b：picker 命令（自定义 app 命令，默认可 invoke，无需 capability）
// =========================================================

/// 返回启动状态：needs_project=true 时前端显示 picker；false 显示"启动中"。
#[tauri::command]
fn cmd_get_launch_state(state: tauri::State<LaunchState>) -> LaunchStateDto {
    let recents = state.recents.lock().expect("LaunchState recents mutex 中毒");
    LaunchStateDto {
        needs_project: !state.auto_launched.load(Ordering::Relaxed),
        recents: recents.recent.clone(),
    }
}

/// 原生目录选择对话框（Rust 侧 DialogExt，不经 webview ACL）。返回选中目录或 None。
#[tauri::command]
fn cmd_pick_project_dialog(app_handle: tauri::AppHandle) -> Option<String> {
    let picked = app_handle
        .dialog()
        .file()
        .set_title("选择 inkos 项目目录")
        .blocking_pick_folder();
    // FilePath::as_path() 对本地 Path 变体返回 Some（远程 Url 变体返回 None）。
    picked.and_then(|fp| fp.as_path().map(|p| p.to_string_lossy().into_owned()))
}

/// 用户选定项目：持久化到 projects.json + spawn sidecar。双发防护（CAS）。
#[tauri::command]
fn cmd_choose_project(
    path: String,
    state: tauri::State<LaunchState>,
    app_handle: tauri::AppHandle,
) -> inkos_desktop::error::Result<()> {
    use inkos_desktop::error::AppError;

    // 双发防护：双击/回车只 spawn 一次。
    if state.chosen.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        // 重置 chosen 允许重试（此次未真正启动）。
        state.chosen.store(false, Ordering::SeqCst);
        return Err(AppError::project("项目目录不存在")
            .with_details(format!("路径: {}", path))
            .with_suggestion("请选择一个有效的 InkOS 项目目录（包含 inkos.json）"));
    }
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    {
        let mut recents = state.recents.lock().expect("LaunchState recents mutex 中毒");
        let next = recents.record_open(&path, &name);
        *recents = next.clone();
        if let Err(e) = next.write(&state.projects_path) {
            eprintln!("[main] 持久化 projects.json 失败: {e:#}");
        }
    }
    eprintln!("[main] 用户选择项目 {}（picker → spawn sidecar）", p.display());
    spawn_sidecar_task(app_handle, p);
    Ok(())
}

// =========================================================
// M3d：updater 命令（engine 通道 + shell 通道）
// =========================================================

/// 查 engine + shell 两个通道的最新版本。失败（无网/无 release/未签名）保守视为无更新。
#[tauri::command]
async fn cmd_check_updates(
    state: tauri::State<'_, UpdaterState>,
    app_handle: tauri::AppHandle,
) -> inkos_desktop::error::Result<UpdatesDto> {
    use inkos_desktop::updater::{engine::EngineChannel, ReleaseInfo};

    let engine_ch =
        EngineChannel::new(state.repo.clone(), state.current_engine_version.clone());
    let engine = match engine_ch.check().await {
        Ok(Some(tag)) => ReleaseInfo {
            channel: "engine",
            version: tag.trim_start_matches('v').to_string(),
            needs_update: true,
        },
        _ => ReleaseInfo {
            channel: "engine",
            version: state.current_engine_version.clone(),
            needs_update: false,
        },
    };

    let shell_version = app_handle.package_info().version.to_string();
    let shell = match check_shell_update(&app_handle).await {
        Ok(Some(v)) => ReleaseInfo {
            channel: "shell",
            version: v,
            needs_update: true,
        },
        _ => ReleaseInfo {
            channel: "shell",
            version: shell_version,
            needs_update: false,
        },
    };
    Ok(UpdatesDto { engine, shell })
}

/// shell 通道探测（Tauri updater）。失败（无 release/未配置 pubkey）→ None，不报错。
async fn check_shell_update(app_handle: &tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app_handle.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(update.version)),
        Ok(None) => Ok(None),
        Err(e) => {
            eprintln!("[updater] shell 通道 check 失败（可能无签名 release）: {e}");
            Ok(None)
        }
    }
}

/// 应用 engine 更新（下载 → SHA256 → 原子替换 + 回滚）。下次启动生效（app_data/engine）。
#[tauri::command]
async fn cmd_apply_engine_update(
    state: tauri::State<'_, UpdaterState>,
) -> inkos_desktop::error::Result<String> {
    use inkos_desktop::error::AppError;
    use inkos_desktop::updater::engine::EngineChannel;

    // C2 审计修复：串行化 apply（防双击/并发调用并发下载/替换破坏 engine 状态）。
    // tokio::sync::Mutex::lock().await 返回 MutexGuard（非 Result，无中毒概念）。
    let _lock = state.apply_lock.lock().await;
    let engine_ch =
        EngineChannel::new(state.repo.clone(), state.current_engine_version.clone());

    // 先 check 确认有更新（友好错误）；apply 内部再 fetch release + 下载 + 校验 + 替换。
    let latest_tag = engine_ch
        .check()
        .await
        .map_err(|e| AppError::update("检查 Engine 更新失败")
            .with_details(format!("{e:#}"))
            .with_suggestion("请检查网络连接，或稍后重试"))?
        .ok_or_else(|| AppError::update("Engine 已是最新版本"))?;

    let ver = latest_tag.trim_start_matches('v');
    engine_ch
        .apply(
            &state.engine_dir,
            &state.bak_dir,
            &state.staging_dir,
            &|bundle, dest| inkos_desktop::engine::node::extract_archive(bundle, dest),
        )
        .await
        .map_err(|e| AppError::update("Engine 更新失败")
            .with_details(format!("{e:#}"))
            .with_suggestion("请稍后重试，或手动下载最新 Engine"))?;

    Ok(format!("engine 已更新至 {ver}（重启 app 生效）"))
}

/// 应用 shell 更新（Tauri updater：下载 + Ed25519 验签 + 安装）。完成后提示重启。
#[tauri::command]
async fn cmd_apply_shell_update(app_handle: tauri::AppHandle) -> inkos_desktop::error::Result<String> {
    use inkos_desktop::error::AppError;
    use tauri_plugin_updater::UpdaterExt;

    let updater = app_handle.updater()
        .map_err(|e| AppError::update("初始化 updater 失败")
            .with_details(e.to_string()))?;

    let update = updater
        .check()
        .await
        .map_err(|e| AppError::update("检查应用更新失败")
            .with_details(e.to_string())
            .with_suggestion("请检查网络连接，或稍后重试"))?
        .ok_or_else(|| AppError::update("应用已是最新版本"))?;

    let ver = update.version.clone();
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| AppError::update("应用更新失败")
            .with_details(e.to_string())
            .with_suggestion("请稍后重试，或手动下载最新安装包"))?;

    Ok(format!("shell 已更新至 {ver}，请重启 app"))
}

/// M4a：获取诊断信息（版本/平台/路径/manifest/最近崩溃）。
#[tauri::command]
async fn cmd_get_diagnostics(app: tauri::AppHandle) -> inkos_desktop::error::Result<inkos_desktop::observability::diagnostics::DiagnosticInfo> {
    inkos_desktop::observability::diagnostics::cmd_get_diagnostics(app).await
}

/// M2a Task 6 接线：health_probe 通过后构建 tray + observer + 信号钩子（未改）。
fn wire_observer_and_lifecycle(app_handle: &tauri::AppHandle, port: u16) {
    let tray = TrayController::build(app_handle);
    app_handle.manage(tray);

    let is_unfocused_app = app_handle.clone();
    let is_unfocused: IsUnfocusedFn = Arc::new(move || {
        is_unfocused_app
            .get_webview_window("main")
            .map(|w| !w.is_focused().unwrap_or(false))
            .unwrap_or(true)
    });

    let notify_app = app_handle.clone();
    let notify: NotifyFn = Arc::new(move |title, body| {
        if let Err(e) = notify_app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
        {
            eprintln!("[main] notification.show 失败: {e:#}");
        }
    });
    let notifier = NativeNotifier { is_unfocused, notify };

    let badge_app = app_handle.clone();
    let inc_badge: IncBadgeFn = Arc::new(move || {
        if let Some(tc) = badge_app.try_state::<TrayController>() {
            tc.inc_badge();
        } else {
            eprintln!("[main] TrayBadge.inc_badge: TrayController 未注册（仅 log）");
        }
    });
    let badge = TrayBadge { inc_badge };

    let router = Arc::new(Router::default_table(Arc::new(notifier), Arc::new(badge)));
    let shutdown = Arc::new(AtomicBool::new(false));
    let events_url = format!("http://127.0.0.1:{}/api/v1/events", port);

    let observer_shutdown = ObserverShutdown(Arc::clone(&shutdown));
    app_handle.manage(observer_shutdown);

    let watcher_router = Arc::clone(&router);
    let watcher_shutdown = Arc::clone(&shutdown);
    let watcher_url = events_url.clone();
    tauri::async_runtime::spawn(async move {
        eprintln!("[main] observer watcher 启动: {}", watcher_url);
        let client = SseClient::new(watcher_url.clone());
        while !watcher_shutdown.load(Ordering::Relaxed) {
            let result = client
                .run(Arc::clone(&watcher_router), Arc::clone(&watcher_shutdown))
                .await;
            if watcher_shutdown.load(Ordering::Relaxed) {
                break;
            }
            match result {
                Ok(()) => eprintln!("[main] observer watcher: run 退出（Ok），500ms 后重启"),
                Err(e) => eprintln!("[main] observer watcher: run 异常: {e:#}，500ms 后重启"),
            }
            for _ in 0..5 {
                if watcher_shutdown.load(Ordering::Relaxed) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
        eprintln!("[main] observer watcher 退出（shutdown=true）");
    });
}

/// observer 任务的 shutdown 标志（同 M2a，未改）。
#[derive(Default, Clone)]
struct ObserverShutdown(Arc<AtomicBool>);

/// 编译期断言：managed state 必须 Send + Sync。
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SidecarState>();
    assert_send_sync::<LoopbackGuardState>();
    assert_send_sync::<ExitingFlag>();
    assert_send_sync::<ObserverShutdown>();
    assert_send_sync::<TrayController>();
    assert_send_sync::<SecretsWritebackState>();
    assert_send_sync::<LaunchState>();
    assert_send_sync::<UpdaterState>();
};

#[cfg(test)]
mod tests {
    use super::to_js_string_literal;

    #[test]
    fn to_js_string_literal_escapes_quotes_backslash_and_control() {
        assert_eq!(to_js_string_literal("hello"), r#""hello""#);
        assert_eq!(
            to_js_string_literal("inkos 启动失败: can't open"),
            r#""inkos 启动失败: can't open""#
        );
        assert_eq!(to_js_string_literal(r#"a"b"#), r#""a\"b""#);
        assert_eq!(to_js_string_literal(r"a\b"), r#""a\\b""#);
        assert_eq!(to_js_string_literal("a\nb"), "\"a\\nb\"");
        assert_eq!(to_js_string_literal(""), "\"\"");
        assert_eq!(to_js_string_literal("中文测试"), "\"中文测试\"");
        let raw = "inkos 启动失败: path 'C:\\foo\\bar' 不存在";
        let literal = to_js_string_literal(raw);
        assert!(literal.starts_with('"') && literal.ends_with('"'));
        assert_eq!(
            literal.chars().filter(|&c| c == '\\').count(),
            4,
            "反斜杠应每个转义为两个: {}",
            literal
        );
        assert!(to_js_string_literal(r#"a"b"#).contains(r#"\""#));
    }
}
