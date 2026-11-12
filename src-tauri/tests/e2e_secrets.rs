//! E2E Secrets 契约测试
//!
//! 验证跨平台 keychain 集成：upsert + read_all + delete。
//!
//! 运行：cargo test --test e2e_secrets -- --ignored --test-threads=1
//!（或 pnpm cargo:ignored-sweep；debug/release 均可）
//!
//! ## 990 号修复：固定 service 名 → unique_service（ACL 授权框根因）
//!
//! 旧版每测固定 service 名（`inkos-e2e-*`）：keychain 条目跨运行持久，而 macOS
//! keychain ACL 按「创建时二进制签名」授权——重构建后的测试二进制（cdhash 变化）
//! 访问旧二进制创建的持久条目 = 跨签名访问 → securityd 弹授权框 → 无人值守挂死。
//! 这才是旧 ignore 理由「首次访问会弹授权框」的真实机制（dark wake/keychain 锁定
//! 是另一重环境门控，与弹框无关）。
//!
//! 修复：对齐 `src/secrets/store.rs` lib 测试的 `unique_service` 形（PID+纳秒唯一，
//! 同进程内创建+访问 = 无跨签名面，永不弹框），收尾 `cleanup` 清 `__index__` 防
//! 残留。`#[ignore]` 保留：CI/headless 无 keychain、keychain 锁定（dark wake）
//! 时 securityd 调用仍会阻塞——环境门控语义不变，只是弹框根因已除。

use inkos_desktop::secrets::store::{KeyringStore, SecretStore};

/// 生成唯一 service 名（每次测试运行独立 keychain namespace）：
/// - 同进程创建+访问 ⇒ 无跨签名 ACL 授权框（990 号根因修复）；
/// - 并行安全：不再依赖「每测固定名」防 `__index__` 互踩。
fn unique_service(label: &str) -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("__test__inkosDesktop_e2e_{label}_{pid}_{nanos}")
}

/// 清理 service 下全部业务条目（走公开 trait 面；`__index__` 自身条目残留——
/// 每次 e2e 运行留一枚空 index 条目于唯一命名空间，无害，与 lib 测试的模块内
/// 深度清理差异在此备案）。
fn cleanup(store: &KeyringStore) {
    if let Ok(all) = store.read_all() {
        for k in all.keys() {
            let _ = store.delete(k);
        }
    }
}

#[test]
#[ignore = "需真实 OS keychain 可达（桌面解锁会话）：keychain 锁定（dark wake）时 securityd 调用会阻塞；unique_service 已消除跨构建 ACL 弹框（990 号）"]
fn test_keychain_upsert() {
    // 每个测试用独立 service：同名 service 共享一个 `__index__` entry，
    // 并行跑时彼此覆盖索引，导致 read_all 漏读。
    let store = KeyringStore::new(&unique_service("upsert"));
    let result = store.upsert("test-key", "test-value");
    assert!(result.is_ok(), "keychain upsert 应该成功");

    // 清理
    let _ = store.delete("test-key");
    cleanup(&store);
}

#[test]
#[ignore = "需真实 OS keychain 可达（桌面解锁会话）：keychain 锁定（dark wake）时 securityd 调用会阻塞；unique_service 已消除跨构建 ACL 弹框（990 号）"]
fn test_keychain_read_all() {
    let store = KeyringStore::new(&unique_service("readall"));
    store.upsert("test-key", "test-value").unwrap();

    let all = store.read_all().unwrap();
    assert_eq!(all.get("test-key"), Some(&"test-value".to_string()));

    // 清理
    store.delete("test-key").unwrap();
    cleanup(&store);
}

#[test]
#[ignore = "需真实 OS keychain 可达（桌面解锁会话）：keychain 锁定（dark wake）时 securityd 调用会阻塞；unique_service 已消除跨构建 ACL 弹框（990 号）"]
fn test_keychain_delete() {
    let store = KeyringStore::new(&unique_service("delete"));
    store.upsert("test-key", "test-value").unwrap();

    let result = store.delete("test-key");
    assert!(result.is_ok());

    // 验证已删除
    let all = store.read_all().unwrap();
    assert!(!all.contains_key("test-key"), "删除后不应存在");
    cleanup(&store);
}

#[test]
#[ignore = "需真实 OS keychain 可达（桌面解锁会话）：keychain 锁定（dark wake）时 securityd 调用会阻塞；unique_service 已消除跨构建 ACL 弹框（990 号）"]
fn test_keychain_read_empty() {
    let store = KeyringStore::new(&unique_service("emptytest"));

    let all = store.read_all().unwrap();
    assert!(all.is_empty(), "空 keychain 应返回空 HashMap");
    cleanup(&store);
}

