//! WASM 插件端到端执行测试（Phase 6.3 第 5 步）
//!
//! 加载 examples/wasm-plugin 编译的真实 .wasm component，经 Component Model
//! 类型化调用 plugin.invoke，验证完整链路：Linker(Host trait) → 实例化 → invoke。
//!
//! 前置：示例 component 已编译（wasm32-wasip2 target）：
//!   cd examples/wasm-plugin && cargo build --target wasm32-wasip2 --release
//! 未编译则 skip（不判失败）——本测试需要 wasm 工具链，CI 按需启用。

use inkos_desktop::plugin::{PluginMetadata, WasmPlugin};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

/// 示例 WASM component 产物路径（examples/wasm-plugin 独立 crate 的 target）
fn example_wasm() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/wasm-plugin/target/wasm32-wasip2/release/inkos_example_plugin.wasm")
}

fn wasm_available() -> bool {
    example_wasm().exists()
}

fn test_metadata() -> PluginMetadata {
    PluginMetadata {
        id: "wasm-test".to_string(),
        name: "WASM Test".to_string(),
        version: "1.0.0".to_string(),
        description: String::new(),
        author: String::new(),
        homepage: None,
        license: String::new(),
        abi_version: "1".to_string(),
        capabilities: Vec::new(),
        entrypoint: "plugin.wasm".to_string(),
        dependencies: HashMap::new(),
        enabled: true,
    }
}

#[test]
fn wasm_plugin_echo_roundtrip() {
    if !wasm_available() {
        eprintln!("[skip] 示例 WASM component 未编译（需 wasm32-wasip2 target）");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let plugin = WasmPlugin::new(test_metadata(), &example_wasm(), tmp.path())
        .expect("加载 WASM component 失败");

    // echo 原样回显 args
    let result = plugin
        .execute("echo", serde_json::json!({"hello": "world"}))
        .expect("execute echo 失败");
    assert_eq!(result["hello"], "world");
}

#[test]
fn wasm_plugin_ping_returns_pong() {
    if !wasm_available() {
        eprintln!("[skip] 示例 WASM component 未编译");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let plugin = WasmPlugin::new(test_metadata(), &example_wasm(), tmp.path()).unwrap();

    let result = plugin
        .execute("ping", serde_json::Value::Null)
        .expect("execute ping 失败");
    assert_eq!(result["pong"], true);
}

#[test]
fn wasm_plugin_unknown_command_returns_error() {
    if !wasm_available() {
        eprintln!("[skip] 示例 WASM component 未编译");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let plugin = WasmPlugin::new(test_metadata(), &example_wasm(), tmp.path()).unwrap();

    // 未知命令：插件返回 Err(result)，execute 转为 PluginError
    let result = plugin.execute("nonexistent", serde_json::Value::Null);
    assert!(result.is_err(), "未知命令应返回错误");
}
