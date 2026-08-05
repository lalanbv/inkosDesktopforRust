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
//!   → 防回环检查 → diff 回写 keychain（SPA 改/新增 key 同步到 keychain）。
//!
//! ## 防回环
//! `sync_on_startup` 写 secrets.json 前设 `syncing=true`，写后 `false`。
//! watcher 处理前查 `syncing`：true → 跳过（programmatic 写不回写）；
//! SPA（前端/用户改 secrets.json）触发时 syncing=false → 正常 diff 回写。
//!
//! ## debounce
//! notify 收到首个事件后进入 500ms 静默窗：窗内每收一个事件重置 deadline，
//! 静默满 500ms 后才触发处理（聚合连续写，避免抖动 + 节省 keychain 调用）。
//!
//! ## 错误处理
//! - `read_all` / `read_secrets` / `write_secrets` / `upsert` 失败 → 显式向上传播
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

use crate::secrets::jsonio::{read_secrets, write_secrets};
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
const POLL_TIMEOUT_MS: u64 = 100;

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

/// 处理一次回写：防回环 → read json → diff vs store → upsert 差异。
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
    let from_json = read_secrets(path)?;
    let from_store = store.read_all()?;
    let diff = compute_writeback_diff(&from_json, &from_store);
    for (key, value) in diff {
        store.upsert(&key, &value)?;
    }
    Ok(())
}

/// 防回环判定：`syncing=true`（programmatic write 进行中）→ 跳过（返回 false）。
///
/// 抽成纯函数便于单测全覆盖（防回环是本任务核心正确性保证）。
pub fn should_writeback(syncing: &AtomicBool) -> bool {
    !syncing.load(Ordering::SeqCst)
}

