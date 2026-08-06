//! E2E Secrets 契约测试
//!
//! 验证跨平台 keychain 集成：upsert + read_all + delete。
//!
//! 运行：cargo test --release --test e2e_secrets -- --ignored --test-threads=1

use inkos_desktop::secrets::store::{KeyringStore, SecretStore};

#[test]
#[ignore = "需真实 OS keychain：机器休眠/锁屏时报 dark wake，首次访问会弹授权框而阻塞"]
fn test_keychain_upsert() {
    // 每个测试用独立 service：同名 service 共享一个 `__index__` entry，
    // 并行跑时彼此覆盖索引，导致 read_all 漏读。
    let store = KeyringStore::new("inkos-e2e-upsert");
    let result = store.upsert("test-key", "test-value");
    assert!(result.is_ok(), "keychain upsert 应该成功");

    // 清理
    let _ = store.delete("test-key");
}

#[test]
#[ignore = "需真实 OS keychain：机器休眠/锁屏时报 dark wake，首次访问会弹授权框而阻塞"]
fn test_keychain_read_all() {
    let store = KeyringStore::new("inkos-e2e-readall");
    store.upsert("test-key", "test-value").unwrap();

    let all = store.read_all().unwrap();
    assert_eq!(all.get("test-key"), Some(&"test-value".to_string()));

    // 清理
    store.delete("test-key").unwrap();
}

#[test]
#[ignore = "需真实 OS keychain：机器休眠/锁屏时报 dark wake，首次访问会弹授权框而阻塞"]
fn test_keychain_delete() {
    let store = KeyringStore::new("inkos-e2e-delete");
    store.upsert("test-key", "test-value").unwrap();

    let result = store.delete("test-key");
    assert!(result.is_ok());

    // 验证已删除
    let all = store.read_all().unwrap();
    assert!(!all.contains_key("test-key"), "删除后不应存在");
}

#[test]
#[ignore = "需真实 OS keychain：机器休眠/锁屏时报 dark wake，首次访问会弹授权框而阻塞"]
fn test_keychain_read_empty() {
    let store = KeyringStore::new("inkos-e2e-empty-test");

    let all = store.read_all().unwrap();
    assert!(all.is_empty(), "空 keychain 应返回空 HashMap");
}

#[test]
#[ignore = "需真实 OS keychain：机器休眠/锁屏时报 dark wake，首次访问会弹授权框而阻塞"]
fn test_keychain_overwrite() {
    let store = KeyringStore::new("inkos-e2e-overwrite");

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
}

#[test]
#[ignore = "需真实 OS keychain：机器休眠/锁屏时报 dark wake，首次访问会弹授权框而阻塞"]
fn test_secrets_full_cycle() {
    let store = KeyringStore::new("inkos-e2e-full");

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
}

#[test]
#[ignore = "需真实 OS keychain：机器休眠/锁屏时报 dark wake，首次访问会弹授权框而阻塞"]
fn test_multiple_keys() {
    let store = KeyringStore::new("inkos-e2e-multi");

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
}

#[test]
#[ignore = "需真实 OS keychain：机器休眠/锁屏时报 dark wake，首次访问会弹授权框而阻塞"]
fn test_empty_value() {
    let store = KeyringStore::new("inkos-e2e-empty");

    // 空字符串应该能正常存储
    store.upsert("empty-key", "").unwrap();
    let all = store.read_all().unwrap();
    assert_eq!(all.get("empty-key"), Some(&"".to_string()));

    // 清理
    store.delete("empty-key").unwrap();
}

#[test]
#[ignore = "需真实 OS keychain：机器休眠/锁屏时报 dark wake，首次访问会弹授权框而阻塞"]
fn test_unicode_value() {
    let store = KeyringStore::new("inkos-e2e-unicode");

    let unicode_value = "测试值 🚀 émoji";
    store.upsert("unicode-key", unicode_value).unwrap();
    let all = store.read_all().unwrap();
    assert_eq!(all.get("unicode-key"), Some(&unicode_value.to_string()));

    // 清理
    store.delete("unicode-key").unwrap();
}
