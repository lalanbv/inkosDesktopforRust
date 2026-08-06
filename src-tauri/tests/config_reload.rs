//! 配置热重载集成测试

use inkos_desktop::config::{
    AppConfig, ConfigChangeEvent, ConfigLoader, ConfigManager, ConfigPaths, ConfigReloader,
    ConfigWatcher, LoggingConfig,
};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_config_watcher_detects_workspace_changes() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let loader = ConfigLoader::new(paths.clone());

    // 创建工作区配置目录
    paths.ensure_workspace_config_dir("ws-test").unwrap();

    // 启动监听器
    let mut watcher = ConfigWatcher::new().unwrap();
    watcher.watch_workspace_config(&paths, "ws-test").unwrap();

    // 保存配置（触发变更）
    let config = AppConfig {
        logging: LoggingConfig {
            level: "debug".to_string(),
            max_file_size_mb: 20,
            max_backups: 5,
        },
        ..Default::default()
    };
    loader.save_workspace_config("ws-test", &config).unwrap();

    // 等待文件系统事件
    thread::sleep(Duration::from_secs(3));

    // 检查事件
    let events = watcher.poll_events();
    assert!(!events.is_empty(), "应该检测到配置变更事件");

    if let ConfigChangeEvent::WorkspaceChanged(ws_id) = &events[0] {
        assert_eq!(ws_id, "ws-test");
    } else {
        panic!("事件类型错误");
    }
}

#[test]
fn test_config_watcher_detects_project_changes() {
    let temp = TempDir::new().unwrap();
    let project_root = temp.path().join("project");
    std::fs::create_dir(&project_root).unwrap();
    ConfigPaths::ensure_project_config_dir(&project_root).unwrap();

    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let loader = ConfigLoader::new(paths.clone());

    // 启动监听器
    let mut watcher = ConfigWatcher::new().unwrap();
    watcher.watch_project_config(&project_root).unwrap();

    // 保存配置（触发变更）
    let config = AppConfig {
        logging: LoggingConfig {
            level: "trace".to_string(),
            max_file_size_mb: 30,
            max_backups: 10,
        },
        ..Default::default()
    };
    loader.save_project_config(&project_root, &config).unwrap();

    // 等待文件系统事件
    thread::sleep(Duration::from_secs(3));

    // 检查事件
    let events = watcher.poll_events();
    assert!(!events.is_empty(), "应该检测到配置变更事件");

    if let ConfigChangeEvent::ProjectChanged(path) = &events[0] {
        assert_eq!(path, &project_root);
    } else {
        panic!("事件类型错误");
    }
}

#[test]
fn test_config_reloader_updates_manager() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let loader = ConfigLoader::new(paths.clone());
    let mut manager = ConfigManager::new();

    // 创建重载器
    let reloader = ConfigReloader::new(loader.clone(), AppConfig::default());

    // 保存工作区配置
    let workspace_config = AppConfig {
        logging: LoggingConfig {
            level: "debug".to_string(),
            max_file_size_mb: 20,
            max_backups: 5,
        },
        ..Default::default()
    };
    loader
        .save_workspace_config("ws-reload", &workspace_config)
        .unwrap();

    // 重新加载配置
    let result = reloader.reload_workspace_config("ws-reload", &mut manager);
    assert!(result.is_ok());

    let merged = result.unwrap();
    assert_eq!(merged.logging.level, "debug");
    assert_eq!(merged.logging.max_file_size_mb, 20);
}

#[test]
fn test_config_reloader_validates_before_reload() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let loader = ConfigLoader::new(paths.clone());
    let mut manager = ConfigManager::new();

    // 创建重载器（保存初始有效配置）
    let initial_config = AppConfig {
        logging: LoggingConfig {
            level: "info".to_string(),
            max_file_size_mb: 10,
            max_backups: 3,
        },
        ..Default::default()
    };
    let reloader = ConfigReloader::new(loader.clone(), initial_config.clone());

    // 保存无效配置
    let invalid_config = AppConfig {
        logging: LoggingConfig {
            level: "invalid_level".to_string(), // 无效日志级别
            max_file_size_mb: 10,
            max_backups: 3,
        },
        ..Default::default()
    };
    loader
        .save_workspace_config("ws-invalid", &invalid_config)
        .unwrap();

    // 重新加载应该失败
    let result = reloader.reload_workspace_config("ws-invalid", &mut manager);
    assert!(result.is_err());

    // 应该保持最后有效配置
    let last_valid = reloader.get_last_valid_config();
    assert_eq!(last_valid.logging.level, "info");
}

#[test]
fn test_config_watcher_debounce() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let loader = ConfigLoader::new(paths.clone());

    paths.ensure_workspace_config_dir("ws-debounce").unwrap();

    // 启动监听器
    let mut watcher = ConfigWatcher::new().unwrap();
    watcher
        .watch_workspace_config(&paths, "ws-debounce")
        .unwrap();

    let config = AppConfig {
        logging: LoggingConfig {
            level: "debug".to_string(),
            max_file_size_mb: 20,
            max_backups: 5,
        },
        ..Default::default()
    };

    // 快速连续保存多次
    for _ in 0..5 {
        loader
            .save_workspace_config("ws-debounce", &config)
            .unwrap();
        thread::sleep(Duration::from_millis(100));
    }

    // 等待文件系统事件
    thread::sleep(Duration::from_secs(3));

    // 检查事件（应该被防抖合并）
    let events = watcher.poll_events();
    assert!(
        events.len() <= 2,
        "防抖应该减少事件数量，实际: {}",
        events.len()
    );
}

#[test]
fn test_full_hot_reload_cycle() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let loader = ConfigLoader::new(paths.clone());
    let mut manager = ConfigManager::new();

    paths.ensure_workspace_config_dir("ws-full").unwrap();

    // 创建重载器
    let reloader = ConfigReloader::new(loader.clone(), AppConfig::default());

    // 启动监听器
    let mut watcher = ConfigWatcher::new().unwrap();
    watcher.watch_workspace_config(&paths, "ws-full").unwrap();

    // 保存配置
    let config = AppConfig {
        logging: LoggingConfig {
            level: "trace".to_string(),
            max_file_size_mb: 50,
            max_backups: 10,
        },
        ..Default::default()
    };
    loader.save_workspace_config("ws-full", &config).unwrap();

    // 等待文件系统事件
    thread::sleep(Duration::from_secs(3));

    // 检查事件
    let events = watcher.poll_events();
    assert!(!events.is_empty());

    // 重新加载配置
    let result = reloader.reload_workspace_config("ws-full", &mut manager);
    assert!(result.is_ok());

    let merged = result.unwrap();
    assert_eq!(merged.logging.level, "trace");
    assert_eq!(merged.logging.max_file_size_mb, 50);
    assert_eq!(merged.logging.max_backups, 10);
}
