//! WASM 插件 execute 精细 bench（criterion）——量化 Component 实例化 + invoke 延迟
//!
//! 运行：先编译示例 component（cd examples/wasm-plugin && cargo build --target wasm32-wasip2 --release），
//!       再 cargo bench --bench plugin_execute
//! 产出：target/criterion/report/index.html
//!
//! 与 tests/perf_plugin_wasm.rs 互补：perf_plugin_wasm 是断言门，
//! criterion 是统计分析（实例化开销分解、回归对比）。

use criterion::{criterion_group, criterion_main, Criterion};
use inkos_desktop::plugin::{PluginMetadata, WasmPlugin};
use std::collections::HashMap;
use std::path::PathBuf;

fn example_wasm() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/wasm-plugin/target/wasm32-wasip2/release/inkos_example_plugin.wasm")
}

fn metadata() -> PluginMetadata {
    PluginMetadata {
        id: "bench".to_string(),
        name: "Bench".to_string(),
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

fn bench_execute(c: &mut Criterion) {
    let wasm = example_wasm();
    if !wasm.exists() {
        // 自举编译示例 component（而非静默空跑），让 bench 在干净环境也能跑。
        // 需 wasm32-wasip2 target；编译失败才 skip（不静默隐藏问题）。
        eprintln!("[bench] 示例 component 未编译，自举编译...");
        let plugin_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../examples/wasm-plugin");
        let status = std::process::Command::new("cargo")
            .args(["build", "--target", "wasm32-wasip2", "--release"])
            .current_dir(&plugin_dir)
            .status();
        let ok = matches!(status, Ok(s) if s.success()) && wasm.exists();
        if !ok {
            eprintln!(
                "[skip] 自举编译失败（需 wasm32-wasip2 target：rustup target add wasm32-wasip2）"
            );
            return;
        }
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let plugin = WasmPlugin::new(metadata(), &wasm, tmp.path()).expect("加载 component");

    let mut group = c.benchmark_group("plugin_execute");
    group.sample_size(20); // execute 含实例化（开销高），样本降到 20 加速
    group.bench_function("wasm_echo", |b| {
        b.iter(|| {
            plugin
                .execute("echo", serde_json::json!({"bench": true}))
                .unwrap()
        });
    });
    group.finish();
}

criterion_group!(benches, bench_execute);
criterion_main!(benches);
