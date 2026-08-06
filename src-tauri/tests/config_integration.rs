//! 配置系统集成测试

use inkos_desktop::config::{AppConfig, ConfigLoader, ConfigPaths};
use tempfile::TempDir;

#[test]
fn test_config_three_layer_merge() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let loader = ConfigLoader::new(paths);

    // 初始化管理器（系统默认）
    let mut mgr = loader.init_manager().unwrap();
    assert_eq!(mgr.merged().logging.level, "info");

    // 设置工作区配置
    let mut ws_cfg = AppConfig::default();
    ws_cfg.logging.level = "debug".to_string();
    loader.save_workspace_config("ws-1", &ws_cfg).unwrap();
    loader.apply_workspace_config(&mut mgr, "ws-1").unwrap();
    assert_eq!(mgr.merged().logging.level, "debug");

    // 设置项目配置
    let project_root = temp.path().join("project-1");
    std::fs::create_dir(&project_root).unwrap();
    let mut proj_cfg = AppConfig::default();
    proj_cfg.logging.level = "trace".to_string();
    proj_cfg.updates.channel = "beta".to_string();
    loader.save_project_config(&project_root, &proj_cfg).unwrap();
    loader.apply_project_config(&mut mgr, &project_root).unwrap();

    // 验证三层合并
    let merged = mgr.merged();
    assert_eq!(merged.logging.level, "trace");  // 项目层覆盖
    assert_eq!(merged.updates.channel, "beta"); // 项目层覆盖
}

#[test]
fn test_config_persistence() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let loader = ConfigLoader::new(paths);

    // 保存工作区配置
    let mut ws_cfg = AppConfig::default();
    ws_cfg.logging.level = "warn".to_string();
    loader.save_workspace_config("ws-persist", &ws_cfg).unwrap();

    // 重新加载验证持久化
    let loaded = loader.load_workspace_config("ws-persist").unwrap();
    assert_eq!(loaded.logging.level, "warn");
}

#[test]
fn test_config_layer_isolation() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let loader = ConfigLoader::new(paths);

    let mut mgr = loader.init_manager().unwrap();

    // 设置工作区 A
    let mut ws_a = AppConfig::default();
    ws_a.logging.level = "debug".to_string();
    loader.save_workspace_config("ws-a", &ws_a).unwrap();
    loader.apply_workspace_config(&mut mgr, "ws-a").unwrap();
    assert_eq!(mgr.merged().logging.level, "debug");

    // 切换到工作区 B
    let mut ws_b = AppConfig::default();
    ws_b.logging.level = "error".to_string();
    loader.save_workspace_config("ws-b", &ws_b).unwrap();
    loader.apply_workspace_config(&mut mgr, "ws-b").unwrap();
    assert_eq!(mgr.merged().logging.level, "error");

    // 验证工作区 A 未被污染
    loader.apply_workspace_config(&mut mgr, "ws-a").unwrap();
    assert_eq!(mgr.merged().logging.level, "debug");
}

#[test]
fn test_config_clear_layers() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let loader = ConfigLoader::new(paths);

    let mut mgr = loader.init_manager().unwrap();

    // 设置工作区和项目配置
    let mut ws_cfg = AppConfig::default();
    ws_cfg.logging.level = "debug".to_string();
    loader.save_workspace_config("ws-clear", &ws_cfg).unwrap();
    loader.apply_workspace_config(&mut mgr, "ws-clear").unwrap();

    let project_root = temp.path().join("project-clear");
    std::fs::create_dir(&project_root).unwrap();
    let mut proj_cfg = AppConfig::default();
    proj_cfg.logging.level = "trace".to_string();
    loader.save_project_config(&project_root, &proj_cfg).unwrap();
    loader.apply_project_config(&mut mgr, &project_root).unwrap();

    assert_eq!(mgr.merged().logging.level, "trace");

    // 清除项目层
    mgr.clear_project();
    assert_eq!(mgr.merged().logging.level, "debug"); // 回退到工作区

    // 清除工作区层
    mgr.clear_workspace();
    assert_eq!(mgr.merged().logging.level, "info"); // 回退到系统默认
}

#[test]
fn test_config_validation() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());
    let _loader = ConfigLoader::new(paths);

    // 无效的日志级别
    let mut invalid_cfg = AppConfig::default();
    invalid_cfg.logging.level = "invalid-level".to_string();

    let result = invalid_cfg.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("无效的日志级别"));

    // 无效的更新通道
    let mut invalid_channel = AppConfig::default();
    invalid_channel.updates.channel = "unknown".to_string();

    let result = invalid_channel.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("无效的更新通道"));
}

#[test]
fn test_config_paths() {
    let temp = TempDir::new().unwrap();
    let paths = ConfigPaths::new(temp.path().to_path_buf());

    // 系统配置路径
    let system_path = paths.system_config();
    assert!(system_path.to_string_lossy().contains("config/default.toml"));

    // 工作区配置路径
    let ws_path = paths.workspace_config("ws-123");
    assert!(ws_path.to_string_lossy().contains("workspace-ws-123/config.toml"));

    // 项目配置路径
    let proj_path = ConfigPaths::project_config(&temp.path().join("my-project"));
    assert!(proj_path.to_string_lossy().contains(".inkos/config.toml"));
}
