//! 启动同步 + 文件监听回写：keychain ↔ secrets.json 双向对齐。
//!
//! ## 模块职责
//! - `sync_on_startup`：启动时一次性同步，保证 keychain 与 secrets.json 一致。
//!   两条路径（互斥）：
//!   1. keychain 非空 → keychain 主，覆盖 secrets.json（写文件前后置 SYNCING 标记，
//!      防 watcher 回写）。
//!   2. keychain 空 且 secrets.json 非空 → 首次迁移：secrets.json → keychain。
//!
//! 两者都空 → 无操作（Ok）。
//! - `spawn_writeback`：常驻 watcher 线程，监听 secrets.json 变更 → debounce
//!   → 防回环检查 → diff 回写 keychain（SPA 改/增/删 key 同步到 keychain）。
//!
//! ## 防回环
//! `sync_on_startup` 写 secrets.json 前设 `syncing=true`，写后 `false`。
//! watcher 处理前查 `syncing`：true → 跳过（programmatic 写不触发删除传播）；
//! SPA（前端 / 用户改 secrets.json，含删除 key）触发时 syncing=false → 正常 diff 回写。
//!
//! **删除传播**（M2b 修复 + C7 加固）：
//! SPA 的 `DELETE /api/v1/services/:service` 调 `delete secrets.services[service]; saveSecrets(...)`
//! （inkos server.ts），写出的 secrets.json 缺该 key。`compute_writeback_diff` 检测
//! "store 有 json 无"的 key → 加入 deletes；`process_writeback` 调 `store.delete`。
//! 这避免了 SPA 删 key 后下次启动 `sync_on_startup` path1 把 keychain 中的 key 写回
//! secrets.json 导致 **已删 key 复活** 的回归。
//!
//! **C7 加固**（M3 批次 3）：`process_writeback` 的 `skip_deletes` 兜底改用
//! [`SecretsFileState`](crate::secrets::jsonio::SecretsFileState) tri-state，
//! 仅在 JSON 真正不可解析（`Corrupt`）时跳过删除。合法清空（`Empty`，如
//! `{"services":{}}`）和文件缺失（`Absent`）视为用户意图 → 正常传播 deletes。
//! 这修复了旧版兜底误把"用户删唯一 service 写出合法 empty"判为"损坏"跳过删除，
//! 导致 keychain 残留 + 重启复活的回归。
//!
//! ## debounce
//! notify 收到首个事件后进入 500ms 静默窗：窗内每收一个事件重置 deadline，
//! 静默满 500ms 后才触发处理（聚合连续写，避免抖动 + 节省 keychain 调用）。
//!
//! ## 错误处理
//! - `read_all` / `read_secrets_state` / `write_secrets` / `upsert` 失败 → 显式向上传播
//!   （不静默吞错）。
//! - SYNCING 标记在 path 1（keychain → json）下保证复位：即便 `write_secrets` 失败
//!   也复位为 false，避免 watcher 回写。
//! - path 2（首迁）中途 `upsert` 失败 → 已迁移的 key 保留，函数返回 Err；
//!   调用方可在排错后重试（幂等：再次 `upsert` 同 key 只是覆盖）。
//! - `spawn_writeback` 内部错误（notify init / read / upsert）→ `eprintln!` log，
//!   watcher 继续（不 panic、不退出）；watcher 是 best-effort IO 层。

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::secrets::jsonio::{read_secrets, read_secrets_state, write_secrets, SecretsFileState};
use crate::secrets::store::SecretStore;