/// diff 计算：返回 `from_json` 中"store 缺失或值不同"的 `(key, value)` 列表。
///
/// **保守策略**：仅 upsert 新增/变更 key；**不删除** store 中 `from_json` 没有的 key
/// （json 缺 key 不视为删除意图——避免 SPA 临时缺失或解析不全误删 keychain 项；
/// 删除路径由 store.delete 显式调用，不经 watcher）。
///
/// 抽成纯函数便于单测全覆盖（diff 是 SPA → keychain 回写的核心语义）。
pub fn compute_writeback_diff(
    from_json: &HashMap<String, String>,
    from_store: &HashMap<String, String>,
) -> Vec<(String, String)> {
    from_json
        .iter()
        .filter(|(k, v)| match from_store.get(*k) {
            None => true,
            Some(existing) => existing != *v,
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::store::MockStore;
    use std::fs;

    /// 辅助：把原始字符串写入指定路径（用于构造既有 secrets.json 测试夹具）。
    fn put_raw(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
    }

    /// 辅助：读 secrets.json 原始内容（断言用）。
    fn read_raw(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    // ---------- path 1：keychain 非空 → 覆盖 secrets.json ----------

    #[test]
    fn keychain_nonempty_writes_to_secrets_json_and_resets_syncing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        // 注意：secrets.json 初始不存在——write_secrets 会创建

        let store = MockStore::new();
        store.upsert("openai", "sk-x").unwrap();

        let syncing = AtomicBool::new(false);
        sync_on_startup(&store, &path, &syncing).unwrap();

        // secrets.json 含 openai
        let got = read_secrets(&path).unwrap();
        assert_eq!(got.get("openai").unwrap(), "sk-x");
        assert_eq!(got.len(), 1);

        // SYNCING 在 write 后复位为 false
        assert!(
            !syncing.load(Ordering::SeqCst),
            "syncing must reset to false after write"
        );

        // keychain 未被修改（source 不变）
        let chain = store.read_all().unwrap();
        assert_eq!(chain.get("openai").unwrap(), "sk-x");
    }

    #[test]
    fn keychain_nonempty_overwrites_existing_json_with_keychain_as_source_of_truth() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        // 既有 secrets.json 含 deepseek（将不出现于结果，因 keychain 主、覆盖语义由
        // write_secrets 的 merge-upsert 实现：keychain 中不存在的 key 会保留——
        // 但本测试 keychain 只放 openai，write_secrets 会保留 deepseek；
        // 真正"覆盖"语义指 keychain 中有的 key 以 keychain 为准）
        put_raw(
            &path,
            r#"{"services": {"deepseek": { "apiKey": "dk-old" }}}"#,
        );

        let store = MockStore::new();
        store.upsert("openai", "sk-x").unwrap();

        let syncing = AtomicBool::new(false);
        sync_on_startup(&store, &path, &syncing).unwrap();

        let got = read_secrets(&path).unwrap();
        // keychain 中的 openai 写入
        assert_eq!(got.get("openai").unwrap(), "sk-x");
        // 既有 deepseek 因 write_secrets merge-upsert 语义保留（jsonio 层契约）
        assert_eq!(got.get("deepseek").unwrap(), "dk-old");
    }

    #[test]
    fn keychain_nonempty_multiple_keys_all_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");

        let store = MockStore::new();
        store.upsert("openai", "sk-1").unwrap();
        store.upsert("deepseek", "dk-2").unwrap();
        store.upsert("anthropic", "ak-3").unwrap();

        let syncing = AtomicBool::new(false);
        sync_on_startup(&store, &path, &syncing).unwrap();

        let got = read_secrets(&path).unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got.get("openai").unwrap(), "sk-1");
        assert_eq!(got.get("deepseek").unwrap(), "dk-2");
        assert_eq!(got.get("anthropic").unwrap(), "ak-3");
    }

    // ---------- path 2：keychain 空 + secrets.json 非空 → 首迁 ----------

    #[test]
    fn keychain_empty_migrates_from_existing_secrets_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        put_raw(
            &path,
            r#"{"services": {"deepseek": { "apiKey": "dk-x" }}}"#,
        );

        let store = MockStore::new();
        let syncing = AtomicBool::new(false);
        sync_on_startup(&store, &path, &syncing).unwrap();

        // MockStore 含 deepseek（首迁成功）
        let chain = store.read_all().unwrap();
        assert_eq!(chain.get("deepseek").unwrap(), "dk-x");
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn first_migration_multiple_keys_all_upserted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        put_raw(
            &path,
            r#"{"services": {
                "openai": { "apiKey": "sk-1" },
                "deepseek": { "apiKey": "dk-2" },
                "anthropic": { "apiKey": "ak-3" }
            }}"#,
        );

        let store = MockStore::new();
        sync_on_startup(&store, &path, &AtomicBool::new(false)).unwrap();

        let chain = store.read_all().unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain.get("openai").unwrap(), "sk-1");
        assert_eq!(chain.get("deepseek").unwrap(), "dk-2");
        assert_eq!(chain.get("anthropic").unwrap(), "ak-3");
    }

    #[test]
    fn first_migration_corrupt_json_is_noop_keychain_stays_empty() {
        // 损坏 JSON → read_secrets 返回 Ok(empty)（jsonio 契约）→ 两者都空 → 无操作
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        put_raw(&path, "{ not valid json }}}}");

        let store = MockStore::new();
        sync_on_startup(&store, &path, &AtomicBool::new(false)).unwrap();

        // keychain 仍空
        assert!(store.read_all().unwrap().is_empty());
    }

    // ---------- path 3：两者都空 → 无操作 ----------

    #[test]
    fn both_empty_is_noop_file_not_created_store_stays_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        // secrets.json 不存在；keychain 空

        let store = MockStore::new();
        sync_on_startup(&store, &path, &AtomicBool::new(false)).unwrap();

        // secrets.json 未被创建
        assert!(
            !path.exists(),
            "both-empty case must not create secrets.json"
        );
        // keychain 仍空
        assert!(store.read_all().unwrap().is_empty());
    }

    #[test]
    fn both_empty_with_existing_empty_json_is_noop() {
        // secrets.json 存在但 services 为空 → read_secrets 返回 empty → 无操作
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        put_raw(&path, r#"{"services": {}}"#);

        let store = MockStore::new();
        sync_on_startup(&store, &path, &AtomicBool::new(false)).unwrap();

        // keychain 仍空
        assert!(store.read_all().unwrap().is_empty());
        // 文件内容不变（无 write 调用）
        let raw = read_raw(&path);
        assert!(raw.contains("\"services\""));
    }

    // ---------- 错误路径 ----------

    #[test]
    fn write_failure_propagates_error_and_resets_syncing() {
        // 用一个父目录不存在的路径触发 write_secrets 失败（atomic_write 写 tmp 失败）
        let path = std::path::Path::new("/nonexistent/dir/secrets.json");

        let store = MockStore::new();
        store.upsert("openai", "sk-x").unwrap();

        let syncing = AtomicBool::new(false);
        let result = sync_on_startup(&store, &path, &syncing);

        // 错误显式传播（不静默吞错）
        assert!(result.is_err(), "write failure must propagate as Err");
        // SYNCING 即便失败也复位（避免 watcher 永久禁用）
        assert!(
            !syncing.load(Ordering::SeqCst),
            "syncing must reset to false even when write fails"
        );
    }

    #[test]
    fn syncing_flag_not_touched_in_first_migration_path() {
        // path 2 不操作 syncing（无文件写入，watcher 不会触发）
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        put_raw(
            &path,
            r#"{"services": {"deepseek": { "apiKey": "dk-x" }}}"#,
        );

        let store = MockStore::new();
        let syncing = AtomicBool::new(true); // 初始故意设 true
        sync_on_startup(&store, &path, &syncing).unwrap();

        // syncing 不被 path 2 触碰——仍是 true（调用方语义：仅 path 1 管理 syncing）
        assert!(
            syncing.load(Ordering::SeqCst),
            "syncing flag must not be touched by first-migration path"
        );
    }

    // ---------- 集成：与 jsonio round-trip 一致 ----------

    #[test]
    fn sync_roundtrip_keychain_to_json_and_back() {
        // 端到端：keychain → secrets.json；清空 MockStore；从 secrets.json 首迁回 keychain。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");

        // stage 1：keychain → json
        let store_src = MockStore::new();
        store_src.upsert("openai", "sk-rt").unwrap();
        store_src.upsert("deepseek", "dk-rt").unwrap();
        sync_on_startup(&store_src, &path, &AtomicBool::new(false)).unwrap();

        // stage 2：清空 keychain 后从 json 首迁
        let store_dst = MockStore::new();
        sync_on_startup(&store_dst, &path, &AtomicBool::new(false)).unwrap();

        let chain = store_dst.read_all().unwrap();
        assert_eq!(chain.get("openai").unwrap(), "sk-rt");
        assert_eq!(chain.get("deepseek").unwrap(), "dk-rt");
        assert_eq!(chain.len(), 2);
    }

    // ===================================================================
    // Task 4: spawn_writeback — 防回环纯逻辑
    // ===================================================================

    #[test]
    fn should_writeback_returns_false_when_syncing_true() {
        // programmatic write（sync_on_startup 写 secrets.json 中）→ 跳过回写
        let syncing = AtomicBool::new(true);
        assert!(!should_writeback(&syncing));
    }

    #[test]
    fn should_writeback_returns_true_when_syncing_false() {
        // SPA 改（无 programmatic 写进行）→ 允许回写
        let syncing = AtomicBool::new(false);
        assert!(should_writeback(&syncing));
    }

    // ===================================================================
    // compute_writeback_diff — diff 纯逻辑全覆盖
    // ===================================================================

    fn singleton(k: &str, v: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert(k.to_string(), v.to_string());
        m
    }

    #[test]
    fn diff_empty_both_returns_empty() {
        let json = HashMap::new();
        let store = HashMap::new();
        assert!(compute_writeback_diff(&json, &store).is_empty());
    }

    #[test]
    fn diff_new_key_in_json_is_added() {
        // SPA 新增 openai：store 无 → 回写
        let json = singleton("openai", "sk-new");
        let store = HashMap::new();
        let mut diff = compute_writeback_diff(&json, &store);
        diff.sort();
        assert_eq!(diff, vec![("openai".to_string(), "sk-new".to_string())]);
    }

    #[test]
    fn diff_changed_value_is_included() {
        // SPA 改 openai 值：store 有但不同 → 回写覆盖
        let json = singleton("openai", "sk-new");
        let store = singleton("openai", "sk-old");
        let diff = compute_writeback_diff(&json, &store);
        assert_eq!(diff, vec![("openai".to_string(), "sk-new".to_string())]);
    }

    #[test]
    fn diff_unchanged_value_is_excluded() {
        // 同 key 同值 → 不重复写（节省 keychain 调用）
        let json = singleton("openai", "sk-x");
        let store = singleton("openai", "sk-x");
        assert!(compute_writeback_diff(&json, &store).is_empty());
    }

    #[test]
    fn diff_store_only_keys_not_removed() {
        // 保守策略：store 比 json 多 → 不删除（删除需显式 store.delete，不经 watcher）
        let json = HashMap::new();
        let store = singleton("deepseek", "dk-x");
        assert!(compute_writeback_diff(&json, &store).is_empty());
    }

    #[test]
    fn diff_mixed_new_changed_unchanged_only_returns_diff() {
        // 综合：A 新增 / B 改值 / C 不变 / D 仅 store 有
        let mut json = HashMap::new();
        json.insert("a_new".to_string(), "v1".to_string());
        json.insert("b_changed".to_string(), "v2-new".to_string());
        json.insert("c_same".to_string(), "v3".to_string());
        let mut store = HashMap::new();
        store.insert("b_changed".to_string(), "v2-old".to_string());
        store.insert("c_same".to_string(), "v3".to_string());
        store.insert("d_store_only".to_string(), "v4".to_string());

        let mut diff = compute_writeback_diff(&json, &store);
        diff.sort();
        assert_eq!(
            diff,
            vec![
                ("a_new".to_string(), "v1".to_string()),
                ("b_changed".to_string(), "v2-new".to_string()),
            ]
        );
    }

    // ===================================================================
    // paths_contain_target — 事件路径过滤纯逻辑
    // ===================================================================

    #[test]
    fn paths_contain_target_match_returns_true() {
        let paths = vec![PathBuf::from("/tmp/secrets.json")];
        assert!(paths_contain_target(&paths, Some("secrets.json")));
    }

    #[test]
    fn paths_contain_target_no_match_returns_false() {
        let paths = vec![PathBuf::from("/tmp/other.txt")];
        assert!(!paths_contain_target(&paths, Some("secrets.json")));
    }

    #[test]
    fn paths_contain_target_none_target_accepts_all() {
        // 无目标名 → 保守接受（不丢事件）
        let paths = vec![PathBuf::from("/tmp/anything")];
        assert!(paths_contain_target(&paths, None));
    }

    #[test]
    fn paths_contain_target_empty_paths_returns_false() {
        let paths: Vec<PathBuf> = vec![];
        assert!(!paths_contain_target(&paths, Some("secrets.json")));
    }

    #[test]
    fn paths_contain_target_multiple_paths_any_match() {
        // atomic_write：tmp + rename 触发 paths=[tmp, secrets.json]，至少一个匹配
        let paths = vec![
            PathBuf::from("/tmp/.secrets.json.tmp"),
            PathBuf::from("/tmp/secrets.json"),
        ];
        assert!(paths_contain_target(&paths, Some("secrets.json")));
    }

    #[test]
    fn event_targets_file_wraps_paths_correctly() {
        // 集成 notify::Event：验证 event_targets_file 与 paths_contain_target 一致
        let event = notify::Event {
            kind: notify::EventKind::Any,
            paths: vec![PathBuf::from("/x/secrets.json")],
            attrs: notify::event::EventAttributes::new(),
        };
        assert!(event_targets_file(&event, Some("secrets.json")));
        assert!(!event_targets_file(&event, Some("other.json")));
    }

    // ===================================================================
    // process_writeback — 防回环 + diff 回写（无 notify timing 依赖）
    // ===================================================================

    #[test]
    fn process_writeback_skips_when_syncing_true_anti_loop() {
        // 防回环核心：syncing=true 时即便 secrets.json 含新 key 也不写 store
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        fs::write(
            &path,
            r#"{"services":{"openai":{"apiKey":"sk-x"}}}"#,
        )
        .unwrap();

        let store = MockStore::new();
        let syncing = AtomicBool::new(true); // programmatic write 中
        process_writeback(&store, &path, &syncing).unwrap();

        // store 未变（防回环）
        assert!(
            store.read_all().unwrap().is_empty(),
            "syncing=true must NOT trigger keychain writeback (anti-loop)"
        );
    }

    #[test]
    fn process_writeback_diffs_upserts_when_syncing_false() {
        // SPA 改：syncing=false → json 中新 key 回写 store
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        fs::write(
            &path,
            r#"{"services":{"openai":{"apiKey":"sk-new"}}}"#,
        )
        .unwrap();

        let store = MockStore::new();
        let syncing = AtomicBool::new(false); // SPA 改
        process_writeback(&store, &path, &syncing).unwrap();

        let all = store.read_all().unwrap();
        assert_eq!(all.get("openai").unwrap(), "sk-new");
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn process_writeback_only_upserts_changed_keys() {
        // store 已有 openai=sk-x，json 改 openai=sk-new + 新增 deepseek
        // → 仅 upsert 这两个差异，未触及不变项
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        fs::write(
            &path,
            r#"{"services":{
                "openai":{"apiKey":"sk-new"},
                "deepseek":{"apiKey":"dk-new"}
            }}"#,
        )
        .unwrap();

        let store = MockStore::new();
        store.upsert("openai", "sk-old").unwrap(); // 将被覆盖
        store.upsert("anthropic", "ak-keep").unwrap(); // 不变项保留

        process_writeback(&store, &path, &AtomicBool::new(false)).unwrap();

        let all = store.read_all().unwrap();
        assert_eq!(all.get("openai").unwrap(), "sk-new"); // 覆盖
        assert_eq!(all.get("deepseek").unwrap(), "dk-new"); // 新增
        assert_eq!(all.get("anthropic").unwrap(), "ak-keep"); // 保留
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn process_writeback_corrupt_json_is_noop() {
        // 损坏 JSON → read_secrets 返回 Ok(empty) → diff 空 → 无 upsert
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        fs::write(&path, "{ broken }").unwrap();

        let store = MockStore::new();
        store.upsert("preexisting", "v").unwrap();

        process_writeback(&store, &path, &AtomicBool::new(false)).unwrap();

        // store 不变（无新 upsert，已存在的 preexisting 保留）
        let all = store.read_all().unwrap();
        assert_eq!(all.get("preexisting").unwrap(), "v");
        assert_eq!(all.len(), 1);
    }

    // ===================================================================
    // spawn_writeback 端到端 IO 测（依赖 notify timing，标 #[ignore]）
    // 跑：cargo test -- --ignored spawn_writeback
    // ===================================================================

    /// 等待 watcher 处理：notify 注册 + debounce(DEBOUNCE_MS) + 处理 + 余量。
    /// 默认 2 × DEBOUNCE_MS + 500ms 余量（macOS FSEvents 抽样间隔较长需更多余量）。
    const WATCHER_SETTLE_MS: u64 = 2 * DEBOUNCE_MS + 800;

    /// 启动前小 sleep：让 notify watcher 注册完成（避免错过最早的事件）。
    const WATCHER_REGISTER_MS: u64 = 200;

    #[test]
    #[ignore = "依赖 notify timing；跑：cargo test -- --ignored watcher_anti_loop_programmatic"]
    fn watcher_anti_loop_programmatic_write_does_not_writeback() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("secrets.json"));
        fs::write(&*path, r#"{"services":{}}"#).unwrap();

        let store: Arc<dyn SecretStore> = Arc::new(MockStore::new());
        // 模拟 sync_on_startup 写 secrets.json 前后：syncing=true
        let syncing = Arc::new(AtomicBool::new(true));
        let shutdown = Arc::new(AtomicBool::new(false));

        let handle = spawn_writeback(
            Arc::clone(&store),
            Arc::clone(&path),
            Arc::clone(&syncing),
            Arc::clone(&shutdown),
        );

        // 等 watcher 注册 → 写 secrets.json（syncing=true 防回环）
        thread::sleep(Duration::from_millis(WATCHER_REGISTER_MS));
        fs::write(
            &*path,
            r#"{"services":{"openai":{"apiKey":"sk-x"}}}"#,
        )
        .unwrap();

        // 等 watcher 处理（debounce + 处理 + 余量）
        thread::sleep(Duration::from_millis(WATCHER_SETTLE_MS));

        shutdown.store(true, Ordering::SeqCst);
        handle.join().expect("watcher thread must not panic");

        // 防回环断言：syncing=true 期间 programmatic 写 secrets.json → store 未变
        let all = store.read_all().unwrap();
        assert!(
            all.is_empty(),
            "programmatic write must NOT trigger keychain writeback (anti-loop); got {all:?}"
        );
    }

    #[test]
    #[ignore = "依赖 notify timing；跑：cargo test -- --ignored watcher_spa_write_writebacks"]
    fn watcher_spa_write_writebacks_to_keychain() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("secrets.json"));
        fs::write(&*path, r#"{"services":{}}"#).unwrap();

        let store: Arc<dyn SecretStore> = Arc::new(MockStore::new());
        let syncing = Arc::new(AtomicBool::new(false)); // SPA 改
        let shutdown = Arc::new(AtomicBool::new(false));

        let handle = spawn_writeback(
            Arc::clone(&store),
            Arc::clone(&path),
            Arc::clone(&syncing),
            Arc::clone(&shutdown),
        );

        thread::sleep(Duration::from_millis(WATCHER_REGISTER_MS));
        // SPA 改：用户/前端新增 openai key 到 secrets.json
        fs::write(
            &*path,
            r#"{"services":{"openai":{"apiKey":"sk-new"}}}"#,
        )
        .unwrap();

        thread::sleep(Duration::from_millis(WATCHER_SETTLE_MS));

        shutdown.store(true, Ordering::SeqCst);
        handle.join().expect("watcher thread must not panic");

        // 回写生效断言：store 含 SPA 新增的 openai=sk-new
        let all = store.read_all().unwrap();
        assert_eq!(
            all.get("openai"),
            Some(&"sk-new".to_string()),
            "SPA write must writeback openai=sk-new to keychain; got {all:?}"
        );
    }

    #[test]
    #[ignore = "依赖 notify timing；跑：cargo test -- --ignored watcher_shutdown_exits_cleanly"]
    fn watcher_shutdown_flag_exits_thread_cleanly() {
        // shutdown=true → 线程在 POLL_TIMEOUT_MS 内退出，join 不阻塞
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("secrets.json"));
        fs::write(&*path, r#"{"services":{}}"#).unwrap();

        let store: Arc<dyn SecretStore> = Arc::new(MockStore::new());
        let syncing = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(true)); // 启动即退出

        let handle = spawn_writeback(store, path, syncing, shutdown);

        // 给线程一次进入循环 + 检查 shutdown 的机会
        thread::sleep(Duration::from_millis(POLL_TIMEOUT_MS * 3));
        handle
            .join()
            .expect("shutdown=true must let watcher thread exit cleanly");
    }
}
