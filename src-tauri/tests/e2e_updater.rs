//! E2E Updater 契约测试
//!
//! 验证 engine 更新流程：原子替换 + 回滚。
//!
//! 运行：cargo test --release --test e2e_updater -- --ignored --test-threads=1

use inkos_desktop::updater::engine;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// 创建 mock engine 目录（带 manifest.json）
fn create_mock_engine(engine_dir: &PathBuf, version: &str) {
    fs::create_dir_all(engine_dir).unwrap();

    let manifest = serde_json::json!({
        "version": version,
        "built_at": "2026-08-06T12:00:00Z",
        "platform": "test",
        "arch": "test"
    });

    fs::write(
        engine_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn test_atomic_replace() {
    let temp_dir = TempDir::new().unwrap();
    let engine_dir = temp_dir.path().join("engine");
    let bak_dir = temp_dir.path().join("engine.bak");
    let new_dir = temp_dir.path().join("new-engine");

    // 1. 创建初始版本
    create_mock_engine(&engine_dir, "0.3.0");
    fs::write(engine_dir.join("test-file.txt"), "old content").unwrap();

    // 2. 创建新版本
    create_mock_engine(&new_dir, "0.4.0");
    fs::write(new_dir.join("test-file.txt"), "new content").unwrap();

    // 3. 健康检查：manifest.json 存在
    let health_check = |path: &std::path::Path| path.join("manifest.json").exists();

    // 4. 原子替换
    let result = engine::atomic_replace_with_rollback(&engine_dir, &bak_dir, &new_dir, &health_check);
    assert!(result.is_ok(), "原子替换应该成功: {:?}", result);

    // 5. 验证新版本
    let manifest_content = fs::read_to_string(engine_dir.join("manifest.json")).unwrap();
    assert!(manifest_content.contains("0.4.0"));

    let file_content = fs::read_to_string(engine_dir.join("test-file.txt")).unwrap();
    assert_eq!(file_content, "new content");

    // 6. 验证备份存在
    assert!(bak_dir.exists(), "备份目录应该存在");
}

#[test]
fn test_atomic_replace_rollback_on_failure() {
    let temp_dir = TempDir::new().unwrap();
    let engine_dir = temp_dir.path().join("engine");
    let bak_dir = temp_dir.path().join("engine.bak");
    let new_dir = temp_dir.path().join("new-engine");

    // 1. 创建初始版本
    create_mock_engine(&engine_dir, "0.3.0");

    // 2. 创建损坏的新版本（缺少 manifest.json）
    fs::create_dir_all(&new_dir).unwrap();
    fs::write(new_dir.join("test-file.txt"), "corrupt").unwrap();
    // 故意不创建 manifest.json

    // 3. 健康检查：manifest.json 必须存在
    let health_check = |path: &std::path::Path| path.join("manifest.json").exists();

    // 4. 尝试替换应失败并回滚
    let result = engine::atomic_replace_with_rollback(&engine_dir, &bak_dir, &new_dir, &health_check);
    assert!(result.is_err(), "损坏的 engine 应触发回滚");

    // 5. 验证回滚：原版本仍完整
    let manifest_content = fs::read_to_string(engine_dir.join("manifest.json")).unwrap();
    assert!(manifest_content.contains("0.3.0"), "回滚后应恢复原版本");
}

#[test]
fn test_atomic_replace_first_install() {
    let temp_dir = TempDir::new().unwrap();
    let engine_dir = temp_dir.path().join("engine");
    let bak_dir = temp_dir.path().join("engine.bak");
    let new_dir = temp_dir.path().join("new-engine");

    // 1. engine_dir 不存在（首次安装）
    assert!(!engine_dir.exists());

    // 2. 创建新版本
    create_mock_engine(&new_dir, "0.4.0");

    // 3. 健康检查
    let health_check = |path: &std::path::Path| path.join("manifest.json").exists();

    // 4. 原子替换（首装，无备份步骤）
    let result = engine::atomic_replace_with_rollback(&engine_dir, &bak_dir, &new_dir, &health_check);
    assert!(result.is_ok(), "首次安装应该成功");

    // 5. 验证新版本
    let manifest_content = fs::read_to_string(engine_dir.join("manifest.json")).unwrap();
    assert!(manifest_content.contains("0.4.0"));
}

#[test]
#[ignore] // 需要网络访问，CI 专用
fn test_is_newer_version_check() {
    // 测试版本比较逻辑（不需要网络）
    let current_version = "0.1.0";
    let new_version = "v0.2.0";

    let result = inkos_desktop::updater::is_newer(current_version, new_version);
    assert!(result.is_some(), "应该检测到新版本");
    assert_eq!(result.unwrap().to_string(), "0.2.0");
}
