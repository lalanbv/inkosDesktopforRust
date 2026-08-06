//! WASM 插件 execute 性能回归测试——量化 Component 实例化 + invoke 延迟
//!
//! execute 每次新建 Store + 实例化 Component + 调 invoke，开销高于进程隔离的
//! 长驻进程 call。此处断言延迟上限，防数量级回归（非微基准）。

use inkos_desktop::plugin::{PluginMetadata, WasmPlugin};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use tempfile::TempDir;

fn example_wasm() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/wasm-plugin/target/wasm32-wasip2/release/inkos_example_plugin.wasm")
}

fn test_metadata() -> PluginMetadata {
    PluginMetadata {
        id: "wasm-perf".to_string(),
        name: "WASM Perf".to_string(),
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
fn wasm_execute_echo_latency_acceptable() {
    // execute 含 Store 创建 + WASI 注册 + Component 实例化 + invoke。
    // debug 构建慢，留余量到 500ms（release 下通常 <50ms）。
    let wasm = example_wasm();
    if !wasm.exists() {
        eprintln!("[skip] 示例 WASM component 未编译（需 wasm32-wasip2 target）");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let plugin = WasmPlugin::new(test_metadata(), &wasm, tmp.path()).unwrap();

    let start = Instant::now();
    let result = plugin
        .execute("echo", serde_json::json!({"perf": "test"}))
        .expect("execute echo 失败");
    let elapsed = start.elapsed();

    assert_eq!(result["perf"], "test");
    assert!(
        elapsed.as_millis() < 500,
        "WASM execute（含实例化）应 <500ms（debug 余量），实际 {:?}",
        elapsed
    );
}
