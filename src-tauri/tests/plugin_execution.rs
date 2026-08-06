//! 插件进程隔离执行 —— 端到端集成测试
//!
//! 用 `examples/plugins/hello-plugin/plugin.sh`（实现 JSON-RPC over stdio）
//! 验证 `PluginManager::execute_plugin` 的完整链路：安装 → 启动长驻进程 →
//! JSON-RPC 调用 → 拿回真实结果。
//!
//! 依赖：bash + jq。CI 环境若无 jq 则跳过（不判失败）。

use inkos_desktop::plugin::PluginManager;
use std::path::PathBuf;
use tempfile::TempDir;

/// 示例插件源目录（仓库内 examples/plugins/hello-plugin）
fn example_plugin_source() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../examples/plugins/hello-plugin")
}

/// 探测 jq 是否可用——示例插件脚本依赖它解析 JSON。
fn jq_available() -> bool {
    std::process::Command::new("jq")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

#[tokio::test]
async fn execute_echo_round_trip() {
    if !jq_available() {
        eprintln!("[skip] jq 未安装，跳过插件进程隔离集成测试");
        return;
    }
    let source = example_plugin_source();
    if !source.exists() {
        eprintln!("[skip] 示例插件源不存在: {}", source.display());
        return;
    }

    let temp = TempDir::new().unwrap();
    let plugins_dir = temp.path().join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();

    let mut manager = PluginManager::new(&plugins_dir).unwrap();

    // 安装示例插件
    manager
        .install_plugin(source.to_str().unwrap())
        .await
        .expect("安装插件失败");
    assert_eq!(manager.list_plugins().len(), 1);

    // 调用 plugin.echo：发什么 params 回什么
    let result = manager
        .execute_plugin("hello-plugin", "plugin.echo", serde_json::json!({"hello": "world"}))
        .await
        .expect("echo 调用失败");

    assert_eq!(result["echo"]["hello"], "world");
}

#[tokio::test]
async fn execute_upper_transforms_text() {
    if !jq_available() {
        eprintln!("[skip] jq 未安装，跳过");
        return;
    }
    let source = example_plugin_source();
    if !source.exists() {
        return;
    }

    let temp = TempDir::new().unwrap();
    let plugins_dir = temp.path().join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();

    let mut manager = PluginManager::new(&plugins_dir).unwrap();
    manager.install_plugin(source.to_str().unwrap()).await.unwrap();

    let result = manager
        .execute_plugin(
            "hello-plugin",
            "plugin.upper",
            serde_json::json!({"text": "hello inkos"}),
        )
        .await
        .expect("upper 调用失败");

    assert_eq!(result["text"], "HELLO INKOS");
}

#[tokio::test]
async fn execute_health_returns_metadata() {
    if !jq_available() {
        eprintln!("[skip] jq 未安装，跳过");
        return;
    }
    let source = example_plugin_source();
    if !source.exists() {
        return;
    }

    let temp = TempDir::new().unwrap();
    let plugins_dir = temp.path().join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();

    let mut manager = PluginManager::new(&plugins_dir).unwrap();
    manager.install_plugin(source.to_str().unwrap()).await.unwrap();

    let result = manager
        .execute_plugin("hello-plugin", "plugin.health", serde_json::Value::Null)
        .await
        .expect("health 调用失败");

    assert_eq!(result["status"], "healthy");
    assert_eq!(result["name"], "hello-plugin");
    assert_eq!(result["version"], "1.0.0");
}

#[tokio::test]
async fn execute_unknown_method_returns_error() {
    if !jq_available() {
        eprintln!("[skip] jq 未安装，跳过");
        return;
    }
    let source = example_plugin_source();
    if !source.exists() {
        return;
    }

    let temp = TempDir::new().unwrap();
    let plugins_dir = temp.path().join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();

    let mut manager = PluginManager::new(&plugins_dir).unwrap();
    manager.install_plugin(source.to_str().unwrap()).await.unwrap();

    // 未知方法：插件返回 JSON-RPC error，execute_plugin 应转成 Err
    let result = manager
        .execute_plugin("hello-plugin", "plugin.nonexistent", serde_json::Value::Null)
        .await;

    assert!(result.is_err(), "未知方法应返回错误");
}

#[tokio::test]
async fn execute_disabled_plugin_is_rejected() {
    if !jq_available() {
        eprintln!("[skip] jq 未安装，跳过");
        return;
    }
    let source = example_plugin_source();
    if !source.exists() {
        return;
    }

    let temp = TempDir::new().unwrap();
    let plugins_dir = temp.path().join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();

    let mut manager = PluginManager::new(&plugins_dir).unwrap();
    manager.install_plugin(source.to_str().unwrap()).await.unwrap();

    // 禁用后执行应被拒
    manager.disable_plugin("hello-plugin").unwrap();
    let result = manager
        .execute_plugin("hello-plugin", "plugin.echo", serde_json::Value::Null)
        .await;

    assert!(result.is_err());
}
