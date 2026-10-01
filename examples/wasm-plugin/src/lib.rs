//! inkos WASM 插件示例（Phase 6.3 SDK 脚手架）
//!
//! 实现 wit/inkos.wit 的 plugin interface（init/invoke/on-event）。
//! 宿主（inkosDesktop，src-tauri/src/plugin/runtime.rs）通过 Component Model
//! 类型化调用本插件——宿主侧绑定由 `wasmtime::component::bindgen!` 生成
//! （同一份 wit 契约），本插件侧绑定由 `wit_bindgen::generate!` 生成。
//!
//! 构建（需 wasm32-wasip2 target，Node SEA 之外的强隔离执行路径）：
//!   rustup target add wasm32-wasip2
//!   cargo build --target wasm32-wasip2 --release
//!   # 产物：target/wasm32-wasip2/release/inkos_example_plugin.wasm
//!
//! 安装：把 .wasm 放到插件目录（entrypoint = "plugin.wasm"），见
//! src-tauri/examples/plugins/hello-plugin/plugin.toml 格式。

// 从 wit/ 生成 guest 绑定（imports 的 host interface 自动可用，exports 需实现）
wit_bindgen::generate!({
    path: "wit",
    world: "inkos-plugin",
});

/// 插件实现。export 的 plugin interface 对应的 Guest trait 路径由 wit-bindgen
/// 按 wit 结构生成（exports::<package>::<interface>::Guest，此处即
/// exports::inkos::plugin::plugin::Guest）。实现后用 export! 导出 world。
struct ExamplePlugin;

impl exports::inkos::plugin::plugin::Guest for ExamplePlugin {
    fn init(_config: String) -> Result<(), String> {
        // config 来自宿主（插件初始化参数 JSON）。失败则插件不可用。
        Ok(())
    }

    fn invoke(command: String, args: String) -> Result<String, String> {
        // 核心调用：command + args(JSON 字符串) → result(JSON 字符串)。
        // 与进程隔离插件的 JSON-RPC 方法语义一致，便于双引擎统一上层 API。
        // host 能力调用（569 号端到端面）：guest→host import→Host trait→
        // HostContext capability 校验→fs/进程，每次调用受权限门。
        match command.as_str() {
            "echo" => Ok(args),                       // 原样回显
            "ping" => Ok(r#"{"pong":true}"#.to_string()),
            "host-read" => {
                let path = serde_json::from_str::<serde_json::Value>(&args)
                    .ok()
                    .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(String::from))
                    .ok_or_else(|| "args 需 {\"path\": string}".to_string())?;
                let content = inkos::plugin::host::read_file(&path)?;
                Ok(format!(r#"{{"content":{}}}"#, serde_json::to_string(&content).unwrap_or_default()))
            }
            "host-exec" => {
                let parsed: serde_json::Value = serde_json::from_str(&args)
                    .map_err(|e| format!("args 非法 JSON: {e}"))?;
                let command = parsed.get("command").and_then(|c| c.as_str())
                    .ok_or_else(|| "args 需 {\"command\": string}".to_string())?;
                let empty = Vec::new();
                let cmd_args: Vec<String> = parsed.get("args").and_then(|a| a.as_array())
                    .map(|list| list.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or(empty);
                let result = inkos::plugin::host::exec_command(command, &cmd_args)?;
                Ok(format!(
                    r#"{{"stdout":{},"stderr":{},"exitCode":{}}}"#,
                    serde_json::to_string(&result.stdout).unwrap_or_default(),
                    serde_json::to_string(&result.stderr).unwrap_or_default(),
                    result.exit_code,
                ))
            }
            "host-log" => {
                let parsed: serde_json::Value = serde_json::from_str(&args)
                    .map_err(|e| format!("args 非法 JSON: {e}"))?;
                let level = parsed.get("level").and_then(|l| l.as_str()).unwrap_or("info");
                let message = parsed.get("message").and_then(|m| m.as_str()).unwrap_or("");
                inkos::plugin::host::log(level, message);
                Ok(r#"{"logged":true}"#.to_string())
            }
            _ => Err(format!("未知命令: {command}")),
        }
    }

    fn on_event(event: String, payload: String) {
        // 事件钩子经 host.log 观测（on-event→host 链路端到端可见）。
        inkos::plugin::host::log("info", &format!("on-event: {event} {payload}"));
    }
}

// 导出 world 实现（wit-bindgen 宏，把 ExamplePlugin 注册为 inkos-plugin world 的实现）
export!(ExamplePlugin);