/// 启动时同步 keychain 与 secrets.json。
///
/// 行为契约（与 [`crate::secrets`] 模块文档一致）：
/// - **keychain 非空** → 用 keychain 内容覆盖 secrets.json。
///   写文件前设 `syncing=true`，写完后设 `syncing=false`（防 watcher 回写）。
///   即便 `write_secrets` 失败也保证 `syncing` 复位（避免 watcher 永久禁用）。
/// - **keychain 空 且 secrets.json 非空** → 首次迁移：把 secrets.json 中每个
///   `(serviceId, apiKey)` 调用 `store.upsert` 写入 keychain。
///   中途 `upsert` 失败 → 已写入的 key 保留，返回 Err（调用方可重试，幂等）。
/// - **两者都空** → 无操作（Ok）。
///
/// `path` 指向 `.inkos/secrets.json`（由调用方传入，通常是
/// `project_root.join(".inkos/secrets.json")`）。
pub fn sync_on_startup(store: &dyn SecretStore, path: &Path, syncing: &AtomicBool) -> Result<()> {
    let from_keychain = store.read_all()?;

    if !from_keychain.is_empty() {
        // path 1：keychain → secrets.json（keychain 主，覆盖文件）
        syncing.store(true, Ordering::SeqCst);
        let write_result = write_secrets(path, &from_keychain);
        // 不论成功失败都复位 SYNCING，避免 watcher 永久禁用（不静默吞错：
        // write 错误仍向下传播，只是先复位标记再返回 Err）
        syncing.store(false, Ordering::SeqCst);
        write_result?;
        return Ok(());
    }

    // keychain 空 → 尝试从 secrets.json 首迁
    let from_json = read_secrets(path)?;
    if from_json.is_empty() {
        // 两者都空：无操作
        return Ok(());
    }

    // path 2：secrets.json → keychain（首次迁移）
    for (service_id, api_key) in from_json {
        store.upsert(&service_id, &api_key)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// spawn_writeback：文件监听 + debounce + 防回环 + diff 回写
// ---------------------------------------------------------------------------

/// debounce 静默期：首个事件后聚合此窗内的连续写，超过则触发处理。
///
/// 500ms 对齐常见编辑器保存间隔 + atomic_write 双步（tmp+rename）间隔。
/// 测试依赖此时长；改值需同步调整 `#[ignore]` IO 测的 sleep。
pub const DEBOUNCE_MS: u64 = 500;

/// watcher 线程名（便于调试/`top`/Activity Monitor 识别）。
pub const THREAD_NAME: &str = "secrets-writeback";

/// watcher 主循环每次 `recv_timeout` 的轮询时长。
///
/// 100ms 让 shutdown 检查延迟保持低（用户感知 < 200ms），同时避免 busy-loop。
///
/// `pub` 是因为外部集成测（`tests/sync_unit.rs`）需引用此常量断言 shutdown 延迟
/// （`watcher_shutdown_flag_exits_thread_cleanly` 用 `POLL_TIMEOUT_MS * 3` 作为
/// 等待窗口）。
pub const POLL_TIMEOUT_MS: u64 = 100;

/// 启动 secrets.json 文件监听 + diff 回写 keychain 的 watcher 线程。
///
/// **设计选型**：用 `std::thread` 而非 `tokio::spawn`——`notify` 本身是同步 crate，
/// 强行套 tokio 无收益；独立线程让 watcher 不依赖外部 runtime（Tauri 启动早期 / 测试
/// 环境均可用）。任务接口签名（无返回 Future）也契合 fire-and-forget 线程模型。
///
/// 行为契约：
/// - 监听 `path` 所在**父目录**（非递归），事件路径过滤仅针对 `path` 文件名
///   （atomic_write tmp+rename 两次事件都会落在目标文件名上）。
/// - 收到目标文件事件 → 进入 `DEBOUNCE_MS` 静默窗聚合连续写。
/// - 静默满窗后触发 `process_writeback`：查 `syncing`，false 才 diff 回写。
/// - `shutdown` 为 true → 线程退出（下次循环检查，延迟 ≤ `POLL_TIMEOUT_MS`）。
/// - 任何 IO/notify 错误 → `eprintln!` log，watcher 继续（不 panic、不退出）。
///
/// 返回 `JoinHandle` 供调用方（测试或 shutdown 同步）可选 join。
pub fn spawn_writeback(
    store: Arc<dyn SecretStore>,
    path: Arc<PathBuf>,
    syncing: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name(THREAD_NAME.to_string())
        .spawn(move || run_writeback_loop(store, path, syncing, shutdown))
        .expect("spawn secrets-writeback watcher thread")
}

/// watcher 主循环：初始化 notify、recv 事件、debounce、process。
///
/// 拆出（非 pub）便于在测试里通过 `process_writeback` 直接验证 IO 路径，
/// 而不需要触发 notify timing。
fn run_writeback_loop(
    store: Arc<dyn SecretStore>,
    path: Arc<PathBuf>,
    syncing: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();

    let mut watcher: RecommendedWatcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            // watcher 初始化失败无法恢复 → eprintln log 并退出（不 panic）。
            // 调用方应观测日志；keychain 不再回写，但 sync_on_startup 已完成首启同步。
            eprintln!(
                "[secrets-writeback] notify watcher init failed: {e:#}; writeback disabled"
            );
            return;
        }
    };

    // 监听目标文件所在目录（监听单个文件在跨平台 backend 上不可靠：
    // FSEvents/inotify 都更稳于目录级 watch + 路径过滤）。
    let watch_dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
        eprintln!(
            "[secrets-writeback] failed to watch {}: {e:#}; writeback disabled",
            watch_dir.display()
        );
        return;
    }

    let target_file = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());

    let poll_timeout = Duration::from_millis(POLL_TIMEOUT_MS);
    let debounce = Duration::from_millis(DEBOUNCE_MS);

    while !shutdown.load(Ordering::SeqCst) {
        match rx.recv_timeout(poll_timeout) {
            Ok(Ok(event)) => {
                if event_targets_file(&event, target_file.as_deref()) {
                    debounce_and_process(&rx, debounce, &shutdown, &store, &path, &syncing);
                }
            }
            Ok(Err(e)) => {
                // 单个事件错误（如 backend 内部 glitch）→ log 继续，不退出 watcher。
                eprintln!("[secrets-writeback] notify event error: {e}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // watcher 句柄被 drop / 内部线程退出 → 退出循环。
                return;
            }
        }
    }
}

