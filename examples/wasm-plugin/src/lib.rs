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
        match command.as_str() {
            "echo" => Ok(args),                       // 原样回显
            "ping" => Ok(r#"{"pong":true}"#.to_string()),
            _ => Err(format!("未知命令: {command}")),
        }
    }

    fn on_event(_event: String, _payload: String) {
        // 可选事件钩子（项目打开、文件保存等）。无返回值，可空实现。
    }
}

// 导出 world 实现（wit-bindgen 宏，把 ExamplePlugin 注册为 inkos-plugin world 的实现）
export!(ExamplePlugin);
