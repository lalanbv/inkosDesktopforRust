//! sync.rs 单元测试外部化（C6 修复：sync.rs 生产代码 < 800 行）。
//!
//! 测试 `process_writeback` / `compute_writeback_diff` / `should_writeback` /
//! `paths_contain_target` / `sync_on_startup` / `spawn_writeback` 的纯逻辑与 IO 行为。
//! 这些函数均 `pub`，通过 `inkos_desktop::secrets::sync::*` 在集成测试中访问。
//!
//! ## 测试分区
//! - sync_on_startup 三路径（keychain→json / json→keychain / 两者都空）
//! - should_writeback / compute_writeback_diff / paths_contain_target 纯逻辑
//! - process_writeback 防回环 + diff 回写
//! - M3 SPA 删除传播（防已删 key 复活）
//! - **C7 单 service 删除复活回归**（本批次新增）
//! - M1 0600 权限恢复（Unix）
//! - spawn_writeback 端到端 IO（`#[ignore]`，依赖 notify timing）

use inkos_desktop::secrets::jsonio::{read_secrets, write_secrets};
use inkos_desktop::secrets::store::{MockStore, SecretStore};
use inkos_desktop::secrets::sync::{
    compute_writeback_diff, event_targets_file, paths_contain_target, process_writeback,
    should_writeback, sync_on_startup,
};

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

// ===========================================================================
// 辅助函数
// ===========================================================================

/// 把原始字符串写入指定路径（构造既有 secrets.json 测试夹具）。
fn put_raw(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
}