#[test]
#[ignore = "需真实 OS keychain 可达（桌面解锁会话）：keychain 锁定（dark wake）时 securityd 调用会阻塞；unique_service 已消除跨构建 ACL 弹框（990 号）"]
fn test_keychain_overwrite() {
    let store = KeyringStore::new(&unique_service("overwrite"));

    // 第一次写入
    store.upsert("test-key", "value1").unwrap();
    let all = store.read_all().unwrap();
    assert_eq!(all.get("test-key"), Some(&"value1".to_string()));

    // 覆盖写入
    store.upsert("test-key", "value2").unwrap();
    let all = store.read_all().unwrap();
    assert_eq!(all.get("test-key"), Some(&"value2".to_string()));

    // 清理
    store.delete("test-key").unwrap();
    cleanup(&store);
}

#[test]
#[ignore = "需真实 OS keychain 可达（桌面解锁会话）：keychain 锁定（dark wake）时 securityd 调用会阻塞；unique_service 已消除跨构建 ACL 弹框（990 号）"]
fn test_secrets_full_cycle() {
    let store = KeyringStore::new(&unique_service("full"));

    // 写入
    store.upsert("api-key", "sk-test-123").unwrap();

    // 读取
    let all = store.read_all().unwrap();
    assert_eq!(all.get("api-key"), Some(&"sk-test-123".to_string()));

    // 更新
    store.upsert("api-key", "sk-test-456").unwrap();
    let all = store.read_all().unwrap();
    assert_eq!(all.get("api-key"), Some(&"sk-test-456".to_string()));

    // 删除
    store.delete("api-key").unwrap();

    // 验证删除
    let all = store.read_all().unwrap();
    assert!(!all.contains_key("api-key"));
    cleanup(&store);
}

#[test]
#[ignore = "需真实 OS keychain 可达（桌面解锁会话）：keychain 锁定（dark wake）时 securityd 调用会阻塞；unique_service 已消除跨构建 ACL 弹框（990 号）"]
fn test_multiple_keys() {
    let store = KeyringStore::new(&unique_service("multi"));

    // 写入多个 key
    store.upsert("key1", "value1").unwrap();
    store.upsert("key2", "value2").unwrap();
    store.upsert("key3", "value3").unwrap();

    // 验证独立性
    let all = store.read_all().unwrap();
    assert_eq!(all.get("key1"), Some(&"value1".to_string()));
    assert_eq!(all.get("key2"), Some(&"value2".to_string()));
    assert_eq!(all.get("key3"), Some(&"value3".to_string()));

    // 删除其中一个
    store.delete("key2").unwrap();

    // 验证其他 key 不受影响
    let all = store.read_all().unwrap();
    assert_eq!(all.get("key1"), Some(&"value1".to_string()));
    assert!(!all.contains_key("key2"));
    assert_eq!(all.get("key3"), Some(&"value3".to_string()));

    // 清理
    store.delete("key1").unwrap();
    store.delete("key3").unwrap();
    cleanup(&store);
}

#[test]
#[ignore = "需真实 OS keychain 可达（桌面解锁会话）：keychain 锁定（dark wake）时 securityd 调用会阻塞；unique_service 已消除跨构建 ACL 弹框（990 号）"]
fn test_empty_value() {
    let store = KeyringStore::new(&unique_service("empty"));

    // 空字符串应该能正常存储
    store.upsert("empty-key", "").unwrap();
    let all = store.read_all().unwrap();
    assert_eq!(all.get("empty-key"), Some(&"".to_string()));

    // 清理
    store.delete("empty-key").unwrap();
    cleanup(&store);
}

#[test]
#[ignore = "需真实 OS keychain 可达（桌面解锁会话）：keychain 锁定（dark wake）时 securityd 调用会阻塞；unique_service 已消除跨构建 ACL 弹框（990 号）"]
fn test_unicode_value() {
    let store = KeyringStore::new(&unique_service("unicode"));

    let unicode_value = "测试值 🚀 émoji";
    store.upsert("unicode-key", unicode_value).unwrap();
    let all = store.read_all().unwrap();
    assert_eq!(all.get("unicode-key"), Some(&unicode_value.to_string()));

    // 清理
    store.delete("unicode-key").unwrap();
    cleanup(&store);
}