/// 进入 debounce 静默窗：窗内每收一个事件重置 deadline，
/// 静默满 `debounce` 时长后调用 `process_writeback`。
///
/// 窗内事件不再二次过滤目标文件名——首次已确认匹配，后续视为同活动簇。
fn debounce_and_process(
    rx: &mpsc::Receiver<notify::Result<notify::Event>>,
    debounce: Duration,
    shutdown: &AtomicBool,
    store: &Arc<dyn SecretStore>,
    path: &Arc<PathBuf>,
    syncing: &AtomicBool,
) {
    let mut deadline = Instant::now() + debounce;
    while !shutdown.load(Ordering::SeqCst) {
        let remaining = match deadline.checked_duration_since(Instant::now()) {
            Some(d) => d,
            None => break, // 静默期满 → 触发处理
        };
        match rx.recv_timeout(remaining) {
            Ok(Ok(_ev)) => {
                // 聚合：重置 deadline（继续等静默期）。
                deadline = Instant::now() + debounce;
            }
            Ok(Err(e)) => {
                eprintln!("[secrets-writeback] notify event error during debounce: {e}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }

    if let Err(e) = process_writeback(store.as_ref(), path, syncing) {
        // 处理失败不退出 watcher：下次事件再触发可能恢复。
        eprintln!("[secrets-writeback] process error: {e:#}");
    }
}

/// 处理一次回写：防回环 → read json → diff vs store → upsert + delete + 恢复 0600。
///
/// 行为契约（C7 修复后的完整语义）：
/// 1. **防回环**：`syncing=true`（programmatic write 进行中）→ 跳过（返回 Ok，不删不写）。
///    只有 SPA 触发（syncing=false）的写才会传播删除，避免 sync_on_startup 自身写
///    secrets.json 触发 watcher 把 keychain 内容删除。
/// 2. **read json (tri-state)**：通过 [`read_secrets_state`] 区分四种文件状态：
///    - [`Corrupt`](SecretsFileState::Corrupt)（JSON 不可解析）→ 保守跳过删除
///      （防瞬时损坏 / 并发写截断 / 磁盘错乱 → keychain 全部丢失的灾难）。
///    - [`Empty`](SecretsFileState::Empty)（合法 JSON 但 services 空，如 `{"services":{}}`）
///      → 视为**用户合法意图清空**，正常传播 deletes。
///    - [`Absent`](SecretsFileState::Absent)（文件不存在）→ 视为用户删文件，正常传播 deletes。
///    - [`Populated`](SecretsFileState::Populated) → 正常 diff 回写。
/// 3. **diff**：[`compute_writeback_diff`] 返回 upserts + deletes。
/// 4. **apply**：先 upsert 后 delete（顺序不重要，两者均幂等）。
/// 5. **恢复 0600**（M1 修复）：inkos `saveSecrets` 用默认 umask 写 secrets.json
///    （通常 0644），桌面壳作为特权方在每次 SPA 写后恢复 0600。Unix cfg-gate；
///    Windows 无 0600 语义，跳过。失败仅 log，不阻塞回写（chmod 失败不影响 keychain）。
///
/// **C7 修复背景**：旧版 `skip_deletes` 兜底条件 `from_json.is_empty() && !from_store.is_empty()`
/// 把"合法清空"与"瞬时损坏"折叠为同一空 map，导致用户删唯一 service 后写出合法
/// `{"services":{}}` 被误判为"损坏"跳过删除，keychain 残留，重启 `sync_on_startup`
/// path1 把残留 key 写回 secrets.json → **已删 key 复活**。
///
/// 此函数是 spawn_writeback 的可测核心（无 notify timing 依赖）：
/// 测试可直接调用以验证防回环与 diff 回写语义。
pub fn process_writeback(
    store: &dyn SecretStore,
    path: &Path,
    syncing: &AtomicBool,
) -> Result<()> {
    if !should_writeback(syncing) {
        return Ok(());
    }
    let (from_json, json_state) = read_secrets_state(path)?;
    let from_store = store.read_all()?;
    let diff = compute_writeback_diff(&from_json, &from_store);

    // C7 修复：仅在 JSON 真正不可解析（Corrupt）时跳过删除传播。
    // Empty（合法清空）/ Absent（用户删文件）→ 正常传播 deletes。
    // 这修复了：用户删唯一 service → 写出合法 {"services":{}} → 旧兜底误判为"损坏"
    // 跳过删除 → keychain 残留 → 重启 sync_on_startup path1 复活已删 key。
    let skip_deletes = json_state == SecretsFileState::Corrupt
        && !from_store.is_empty()
        && !diff.deletes.is_empty();
    if skip_deletes {
        eprintln!(
            "[secrets-writeback] 警告：secrets.json JSON 不可解析 + keychain 有 {} key，跳过删除传播（防损坏误删）",
            from_store.len()
        );
    }

    for (key, value) in diff.upserts {
        store.upsert(&key, &value)?;
    }
    if !skip_deletes {
        for key in diff.deletes {
            store.delete(&key)?;
        }
    }

    // M1 修复：恢复 secrets.json 权限到 0600（inkos saveSecrets 用默认 umask，常 0644）。
    // 桌面壳作为特权方在每次 SPA 写后恢复 0600。失败仅 log，不影响回写。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            eprintln!(
                "[secrets-writeback] 恢复 {} 权限 0600 失败（不影响回写）: {e}",
                path.display()
            );
        }
    }

    Ok(())
}

/// 防回环判定：`syncing=true`（programmatic write 进行中）→ 跳过（返回 false）。
///
/// 抽成纯函数便于单测全覆盖（防回环是本任务核心正确性保证）。
pub fn should_writeback(syncing: &AtomicBool) -> bool {
    !syncing.load(Ordering::SeqCst)
}

/// diff 计算结果：watcher 应应用到 keychain 的变更集。
///
/// - `upserts`：json 中"store 缺失或值不同"的 `(key, value)` 列表（新增 / 改值）。
/// - `deletes`：store 中"json 缺失"的 key 列表（**SPA 删除传播**，M2b 修复）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritebackDiff {
    pub upserts: Vec<(String, String)>,
    pub deletes: Vec<String>,
}

