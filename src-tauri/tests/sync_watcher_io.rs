//! spawn_writeback 端到端 IO 测（依赖 notify timing，全部 `#[ignore]`）。
//!
//! 从 `tests/sync_unit.rs` 拆出（C6 修复：sync_unit.rs 文件 < 800 行）。
//! 这些测依赖真实 notify watcher + debounce timing，CI / headless 环境可能 flaky，
//! 故默认不跑。手动验证：
//!
//! ```bash
//! cd src-tauri && cargo test --test sync_watcher_io -- --ignored
//! ```
//!
//! macOS FSEvents 抽样间隔较长，需要较大的 settle 余量。

use inkos_desktop::secrets::store::{MockStore, SecretStore};
use inkos_desktop::secrets::sync::{spawn_writeback, DEBOUNCE_MS, POLL_TIMEOUT_MS};

use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// 等待 watcher 处理：notify 注册 + debounce(DEBOUNCE_MS) + 处理 + 余量。
/// 默认 2 × DEBOUNCE_MS + 500ms 余量（macOS FSEvents 抽样间隔较长需更多余量）。
const WATCHER_SETTLE_MS: u64 = 2 * DEBOUNCE_MS + 800;

/// 启动前小 sleep：让 notify watcher 注册完成（避免错过最早的事件）。
const WATCHER_REGISTER_MS: u64 = 200;

#[test]
#[ignore = "依赖 notify timing；跑：cargo test --test sync_watcher_io -- --ignored watcher_anti_loop_programmatic"]
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
#[ignore = "依赖 notify timing；跑：cargo test --test sync_watcher_io -- --ignored watcher_spa_write_writebacks"]
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
#[ignore = "依赖 notify timing；跑：cargo test --test sync_watcher_io -- --ignored watcher_shutdown_exits_cleanly"]
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
