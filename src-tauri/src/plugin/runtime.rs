//! WASM Runtime - 插件执行引擎骨架
//!
//! 基于 Wasmtime 提供 WASM 模块的加载与资源受限的执行环境。
//!
//! ## 当前状态（M7g）
//!
//! 已实现：Engine 初始化（Component Model + Fuel + Epoch 超时）、模块编译加载、
//! WASI 上下文构建、沙箱 Store 创建。这些是真正运行的——插件 .wasm 会被编译并
//! 通过 Engine 校验。
//!
//! 未实现：基于 `wit` IDL + `wasmtime::component::bindgen!` 的宿主-插件接口绑定。
//! 接口契约已定义在 `wit/inkos.wit`（host + plugin interface + inkos-plugin world）。
//! Component Model 的类型化调用需要为该契约生成绑定代码 + 实现链接器，这是一个独立的
//! 大工程（见 wit/inkos.wit 顶部「完整执行路径」）。在绑定就绪前，[`WasmPlugin::execute`]
//! 返回明确的 [`PluginError::ExecutionFailed`]，而不是假装执行——这样上层不会
//! 拿到静默错误的"假结果"。
//!
//! 在 WASM 绑定就绪前，插件的**真正可执行路径是进程隔离**（见 [`crate::plugin::process`]）：
//! `PluginManager::execute_plugin` 对带可执行 entrypoint 的插件通过 `PluginProcess`
//! 走 JSON-RPC，立即可用。

use super::host_api::HostContext;
use super::types::{PluginError, PluginMetadata};
use std::path::Path;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::*;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

// Phase 6.3：用 wit 契约生成宿主绑定（InkosPlugin 实例类型 + Host trait）。
// 编译期解析 wit/inkos.wit，生成类型化接口——这是 Component Model 完整执行的基础。
// path 相对 crate root（src-tauri/）。
wasmtime::component::bindgen!({
    path: "wit",
    world: "inkos-plugin",
});

/// 默认 fuel 上限：10M 条指令。
/// 足够典型插件处理一次格式化/转换；死循环会在 ~毫秒级被中断。
const DEFAULT_FUEL: u64 = 10_000_000;

/// 默认 epoch 超时：插件执行最多跑这么多轮 epoch tick。
const DEFAULT_EPOCH_DEADLINE: u64 = 10;

/// WASM 插件实例（已编译，可复用）
///
/// `Engine`、`Component`、`Linker` 在多次 `execute` 间复用，避免重复编译与
/// 重复注册 host imports；`Store` 每次调用新建，保证插件状态隔离（一次执行 =
/// 一个干净的实例）。Linker 是实例化模板（不持 Store 状态），跨实例化复用安全。
pub struct WasmPlugin {
    /// 插件元数据
    metadata: PluginMetadata,

    /// Wasmtime Engine（线程安全，跨调用共享）
    engine: Engine,

    /// 已编译的 Component（复用编译产物，execute 时实例化）
    component: Component,

    /// Host 上下文（权限检查；每次 execute clone 一份给独立 Store）
    host_context: HostContext,

    /// 预构建的 Linker（WASI + host imports 注册一次，execute 复用——高性能）
    linker: Linker<PluginState>,
}

/// 每次执行所依附的 WASI + Host 状态
struct PluginState {
    wasi: WasiCtx,
    table: ResourceTable,
    /// Host 上下文。为未来通过 WASI host imports 把 read_file/exec_command 等
    /// 暴露给 wasm 组件预留——届时 host functions 会从这个字段取权限校验器。
    #[allow(dead_code)]
    host: HostContext,
}

impl WasiView for PluginState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

// Phase 6.3：实现 bindgen 生成的 Host trait（对应 wit 的 host import interface）。
// 把插件对 host 能力的调用委托给 HostContext（复用 capability 校验 + 路径沙箱），
// 这样 WASM 插件与进程隔离插件走同一套权限模型，而非另造。
//
// 路径 inkos::plugin::host 由 wit 的 `package inkos:plugin; interface host` 决定：
// wasmtime bindgen 按 namespace::package::interface 组织 mod。
impl inkos::plugin::host::Host for PluginState {
    fn read_file(&mut self, path: String) -> Result<String, String> {
        self.host
            .read_file(&path)
            .map(|r| r.content)
            .map_err(|e| e.to_string())
    }

    fn write_file(&mut self, path: String, content: String) -> Result<(), String> {
        self.host
            .write_file(&path, &content)
            .map_err(|e| e.to_string())
            .map(|_| ())
    }

    fn list_dir(&mut self, path: String) -> Result<Vec<String>, String> {
        self.host
            .list_dir(&path)
            .map(|r| r.entries)
            .map_err(|e| e.to_string())
    }

    fn http_get(&mut self, url: String) -> Result<String, String> {
        self.host.http_get(&url).map_err(|e| e.to_string())
    }

    fn log(&mut self, level: String, message: String) {
        match level.as_str() {
            "error" => tracing::error!(plugin_log = %message),
            "warn" => tracing::warn!(plugin_log = %message),
            "info" => tracing::info!(plugin_log = %message),
            "debug" => tracing::debug!(plugin_log = %message),
            _ => tracing::trace!(plugin_log = %message),
        }
    }
}