impl WritebackDiff {
    /// 便利构造：空 diff（两个空 HashMap 等价）。
    pub fn empty() -> Self {
        Self {
            upserts: Vec::new(),
            deletes: Vec::new(),
        }
    }
}

/// diff 计算：返回 SPA 写（secrets.json）相对 keychain（store）的差异变更集。
///
/// **策略**（M2b 修复后的完整语义）：
/// - **upserts**：json 中"store 缺失或值不同"的 key → 加入 upserts（新增 / 改值）。
/// - **deletes**：store 中"json 缺失"的 key → 加入 deletes（**传播 SPA 删除**）。
///   这修复了 SPA 调 `DELETE /api/v1/services/:service` → secrets.json 缺 key →
///   watcher 不删 store → 下次启动 sync_on_startup path1 把 keychain 中 key 写回
///   secrets.json 导致 **已删 key 复活** 的回归。
///
/// **防回环安全**（不在本函数，在 process_writeback）：
/// `syncing=true` 时 `process_writeback` 整体跳过——本函数是纯计算，syncing 期间
/// 不会被调用。故 compute 内无需考虑 syncing 语义。
///
/// 抽成纯函数便于单测全覆盖（diff 是 SPA → keychain 回写的核心语义）。
pub fn compute_writeback_diff(
    from_json: &HashMap<String, String>,
    from_store: &HashMap<String, String>,
) -> WritebackDiff {
    // upserts：json 中"store 缺失或值不同"
    let upserts: Vec<(String, String)> = from_json
        .iter()
        .filter(|(k, v)| match from_store.get(*k) {
            None => true,
            Some(existing) => existing != *v,
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // deletes：store 中"json 缺失"——SPA 删除传播的核心
    let deletes: Vec<String> = from_store
        .keys()
        .filter(|k| !from_json.contains_key(*k))
        .cloned()
        .collect();

    WritebackDiff { upserts, deletes }
}

/// 判断 notify 事件是否针对目标文件：`event.paths` 中任一路径的 file_name 等于
/// `target_file`。`target_file=None` 时视为无过滤（接受所有事件，保守）。
pub fn event_targets_file(event: &notify::Event, target_file: Option<&str>) -> bool {
    paths_contain_target(&event.paths, target_file)
}

/// 纯路径匹配逻辑（脱离 `notify::Event`，便于无 notify API 依赖单测）。
pub fn paths_contain_target(paths: &[PathBuf], target_file: Option<&str>) -> bool {
    let target = match target_file {
        Some(t) => t,
        None => return true,
    };
    paths
        .iter()
        .any(|p| p.file_name().and_then(|s| s.to_str()) == Some(target))
}
