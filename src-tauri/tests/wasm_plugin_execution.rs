//! WASM 插件端到端执行测试（Phase 6.3 第 5 步）
//!
//! 加载 examples/wasm-plugin 编译的真实 .wasm component，经 Component Model
//! 类型化调用 plugin.invoke，验证完整链路：Linker(Host trait) → 实例化 → invoke。
//!
//! 组件来源：优先本地新构建产物，缺失则回退 tests/fixtures/ 受跟踪组件
//! （两者任一存在即跑；本测试不再依赖 wasm 工具链）。

use inkos_desktop::plugin::{PluginMetadata, WasmPlugin};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

/// 示例 WASM component 产物路径：
/// 优先 examples/wasm-plugin 的新构建产物（本地开发迭代用）；
/// 缺失则回退 tests/fixtures/ 下的受跟踪组件（66KB）——保证新克隆环境
/// 不静默跳过，wasmtime 升级始终有行为级验证兜底。
fn example_wasm() -> PathBuf {
    let fresh = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../examples/wasm-plugin/target/wasm32-wasip2/release/inkos_example_plugin.wasm",
    );
    if fresh.exists() {
        return fresh;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/inkos_example_plugin.wasm")
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

/// 569 号：wit 契约单源护栏——示例插件的 wit 副本必须与宿主契约逐字一致。
/// 历史教训：副本曾缺 exec-command 整块（语义级分叉，guest 绑定静默缺能力）。
#[test]
fn wit_contract_copy_in_sync_with_host() {
    let host = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("wit/inkos.wit"),
    )
    .expect("宿主 wit 在库");
    let guest = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/wasm-plugin/wit/inkos.wit"),
    )
    .expect("示例插件 wit 副本在库（examples/ 随仓分发）");
    assert_eq!(guest, host, "wit 副本与宿主契约分叉——guest 绑定会静默缺失能力面");
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

// ── 569 号：guest→host 能力链端到端（沙箱全链：guest import → Host trait →
// HostContext capability 校验 → fs/进程）。fixture 插件新增 host-read /
// host-exec / host-log 三命令 + on-event 经 host.log 观测。 ──

use inkos_desktop::plugin::{Capability, PluginError};

/// 带 capability 的 metadata 构造器。
fn metadata_with_capabilities(capabilities: Vec<Capability>) -> PluginMetadata {
    PluginMetadata { capabilities, ..test_metadata() }
}

#[test]
fn wasm_plugin_host_read_with_capability_returns_content() {
    if !wasm_available() {
        eprintln!("[skip] 示例 WASM component 未编译");
        return;
    }
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("note.txt"), "沙箱内的内容").unwrap();
    // 路径允许列表语义：ReadProject 之外还须 Filesystem{绝对路径}（相对路径
    // fail-closed——纵深防御，host_api 只认绝对 allowed_path）；tempdir 需
    // canonicalize（macOS /var → /private/var 符号链接）。
    let allowed_dir = tmp.path().canonicalize().unwrap();
    let plugin = WasmPlugin::new(
        metadata_with_capabilities(vec![
            Capability::ReadProject,
            Capability::Filesystem { path: allowed_dir.to_string_lossy().into_owned() },
        ]),
        &example_wasm(),
        tmp.path(),
    )
    .expect("加载失败");

    let result = plugin
        .execute("host-read", serde_json::json!({ "path": "note.txt" }))
        .expect("guest→host read 链失败");
    assert_eq!(result["content"], "沙箱内的内容");
}

#[test]
fn wasm_plugin_host_read_denied_without_capability() {
    if !wasm_available() {
        eprintln!("[skip] 示例 WASM component 未编译");
        return;
    }
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("note.txt"), "秘密").unwrap();
    // 零 capability → HostContext 拒绝 → guest 收 err → execute 报 ExecutionFailed。
    let plugin = WasmPlugin::new(metadata_with_capabilities(vec![]), &example_wasm(), tmp.path())
        .expect("加载失败");

    let err = plugin
        .execute("host-read", serde_json::json!({ "path": "note.txt" }))
        .expect_err("无 read_project 权限必须拒绝");
    let message = err.to_string();
    assert!(
        message.contains("缺少 read_project 权限"),
        "拒绝消息应透传权限语义: {message}"
    );
}

#[test]
fn wasm_plugin_host_exec_whitelist_pass_and_fail_closed() {
    if !wasm_available() {
        eprintln!("[skip] 示例 WASM component 未编译");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let plugin = WasmPlugin::new(
        metadata_with_capabilities(vec![Capability::SystemCommand {
            allowed_commands: vec!["echo".to_string()],
        }]),
        &example_wasm(),
        tmp.path(),
    )
    .expect("加载失败");

    // 白名单内：echo 经宿主执行，stdout 回到 guest 再回到断言。
    let result = plugin
        .execute("host-exec", serde_json::json!({ "command": "echo", "args": ["hello"] }))
        .expect("白名单内命令应执行");
    assert_eq!(result["exitCode"], 0);
    assert!(
        result["stdout"].as_str().unwrap_or_default().contains("hello"),
        "stdout: {:?}",
        result["stdout"]
    );

    // 白名单外：fail-closed（命令名不匹配 → PermissionDenied 透传）。
    let err = plugin
        .execute("host-exec", serde_json::json!({ "command": "curl", "args": [] }))
        .expect_err("白名单外命令必须拒绝");
    assert!(
        err.to_string().contains("缺少 system_command 权限或命令不在白名单"),
        "{err}"
    );
}

#[test]
fn wasm_plugin_broadcast_event_completes_chain() {
    if !wasm_available() {
        eprintln!("[skip] 示例 WASM component 未编译");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let plugin = WasmPlugin::new(metadata_with_capabilities(vec![]), &example_wasm(), tmp.path())
        .expect("加载失败");
    // on-event 无返回值——链路完成（实例化→init→on_event→host.log）即 Ok；
    // guest 侧经 host.log 落 tracing，观测面在 JSON 日志可查。
    plugin
        .broadcast_event("project.opened", r#"{"path":"/tmp/x"}"#)
        .expect("on-event 链失败");
}

#[test]
fn wasm_plugin_permission_denied_is_typed_error() {
    if !wasm_available() {
        eprintln!("[skip] 示例 WASM component 未编译");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let plugin = WasmPlugin::new(metadata_with_capabilities(vec![]), &example_wasm(), tmp.path())
        .expect("加载失败");
    // 错误类型链：guest Err(string) → runtime ExecutionFailed 包装（非 panic/trap）。
    let err = plugin.execute("host-read", serde_json::json!({ "path": "x" })).unwrap_err();
    assert!(
        matches!(err, PluginError::ExecutionFailed(ref message) if message.contains("缺少 read_project 权限")),
        "权限拒绝应作为 ExecutionFailed 语义错误透传，而非 trap: {err:?}"
    );
}