impl WasmPlugin {
    /// 加载并编译插件
    ///
    /// 会立即编译 .wasm（Cranelift AOT）；编译失败（坏文件、不合法 wasm）
    /// 在此处就报错，而不是等到第一次 execute。
    pub fn new(
        metadata: PluginMetadata,
        wasm_path: &Path,
        work_dir: &Path,
    ) -> Result<Self, PluginError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.cranelift_opt_level(OptLevel::Speed);
        config.consume_fuel(true);
        config.epoch_interruption(true);

        let engine = Engine::new(&config)
            .map_err(|e| PluginError::ExecutionFailed(format!("创建 Wasmtime Engine 失败: {e}")))?;

        let component = Component::from_file(&engine, wasm_path).map_err(|e| {
            tracing::error!(path = %wasm_path.display(), error = %e, "编译 WASM Component 失败");
            PluginError::ExecutionFailed(format!("编译 WASM Component 失败: {e}"))
        })?;

        tracing::info!(
            plugin_id = %metadata.id,
            path = %wasm_path.display(),
            "WASM Component 加载成功"
        );

        // Linker 一次性构建并复用（高性能：避免每次 execute 重建 + 重注册 WASI/host imports）。
        // WASI 注册必需：component（wasm32-wasip2 target）默认依赖 wasi:io/poll 等，
        // 不注册会在实例化时报 "imports not found in linker"。
        let mut linker = Linker::<PluginState>::new(&engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)
            .map_err(|e| PluginError::ExecutionFailed(format!("注册 WASI 失败: {e}")))?;
        InkosPlugin::add_to_linker(&mut linker, |state: &mut PluginState| state)
            .map_err(|e| PluginError::ExecutionFailed(format!("add_to_linker 失败: {e}")))?;

        let host_context = HostContext::new(metadata.clone(), work_dir.to_path_buf());

        Ok(Self {
            metadata,
            engine,
            component,
            host_context,
            linker,
        })
    }

    /// 执行插件命令（Component Model 类型化调用）
    ///
    /// 链路：Linker 注册 host imports（Host trait 委托 HostContext）→
    /// 预实例化 Component → 调用 export 的 `plugin.invoke(command, args_json)` →
    /// 反序列化 JSON 结果。
    pub fn execute(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        tracing::debug!(plugin_id = %self.metadata.id, %command, "执行 WASM Component invoke");
        let args_str = serde_json::to_string(&args)
            .map_err(|e| PluginError::ExecutionFailed(format!("序列化参数失败: {e}")))?;

        let mut store = self.create_store()?;

        let bindings = InkosPlugin::instantiate(&mut store, &self.component, &self.linker)
            .map_err(|e| PluginError::ExecutionFailed(format!("实例化 Component 失败: {e}")))?;

        // call_invoke 返回 Result<Result<String, String>, wasmtime error>：
        // 外层是 wasmtime trap / 资源耗尽，内层是插件语义的 result<string,string>
        let inner = bindings
            .inkos_plugin_plugin()
            .call_invoke(&mut store, command, &args_str)
            .map_err(|e| PluginError::ExecutionFailed(format!("调用 invoke 失败: {e}")))?;
        let result_str = inner
            .map_err(|e| PluginError::ExecutionFailed(format!("插件 invoke 返回错误: {e}")))?;

        let result: serde_json::Value = serde_json::from_str(&result_str)
            .map_err(|e| PluginError::ExecutionFailed(format!("结果反序列化失败: {e}")))?;
        Ok(result)
    }

    /// 创建带资源限制的 Store
    fn create_store(&self) -> Result<Store<PluginState>, PluginError> {
        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .build();

        let state = PluginState {
            wasi,
            table: ResourceTable::new(),
            host: self.host_context.clone(),
        };

        let mut store = Store::new(&self.engine, state);
        store
            .set_fuel(DEFAULT_FUEL)
            .map_err(|e| PluginError::ExecutionFailed(format!("设置 fuel 限制失败: {e}")))?;
        store.set_epoch_deadline(DEFAULT_EPOCH_DEADLINE);
        self.engine.increment_epoch();
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn test_metadata() -> PluginMetadata {
        PluginMetadata {
            id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            author: "Test".to_string(),
            homepage: None,
            license: "MIT".to_string(),
            abi_version: "1".to_string(),
            capabilities: vec![],
            entrypoint: "plugin.wasm".to_string(),
            dependencies: HashMap::new(),
            enabled: true,
        }
    }

    #[test]
    fn test_load_nonexistent_wasm_fails() {
        let temp = TempDir::new().unwrap();
        let result = WasmPlugin::new(
            test_metadata(),
            &temp.path().join("plugin.wasm"),
            temp.path(),
        );
        // 文件不存在 → 编译阶段就报错
        assert!(result.is_err());
    }

    #[test]
    fn test_load_invalid_wasm_fails() {
        let temp = TempDir::new().unwrap();
        let wasm_path = temp.path().join("plugin.wasm");
        // 写入垃圾内容，Module::from_file 应当拒绝
        std::fs::write(&wasm_path, b"not a real wasm module").unwrap();

        let result = WasmPlugin::new(test_metadata(), &wasm_path, temp.path());
        assert!(result.is_err());
    }
}