/// 读 secrets.json 原始内容（断言用）。
fn read_raw(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// 构造单 (k, v) HashMap（diff 测试夹具）。
fn singleton(k: &str, v: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(k.to_string(), v.to_string());
    m
}

// ===========================================================================
// sync_on_startup — path 1：keychain 非空 → 覆盖 secrets.json
// ===========================================================================

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

// ===========================================================================
// sync_on_startup — path 2：keychain 空 + secrets.json 非空 → 首迁
// ===========================================================================

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

// ===========================================================================
// sync_on_startup — path 3：两者都空 → 无操作
// ===========================================================================

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

// ===========================================================================
// sync_on_startup — 错误路径
// ===========================================================================

#[test]
fn write_failure_propagates_error_and_resets_syncing() {
    // 用一个父目录不存在的路径触发 write_secrets 失败（atomic_write 写 tmp 失败）
    let path = Path::new("/nonexistent/dir/secrets.json");

    let store = MockStore::new();
    store.upsert("openai", "sk-x").unwrap();

    let syncing = AtomicBool::new(false);
    let result = sync_on_startup(&store, path, &syncing);

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

// ===========================================================================
// sync_on_startup — 集成：与 jsonio round-trip 一致
// ===========================================================================

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

// ===========================================================================
// should_writeback — 防回环纯逻辑
// ===========================================================================

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

// ===========================================================================
// compute_writeback_diff — diff 纯逻辑全覆盖
// ===========================================================================

#[test]
fn diff_empty_both_returns_empty() {
    let json = HashMap::new();
    let store = HashMap::new();
    let diff = compute_writeback_diff(&json, &store);
    assert!(diff.upserts.is_empty());
    assert!(diff.deletes.is_empty());
}

#[test]
fn diff_new_key_in_json_is_added() {
    // SPA 新增 openai：store 无 → upsert
    let json = singleton("openai", "sk-new");
    let store = HashMap::new();
    let diff = compute_writeback_diff(&json, &store);
    let mut upserts = diff.upserts;
    upserts.sort();
    assert_eq!(upserts, vec![("openai".to_string(), "sk-new".to_string())]);
    assert!(diff.deletes.is_empty());
}

#[test]
fn diff_changed_value_is_included() {
    // SPA 改 openai 值：store 有但不同 → upsert 覆盖
    let json = singleton("openai", "sk-new");
    let store = singleton("openai", "sk-old");
    let diff = compute_writeback_diff(&json, &store);
    assert_eq!(diff.upserts, vec![("openai".to_string(), "sk-new".to_string())]);
    assert!(diff.deletes.is_empty());
}

#[test]
fn diff_unchanged_value_is_excluded() {
    // 同 key 同值 → 不重复写（节省 keychain 调用）
    let json = singleton("openai", "sk-x");
    let store = singleton("openai", "sk-x");
    let diff = compute_writeback_diff(&json, &store);
    assert!(diff.upserts.is_empty());
    assert!(diff.deletes.is_empty());
}

#[test]
fn diff_store_only_keys_propagate_delete() {
    // M2b 修复：store 比 json 多 → deletes（SPA 删除传播，防已删 key 复活回归）
    let json = HashMap::new();
    let store = singleton("deepseek", "dk-x");
    let diff = compute_writeback_diff(&json, &store);
    assert!(diff.upserts.is_empty());
    assert_eq!(diff.deletes, vec!["deepseek".to_string()]);
}

#[test]
fn diff_mixed_new_changed_unchanged_store_only() {
    // 综合：A 新增 / B 改值 / C 不变 / D 仅 store 有（SPA 删 D）
    let mut json = HashMap::new();
    json.insert("a_new".to_string(), "v1".to_string());
    json.insert("b_changed".to_string(), "v2-new".to_string());
    json.insert("c_same".to_string(), "v3".to_string());
    let mut store = HashMap::new();
    store.insert("b_changed".to_string(), "v2-old".to_string());
    store.insert("c_same".to_string(), "v3".to_string());
    store.insert("d_store_only".to_string(), "v4".to_string());

    let diff = compute_writeback_diff(&json, &store);

    let mut upserts = diff.upserts;
    upserts.sort();
    assert_eq!(
        upserts,
        vec![
            ("a_new".to_string(), "v1".to_string()),
            ("b_changed".to_string(), "v2-new".to_string()),
        ]
    );
    // d_store_only 应出现在 deletes
    assert_eq!(diff.deletes, vec!["d_store_only".to_string()]);
}

#[test]
fn diff_multiple_store_only_keys_all_in_deletes() {
    // SPA 删多个 key：store 比 json 少 2 个 → 都进 deletes
    let mut json = HashMap::new();
    json.insert("keep".to_string(), "v".to_string());
    let mut store = HashMap::new();
    store.insert("keep".to_string(), "v".to_string());
    store.insert("del1".to_string(), "v1".to_string());
    store.insert("del2".to_string(), "v2".to_string());

    let diff = compute_writeback_diff(&json, &store);

    assert!(diff.upserts.is_empty());
    let mut deletes = diff.deletes;
    deletes.sort();
    assert_eq!(deletes, vec!["del1".to_string(), "del2".to_string()]);
}

// ===========================================================================
// paths_contain_target — 事件路径过滤纯逻辑
// ===========================================================================

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

// ===========================================================================
// process_writeback — 防回环 + diff 回写（无 notify timing 依赖）
// ===========================================================================

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
    // → 仅 upsert 这两个差异；同时 json 缺 anthropic 但 store 有 → 进 deletes
    // 但本测试 anthropic 在 store 中、json 中缺，按新语义会删除——为了让旧场景
    // 保持"只验证 upsert"语义，本测试改用 json 也含 anthropic 的形态。
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    fs::write(
        &path,
        r#"{"services":{
            "openai":{"apiKey":"sk-new"},
            "deepseek":{"apiKey":"dk-new"},
            "anthropic":{"apiKey":"ak-keep"}
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
fn process_writeback_corrupt_json_skips_deletes_as_conservative_guard() {
    // C7 修复后的精确语义：损坏 JSON（不可解析）→ state=Corrupt → 兜底跳过删除
    // （防瞬时损坏 / 并发写截断 / 磁盘错乱 → keychain 全部丢失的灾难）。
    // 对比：合法 {"services":{}} → state=Empty → 正常传播 deletes
    // （见 process_writeback_propagates_delete_when_user_clears_all_services）。
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    fs::write(&path, "{ broken }").unwrap();

    let store = MockStore::new();
    store.upsert("preexisting", "v").unwrap();

    process_writeback(&store, &path, &AtomicBool::new(false)).unwrap();

    // store 不变：Corrupt 兜底跳过删除，且 from_json 空 → 无 upsert
    let all = store.read_all().unwrap();
    assert_eq!(all.get("preexisting").unwrap(), "v");
    assert_eq!(all.len(), 1, "损坏 json 不应触发 keychain 误删");
}

// ===========================================================================
// M3 修复：SPA 删除传播（防已删 key 复活回归）
// inkos server.ts `app.delete("/api/v1/services/:service")` 会
// `delete secrets.services[service]; saveSecrets(...)`，watcher 必须
// 把 store 中对应 key 也 delete，否则下次 sync_on_startup path1 复活它。
// ===========================================================================

#[test]
fn process_writeback_propagates_spa_delete_to_store() {
    // SPA 删 openai：json 缺 openai 但 store 有 → process_writeback 应删 store.openai
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    // SPA 删除后 secrets.json 只剩 deepseek（openai 被删）
    fs::write(
        &path,
        r#"{"services":{"deepseek":{"apiKey":"dk-keep"}}}"#,
    )
    .unwrap();

    let store = MockStore::new();
    store.upsert("openai", "sk-stale").unwrap(); // SPA 已删，watcher 应删
    store.upsert("deepseek", "dk-keep").unwrap(); // 保留

    process_writeback(&store, &path, &AtomicBool::new(false)).unwrap();

    let all = store.read_all().unwrap();
    assert!(
        !all.contains_key("openai"),
        "SPA 删 openai 后 watcher 必须删除 keychain.openai；实际: {all:?}"
    );
    assert_eq!(all.get("deepseek").unwrap(), "dk-keep");
    assert_eq!(all.len(), 1);
}

#[test]
fn process_writeback_does_not_revive_deleted_key_after_restart() {
    // M3 核心回归断言：SPA 删 key → watcher 删 store → 重启 sync_on_startup
    // 不复活该 key（path1 从 keychain 覆盖 secrets.json，但 keychain 已无该 key）。
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");

    // stage 0：初始 secrets.json 含 openai + deepseek
    fs::write(
        &path,
        r#"{"services":{
            "openai":{"apiKey":"sk-x"},
            "deepseek":{"apiKey":"dk-x"}
        }}"#,
    )
    .unwrap();

    // stage 1：首迁到 keychain（keychain 空 + secrets.json 非空 → path2）
    let store = MockStore::new();
    sync_on_startup(&store, &path, &AtomicBool::new(false)).unwrap();
    assert_eq!(store.read_all().unwrap().len(), 2);

    // stage 2：SPA 删除 openai（写回只含 deepseek 的 secrets.json）
    fs::write(
        &path,
        r#"{"services":{"deepseek":{"apiKey":"dk-x"}}}"#,
    )
    .unwrap();

    // stage 3：watcher 触发 process_writeback → 删除 keychain.openai
    process_writeback(&store, &path, &AtomicBool::new(false)).unwrap();

    let after_delete = store.read_all().unwrap();
    assert!(!after_delete.contains_key("openai"), "watcher 应删除 openai");
    assert_eq!(after_delete.len(), 1);

    // stage 4：模拟重启——sync_on_startup 再次执行。
    // 关键断言：path1 从 keychain 读，keychain 已无 openai → secrets.json 不再有 openai。
    // 若 process_writeback 没删 keychain，path1 会再次把 openai 写回 secrets.json（回归）。
    let syncing_restart = AtomicBool::new(false);
    sync_on_startup(&store, &path, &syncing_restart).unwrap();

    let final_json = read_secrets(&path).unwrap();
    assert!(
        !final_json.contains_key("openai"),
        "重启后 openai 不应复活（M3 回归断言）；实际: {final_json:?}"
    );
    assert_eq!(final_json.get("deepseek").unwrap(), "dk-x");
}

#[test]
fn process_writeback_anti_loop_programmatic_write_skips_delete() {
    // 防回环：syncing=true 时即便 json 完全缺 key 也不删除 store。
    // sync_on_startup 写 secrets.json 期间 syncing=true，watcher 必须整体跳过——
    // 否则 sync 写入 = "json 暂时空" → watcher 删 keychain 全部 key = 灾难。
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    // syncing=true 期间 sync_on_startup 写入新内容（假设从 keychain 读，写 openai）
    // 但 watcher 看到的瞬间可能是空 / 旧值——必须不删。
    fs::write(&path, r#"{"services":{}}"#).unwrap();

    let store = MockStore::new();
    store.upsert("openai", "sk-x").unwrap();
    store.upsert("deepseek", "dk-x").unwrap();

    process_writeback(&store, &path, &AtomicBool::new(true)).unwrap();

    let all = store.read_all().unwrap();
    assert_eq!(all.len(), 2, "syncing=true 必须 skip 整体（不删不写）");
    assert!(all.contains_key("openai"));
    assert!(all.contains_key("deepseek"));
}

// ===========================================================================
// C7 回归测（M3 批次 3 新增）：单 service 删除复活防护
// 旧版 skip_deletes 兜底条件 from_json.is_empty() 把"合法清空"与"损坏"折叠为
// 同一空 map → 用户删唯一 service 写出合法 {"services":{}} 被误判为"损坏"跳过
// 删除 → keychain 残留 → 重启 sync_on_startup path1 复活已删 key。
// C7 修复：read_secrets_state tri-state 区分 Corrupt / Empty / Absent，兜底
// 仅对 Corrupt 触发；Empty 视为用户合法意图 → 正常传播 deletes。
// ===========================================================================

#[test]
fn process_writeback_propagates_delete_when_user_clears_all_services() {
    // C7 核心回归：用户删唯一 service → 写出合法 {"services":{}}（state=Empty，
    // 非Corrupt）→ watcher 必须传播 delete（不误判为损坏跳过）→ 重启 sync 不复活。
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");

    // stage 0：初始 secrets.json 含唯一 service openai
    fs::write(
        &path,
        r#"{"services":{"openai":{"apiKey":"sk-x"}}}"#,
    )
    .unwrap();

    // stage 1：首迁到 keychain（keychain 空 + secrets.json 非空 → path2）
    let store = MockStore::new();
    sync_on_startup(&store, &path, &AtomicBool::new(false)).unwrap();
    assert_eq!(store.read_all().unwrap().len(), 1);
    assert!(store.read_all().unwrap().contains_key("openai"));

    // stage 2：用户删 openai（前端 delete secrets.services.openai; saveSecrets）
    // 写出合法的 {"services":{}} —— 这是 C7 修复的关键场景。
    fs::write(&path, r#"{"services":{}}"#).unwrap();

    // stage 3：watcher 触发 process_writeback
    process_writeback(&store, &path, &AtomicBool::new(false)).unwrap();

    // C7 关键断言：keychain 中 openai 被删除（合法清空 → 传播 deletes）
    // 旧版兜底误判为"损坏"会跳过删除 → 这里断言不会被跳过。
    let after_clear = store.read_all().unwrap();
    assert!(
        after_clear.is_empty(),
        "C7：合法 {{services:{{}}}} (Empty) 必须传播 delete；旧版兜底误判为 Corrupt 跳过 → keychain 应被清空；实际: {after_clear:?}"
    );

    // stage 4：模拟重启——sync_on_startup 再次执行。
    // 关键断言：path1 不触发（keychain 空）；path2 不触发（secrets.json 也空）→ 无操作。
    // keychain 仍空（不复活 openai）。
    let syncing_restart = AtomicBool::new(false);
    sync_on_startup(&store, &path, &syncing_restart).unwrap();

    // 双重断言：keychain 仍空 + secrets.json 仍为合法 empty（不被 path1 写回 openai）
    assert!(
        store.read_all().unwrap().is_empty(),
        "C7：重启后 keychain 不应复活已删 key"
    );
    let final_raw = read_raw(&path);
    assert!(
        !final_raw.contains("openai"),
        "C7：重启后 secrets.json 不应再含 openai（不应被 path1 复活）；实际: {final_raw}"
    );
    assert!(
        final_raw.contains("\"services\""),
        "secrets.json 仍应含合法 services 字段"
    );
}

// ===========================================================================
// M1 修复：SPA 写后恢复 secrets.json 权限到 0600（Unix）
// inkos saveSecrets 用默认 umask 写（通常 0644），桌面壳作为特权方恢复 0600。
// ===========================================================================

#[test]
#[cfg(unix)]
fn process_writeback_restores_0600_after_spa_write() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    // 模拟 inkos saveSecrets：用默认 umask 写（此处显式 0644 模拟最差情况）
    fs::write(&path, r#"{"services":{"openai":{"apiKey":"sk-x"}}}"#).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    // 验证夹具：确实是 0644
    let mode_before = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode_before, 0o644);

    let store = MockStore::new();
    process_writeback(&store, &path, &AtomicBool::new(false)).unwrap();

    // 核心断言：process_writeback 后权限恢复为 0600
    let mode_after = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode_after, 0o600, "SPA 写后桌面壳必须恢复 secrets.json 到 0600");
}

#[test]
#[cfg(unix)]
fn atomic_write_0600_creates_file_with_0600_mode() {
    // M2 加固验证：即便父目录有历史固定名 tmp 风险，NamedTempFile 创建即 0600。
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    let mut m = HashMap::new();
    m.insert("openai".to_string(), "sk-x".to_string());

    // 设置宽松 umask（022）模拟常见 shell 环境——但 tempfile::Builder::permissions
    // 应该覆盖 umask 影响（创建即 0600）。
    write_secrets(&path, &m).unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "atomic_write_0600 必须创建 0600 文件");
}

// ===========================================================================
// spawn_writeback 端到端 IO 测已拆到 tests/sync_watcher_io.rs（依赖 notify
// timing，全部 #[ignore]；C6 文件行数 < 800 约束）
// ===========================================================================
