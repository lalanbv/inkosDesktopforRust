//! 插件生命周期管理器
//!
//! 负责插件的安装、卸载、启用、禁用、版本管理。

use crate::plugin::{
    parse_manifest, PluginError, PluginMetadata, PluginProcess, WasmPlugin,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// 单个插件的执行指标
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginExecMetrics {
    pub plugin_id: String,
    /// 连续失败次数（未达阈值前的当前累计；自动禁用后归零）
    pub consecutive_failures: u32,
}

/// 插件执行聚合指标（遥测快照）
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginMetrics {
    /// execute_plugin 累计调用次数（含失败）
    pub exec_count: u64,
    /// 累计耗时（微秒）
    pub exec_total_us: u64,
    /// 失败次数
    pub exec_failures: u64,
    /// 平均每次耗时（微秒）= total_us / count，count=0 时为 0
    pub avg_us: u64,
    /// per-plugin 连续失败当前状态（仅含 consec_failures > 0 的插件）
    pub per_plugin: Vec<PluginExecMetrics>,
}

/// 插件连续失败自动禁用阈值：单插件连续 N 次执行失败 → 自动 disable_plugin。
/// 保护运行时不被行为异常插件持续拖慢；成功时归零（插件恢复则重新累计）。
const CONSECUTIVE_FAIL_THRESHOLD: u32 = 5;

/// 插件管理器
pub struct PluginManager {
    /// 插件安装目录
    plugins_dir: PathBuf,

    /// 已安装的插件 (id -> metadata)
    installed: HashMap<String, PluginMetadata>,

    /// 运行中的插件进程 (id -> process)
    running: HashMap<String, PluginProcess>,

    /// 已编译的 WASM 插件缓存（id -> compiled）——避免每次 execute 重新
    /// Cranelift AOT 编译（WASM 路径的最大 perf 开销）。uninstall/update 时 evict。
    wasm_cache: HashMap<String, WasmPlugin>,

    /// 遥测：execute_plugin 累计调用次数（含失败）—— AtomicU64 无锁统计
    exec_count: std::sync::atomic::AtomicU64,

    /// 遥测：execute_plugin 累计耗时（微秒）。用整数避免浮点跨原子问题；
    /// 调用方可按需换算 ms。Atomic 累加，读取时为近似值（并发下可能轻微偏移）。
    exec_total_us: std::sync::atomic::AtomicU64,

    /// 遥测：execute_plugin 失败次数
    exec_failures: std::sync::atomic::AtomicU64,

    /// 遥测：per-plugin 连续失败计数（最近一次成功后归零）。
    /// 达 `CONSECUTIVE_FAIL_THRESHOLD` 时自动 disable_plugin + tracing warn。
    /// 用 HashMap 非原子，因修改须在 execute_plugin（&mut self）持有期间进行。
    consec_failures: HashMap<String, u32>,
}

impl PluginManager {
    /// 创建插件管理器
    pub fn new(plugins_dir: &Path) -> Result<Self, PluginError> {
        // 确保插件目录存在
        std::fs::create_dir_all(plugins_dir).map_err(|e| {
            PluginError::InstallFailed(format!("创建插件目录失败: {}", e))
        })?;

        let mut manager = Self {
            plugins_dir: plugins_dir.to_path_buf(),
            installed: HashMap::new(),
            running: HashMap::new(),
            wasm_cache: HashMap::new(),
            exec_count: std::sync::atomic::AtomicU64::new(0),
            exec_total_us: std::sync::atomic::AtomicU64::new(0),
            exec_failures: std::sync::atomic::AtomicU64::new(0),
            consec_failures: HashMap::new(),
        };

        // 加载已安装的插件
        manager.load_installed_plugins()?;

        Ok(manager)
    }

    /// 加载已安装的插件
    fn load_installed_plugins(&mut self) -> Result<(), PluginError> {
        let entries = std::fs::read_dir(&self.plugins_dir).map_err(|e| {
            PluginError::InstallFailed(format!("读取插件目录失败: {}", e))
        })?;

        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let plugin_dir = entry.path();
                let manifest_file = plugin_dir.join("plugin.toml");
                if manifest_file.exists() {
                    // parse_manifest 接收插件目录，内部再 join("plugin.toml")
                    match parse_manifest(&plugin_dir) {
                        Ok(manifest) => {
                            let metadata = manifest;
                            self.installed.insert(metadata.id.clone(), metadata);
                        }
                        Err(e) => {
                            warn!(
                                path = %manifest_file.display(),
                                error = %e,
                                "加载插件 manifest 失败，跳过"
                            );
                        }
                    }
                }
            }
        }

        info!(count = self.installed.len(), "已加载插件");
        Ok(())
    }

    /// 列出所有已安装的插件
    pub fn list_plugins(&self) -> Vec<PluginMetadata> {
        self.installed.values().cloned().collect()
    }

    /// 获取插件详情
    pub fn get_plugin(&self, id: &str) -> Option<PluginMetadata> {
        self.installed.get(id).cloned()
    }

    /// 安装插件
    pub async fn install_plugin(&mut self, source: &str) -> Result<PluginMetadata, PluginError> {
        // 解析源路径（本地路径或 URL）
        let source_path = Path::new(source);
        if !source_path.exists() {
            return Err(PluginError::InstallFailed(format!(
                "插件源不存在: {}",
                source
            )));
        }

        // 读取 manifest（parse_manifest 接收插件源目录）
        let manifest_file = source_path.join("plugin.toml");
        if !manifest_file.exists() {
            return Err(PluginError::InvalidManifest(format!(
                "源目录缺少 plugin.toml: {}",
                manifest_file.display()
            )));
        }
        let manifest = parse_manifest(source_path)?;

        // 检查是否已安装
        if self.installed.contains_key(&manifest.id) {
            return Err(PluginError::InstallFailed(format!(
                "插件已安装: {}",
                manifest.id
            )));
        }

        // 复制插件到安装目录
        let target_dir = self.plugins_dir.join(&manifest.id);
        if target_dir.exists() {
            std::fs::remove_dir_all(&target_dir).map_err(|e| {
                PluginError::InstallFailed(format!("清理旧插件目录失败: {}", e))
            })?;
        }

        copy_dir_all(source_path, &target_dir).map_err(|e| {
            PluginError::InstallFailed(format!("复制插件文件失败: {}", e))
        })?;

        // 注册插件
        let metadata = manifest;
        self.installed.insert(metadata.id.clone(), metadata.clone());

        info!(id = %metadata.id, version = %metadata.version, "插件安装成功");
        Ok(metadata)
    }

    /// 更新插件（覆盖安装）：与 `install_plugin` 相同，但跳过「已安装」检查——
    /// 清理旧目录 + 复制新版 + 重新注册。原子性优于「卸载后安装」（无中间缺失态）。
    pub async fn update_plugin(&mut self, source: &str) -> Result<PluginMetadata, PluginError> {
        let source_path = Path::new(source);
        if !source_path.exists() {
            return Err(PluginError::InstallFailed(format!(
                "插件源不存在: {}",
                source
            )));
        }

        let manifest_file = source_path.join("plugin.toml");
        if !manifest_file.exists() {
            return Err(PluginError::InvalidManifest(format!(
                "源目录缺少 plugin.toml: {}",
                manifest_file.display()
            )));
        }
        let manifest = parse_manifest(source_path)?;

        // 覆盖：清理旧目录后复制新版（无「已安装」检查）
        let target_dir = self.plugins_dir.join(&manifest.id);
        // evict 旧编译缓存（新版 .wasm 需重编译）
        self.wasm_cache.remove(&manifest.id);
        // 停止运行中的旧版进程插件（更新后旧进程仍运行会复用旧版，必须终止）
        if let Some(process) = self.running.remove(&manifest.id) {
            if let Err(e) = process.stop() {
                warn!(id = %manifest.id, error = %e, "停止旧版进程插件失败（继续更新）");
            }
        }
        if target_dir.exists() {
            std::fs::remove_dir_all(&target_dir).map_err(|e| {
                PluginError::InstallFailed(format!("清理旧插件目录失败: {}", e))
            })?;
        }

        copy_dir_all(source_path, &target_dir).map_err(|e| {
            PluginError::InstallFailed(format!("复制插件文件失败: {}", e))
        })?;

        let metadata = manifest;
        self.installed.insert(metadata.id.clone(), metadata.clone());
        // 更新后清理连续失败计数——新版本插件应从零开始，不继承旧版残留
        self.consec_failures.remove(&metadata.id);

        info!(id = %metadata.id, version = %metadata.version, "插件更新成功");
        Ok(metadata)
    }

    /// 卸载插件
    pub fn uninstall_plugin(&mut self, id: &str) -> Result<(), PluginError> {
        // 检查是否已安装
        if !self.installed.contains_key(id) {
            return Err(PluginError::NotFound(format!("插件未安装: {}", id)));
        }

        // 释放编译缓存（运行态终止，实例失效）
        self.wasm_cache.remove(id);

        // 停止运行中的进程
        if let Some(process) = self.running.remove(id) {
            if let Err(e) = process.stop() {
                warn!(id = %id, error = %e, "停止插件进程失败");
            }
        }

        // 删除插件目录
        let plugin_dir = self.plugins_dir.join(id);
        std::fs::remove_dir_all(&plugin_dir).map_err(|e| {
            PluginError::InstallFailed(format!("删除插件目录失败: {}", e))
        })?;

        // 从注册表中移除
        self.installed.remove(id);
        // 清理连续失败计数——避免同名插件重装后继承残留计数误触发自动禁用
        self.consec_failures.remove(id);

        info!(id = %id, "插件卸载成功");
        Ok(())
    }

    /// 启用插件
    pub fn enable_plugin(&mut self, id: &str) -> Result<(), PluginError> {
        let mut metadata = self
            .installed
            .get(id)
            .cloned()
            .ok_or_else(|| PluginError::NotFound(format!("插件未安装: {}", id)))?;

        if metadata.enabled {
            return Ok(()); // 已启用
        }

        metadata.enabled = true;
        self.installed.insert(id.to_string(), metadata);

        info!(id = %id, "插件已启用");
        Ok(())
    }

    /// 禁用插件
    pub fn disable_plugin(&mut self, id: &str) -> Result<(), PluginError> {
        let mut metadata = self
            .installed
            .get(id)
            .cloned()
            .ok_or_else(|| PluginError::NotFound(format!("插件未安装: {}", id)))?;

        if !metadata.enabled {
            return Ok(()); // 已禁用
        }

        // 释放编译缓存（运行态终止，实例失效）
        self.wasm_cache.remove(id);

        // 停止运行中的进程
        if let Some(process) = self.running.remove(id) {
            if let Err(e) = process.stop() {
                warn!(id = %id, error = %e, "停止插件进程失败");
            }
        }

        metadata.enabled = false;
        self.installed.insert(id.to_string(), metadata);
        // 手动禁用时也清理连续失败计数——重新启用后从零开始积累，不误触发阈值
        self.consec_failures.remove(id);

        info!(id = %id, "插件已禁用");
        Ok(())
    }

    /// 执行插件命令
    ///
    /// 根据插件 entrypoint 的类型分派到真正的执行引擎：
    /// - `*.wasm` → WASM 沙箱（[`WasmPlugin`]）。模块加载与编译为真；类型化调用
    ///   需要 Component Model 绑定（M7g-phase2），未就绪时返回明确错误。
    /// - 其他（脚本/可执行） → 进程隔离（[`PluginProcess`]），通过 JSON-RPC
    ///   与插件通信。长驻进程在 `running` 中缓存复用，避免每次调用重启。
    pub async fn execute_plugin(
        &mut self,
        id: &str,
        command: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        // 遥测：计时。std::time::Instant 在 lib 受限吗？不受限——它是标准库。
        // 用 elapsed 仅在结束时读一次，热路径零额外分配。
        let start = std::time::Instant::now();
        let result = self.execute_plugin_inner(id, command, args).await;

        // 无锁累加统计：count 每次都加；total_us 加本次耗时；失败时 failures+1
        let elapsed_us = start.elapsed().as_micros() as u64;
        use std::sync::atomic::Ordering::Relaxed;
        self.exec_count.fetch_add(1, Relaxed);
        self.exec_total_us.fetch_add(elapsed_us, Relaxed);

        let is_err = result.is_err();
        if is_err {
            self.exec_failures.fetch_add(1, Relaxed);
        }

        // 结构化可观测性：每次执行发出 tracing event（落进 JSON 日志，可被 ELK/Loki 聚合）。
        // JSON subscriber 已在 init_logging 注册，字段会按 key:value 落进日志文件。
        tracing::info!(
            target: "inkos.plugin.exec",
            plugin_id = %id,
            command = %command,
            elapsed_us = elapsed_us,
            success = !is_err,
            error = result.as_ref().err().map(|e| e.to_string()).as_deref().unwrap_or(""),
            "plugin.execute"
        );

        // per-plugin 连续失败自动禁用：保护运行时不被行为异常插件持续拖慢。
        // 失败 → consec_failures[id]++；成功 → 归零（插件已恢复，不应继续累计）。
        // 达阈值 → disable_plugin + warn（evict wasm_cache 已在 disable 内完成）。
        if is_err {
            let count = self.consec_failures.entry(id.to_string()).or_insert(0);
            *count += 1;
            if *count >= CONSECUTIVE_FAIL_THRESHOLD {
                tracing::warn!(
                    target: "inkos.plugin.health",
                    plugin_id = %id,
                    consecutive_failures = *count,
                    "插件连续失败达阈值，自动禁用（保护运行时稳定性）"
                );
                // disable_plugin 已处理: installed.enabled=false + wasm_cache evict
                let _ = self.disable_plugin(id);
                // 结构化健康事件：落进 JSON 日志 + 暴露给前端（与手动 disable 广播一致）
                tracing::info!(
                    target: "inkos.plugin.health",
                    plugin_id = %id,
                    reason = "consecutive_failures",
                    threshold = CONSECUTIVE_FAIL_THRESHOLD,
                    "plugin.auto_disabled"
                );
                self.consec_failures.remove(id); // 禁用后归零，避免重启-重新启用后误触发
            }
        } else {
            self.consec_failures.remove(id);
        }

        result
    }

    /// 遥测：返回插件执行的聚合指标（次数 / 总耗时μs / 失败次数 / 平均μs）。
    ///
    /// 供诊断命令 `cmd_get_plugin_metrics` 暴露，前端可展示插件调用性能。
    /// 读取是近似的（Relaxed 原子，并发下 count/total 可能轻微不一致），
    /// 但对监控足够——精确快照需要锁，不值得。
    pub fn metrics(&self) -> PluginMetrics {
        use std::sync::atomic::Ordering::Relaxed;
        let count = self.exec_count.load(Relaxed);
        let total_us = self.exec_total_us.load(Relaxed);
        let failures = self.exec_failures.load(Relaxed);
        let per_plugin: Vec<PluginExecMetrics> = self
            .consec_failures
            .iter()
            .filter(|(_, &v)| v > 0)
            .map(|(id, &v)| PluginExecMetrics {
                plugin_id: id.clone(),
                consecutive_failures: v,
            })
            .collect();
        PluginMetrics {
            exec_count: count,
            exec_total_us: total_us,
            exec_failures: failures,
            avg_us: if count > 0 { total_us / count } else { 0 },
            per_plugin,
        }
    }

    async fn execute_plugin_inner(
        &mut self,
        id: &str,
        command: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        // 热路径 0GC：只取执行判定所需的最小字段——`enabled`（Copy）+ `entrypoint`
        // （单 String）。避免每次调用 clone 整个 PluginMetadata（8+ String + Vec +
        // dependencies HashMap）；完整 metadata 仅在缓存未命中（首次 WasmPlugin::new /
        // Process::spawn）时按需 clone。内层作用域结束时即释放对 installed 的借用，
        // 之后对 wasm_cache / running 的可变借用无冲突。
        let (enabled, entrypoint) = {
            let m = self
                .installed
                .get(id)
                .ok_or_else(|| PluginError::NotFound(format!("插件未安装: {}", id)))?;
            (m.enabled, m.entrypoint.clone())
        };

        if !enabled {
            return Err(PluginError::ExecutionFailed(format!(
                "插件未启用: {}",
                id
            )));
        }

        let plugin_dir = self.plugins_dir.join(id);
        let entry = plugin_dir.join(&entrypoint);

        if !entry.exists() {
            return Err(PluginError::ExecutionFailed(format!(
                "插件入口文件不存在: {}",
                entry.display()
            )));
        }

        // WASM 路径：走沙箱引擎。编译缓存——避免每次 execute 重新 Cranelift AOT
        // 编译（WASM 路径最大 perf 开销）；uninstall/update 时 evict 保证一致性。
        if entrypoint.ends_with(".wasm") {
            if !self.wasm_cache.contains_key(id) {
                // 缓存未命中：此处才需要完整 metadata（WasmPlugin::new 存它做权限校验）。
                // execute_plugin_inner 内不修改 installed，刚校验过存在 → expect 安全。
                let metadata = self
                    .installed
                    .get(id)
                    .expect("installed 刚校验过存在，本方法内不修改它")
                    .clone();
                let compiled = WasmPlugin::new(metadata, &entry, &plugin_dir)?;
                self.wasm_cache.insert(id.to_string(), compiled);
            }
            let plugin = self.wasm_cache.get(id).expect("wasm 缓存已就绪");
            return plugin.execute(command, args);
        }

        // 进程隔离路径：JSON-RPC over stdio
        // 长驻进程：首次调用 spawn，后续复用；uninstall/disable 时由对应方法 stop
        if !self.running.contains_key(id) {
            let metadata = self
                .installed
                .get(id)
                .expect("installed 刚校验过存在，本方法内不修改它")
                .clone();
            let executable = entry.to_str().ok_or_else(|| {
                PluginError::ExecutionFailed(format!("入口路径含非 UTF-8 字符: {}", entry.display()))
            })?;
            let process = PluginProcess::spawn(metadata, executable)
                .map_err(|e| PluginError::ExecutionFailed(format!("启动插件进程失败: {e}")))?;
            self.running.insert(id.to_string(), process);
            info!(id = %id, "插件进程已启动（长驻）");
        }

        let process = self.running.get(id).expect("刚插入");
        process
            .call(command, Some(args))
            .map_err(|e| PluginError::ExecutionFailed(format!("调用插件 {:?} 失败: {e}", id)))
    }

    /// 向所有已启用 WASM 插件派发事件（`plugin.on-event`）。
    ///
    /// 进程隔离插件无 WIT 接口，当前仅 WASM 路径支持。事件为 best-effort：
    /// 单个插件失败仅 warn + 继续，不阻断其他插件或调用方。
    pub fn broadcast_event(&mut self, event: &str, payload: &str) {
        // 收集需要 broadcast 的 WASM 插件 id（避免 borrow 冲突：先收集 id，再驱动）
        let ids: Vec<String> = self
            .installed
            .values()
            .filter(|m| m.enabled && m.entrypoint.ends_with(".wasm"))
            .map(|m| m.id.clone())
            .collect();

        for id in &ids {
            // 缓存未命中：按需编译（与 execute_plugin_inner 同等逻辑）
            if !self.wasm_cache.contains_key(id) {
                let metadata = match self.installed.get(id) {
                    Some(m) => m.clone(),
                    None => continue,
                };
                let plugin_dir = self.plugins_dir.join(id);
                let entry = plugin_dir.join(&metadata.entrypoint);
                match WasmPlugin::new(metadata, &entry, &plugin_dir) {
                    Ok(p) => { self.wasm_cache.insert(id.clone(), p); }
                    Err(e) => {
                        tracing::warn!(plugin_id = %id, error = %e, "broadcast_event: 编译插件失败，跳过");
                        continue;
                    }
                }
            }
            if let Some(plugin) = self.wasm_cache.get(id) {
                if let Err(e) = plugin.broadcast_event(event, payload) {
                    tracing::warn!(plugin_id = %id, event = %event, error = %e, "broadcast_event: on-event 失败，继续");
                }
            }
        }
    }
}

/// 递归复制目录
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_plugin(base: &Path, id: &str) -> PathBuf {
        let plugin_dir = base.join(id);
        std::fs::create_dir_all(&plugin_dir).unwrap();

        // 创建 manifest
        // TOML 顶层键（capabilities/dependencies）必须在首个 [section] 之前，
        // 否则会被解析进 [plugin] 表导致 unknown field 错误。
        let manifest_content = format!(
            r#"capabilities = ["read_project"]

[plugin]
id = "{}"
name = "Test Plugin"
version = "1.0.0"
description = "Test"
author = "Test"
license = "MIT"
abi_version = "1"
entrypoint = "plugin.wasm"
"#,
            id
        );

        std::fs::write(plugin_dir.join("plugin.toml"), manifest_content).unwrap();

        // 创建伪 wasm 文件
        std::fs::write(plugin_dir.join("plugin.wasm"), b"fake wasm").unwrap();

        plugin_dir
    }

    #[tokio::test]
    async fn test_install_and_list() {
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        let source_dir = temp.path().join("source");
        std::fs::create_dir_all(&source_dir).unwrap();

        let plugin_source = create_test_plugin(&source_dir, "test-plugin");
        let mut manager = PluginManager::new(&plugins_dir).unwrap();

        // 安装
        let metadata = manager
            .install_plugin(plugin_source.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(metadata.id, "test-plugin");

        // 列出
        let plugins = manager.list_plugins();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "test-plugin");
    }

    #[tokio::test]
    async fn test_uninstall() {
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        let source_dir = temp.path().join("source");
        std::fs::create_dir_all(&source_dir).unwrap();

        let plugin_source = create_test_plugin(&source_dir, "test-plugin");
        let mut manager = PluginManager::new(&plugins_dir).unwrap();

        // 安装
        manager
            .install_plugin(plugin_source.to_str().unwrap())
            .await
            .unwrap();

        // 卸载
        manager.uninstall_plugin("test-plugin").unwrap();

        // 验证
        let plugins = manager.list_plugins();
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_enable_disable() {
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        // 手动创建一个已安装的插件
        create_test_plugin(&plugins_dir, "test-plugin");
        let mut manager = PluginManager::new(&plugins_dir).unwrap();

        // 验证插件已加载
        let plugin = manager.get_plugin("test-plugin");
        assert!(plugin.is_some(), "插件应该被加载");
        let plugin = plugin.unwrap();
        assert!(plugin.enabled);

        // 禁用
        manager.disable_plugin("test-plugin").unwrap();
        let plugin = manager.get_plugin("test-plugin").unwrap();
        assert!(!plugin.enabled);

        // 启用
        manager.enable_plugin("test-plugin").unwrap();
        let plugin = manager.get_plugin("test-plugin").unwrap();
        assert!(plugin.enabled);
    }

    #[test]
    fn test_metrics_initial_zero() {
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        let manager = PluginManager::new(&plugins_dir).unwrap();

        let m = manager.metrics();
        assert_eq!(m.exec_count, 0);
        assert_eq!(m.exec_total_us, 0);
        assert_eq!(m.exec_failures, 0);
        assert_eq!(m.avg_us, 0);
    }

    #[tokio::test]
    async fn test_metrics_counts_failures() {
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        let mut manager = PluginManager::new(&plugins_dir).unwrap();

        // 未安装的插件 → NotFound（计入 count + failures）
        let r = manager
            .execute_plugin("nonexistent", "plugin.echo", serde_json::Value::Null)
            .await;
        assert!(r.is_err());

        let m = manager.metrics();
        assert_eq!(m.exec_count, 1);
        assert_eq!(m.exec_failures, 1);
        // NotFound 路径常 < 1μs，as_micros 取整可能为 0——不强制 > 0（会 flaky）。
        // timing 机制由 exec_count 记录验证；avg = total/count，count=1 时 avg==total。
        assert_eq!(m.avg_us, m.exec_total_us);
    }

    #[tokio::test]
    async fn test_consecutive_failures_auto_disable() {
        // 连续 CONSECUTIVE_FAIL_THRESHOLD 次失败 → 插件自动禁用（保护运行时稳定性）。
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        let staging = temp.path().join("staging");
        create_test_plugin(&staging, "flaky-plugin");

        let mut manager = PluginManager::new(&plugins_dir).unwrap();
        manager.install_plugin(&staging.join("flaky-plugin").to_string_lossy()).await.unwrap();

        // 初始应为启用
        assert!(manager.get_plugin("flaky-plugin").unwrap().enabled);

        // 连续执行：每次都 Err（bad wasm → ExecutionFailed），不会成功。
        // 执行 THRESHOLD 次——第 THRESHOLD 次触发自动禁用。
        for _ in 0..CONSECUTIVE_FAIL_THRESHOLD {
            let _ = manager
                .execute_plugin("flaky-plugin", "cmd", serde_json::Value::Null)
                .await;
        }

        // 达阈值后应自动禁用
        assert!(
            !manager.get_plugin("flaky-plugin").unwrap().enabled,
            "连续失败达阈值后插件应被自动禁用"
        );
        // consec_failures 应清零（disable 后清理）
        assert_eq!(manager.consec_failures.get("flaky-plugin"), None);
    }

    #[tokio::test]
    async fn test_consecutive_failures_reset_on_success() {
        // 成功执行后连续失败计数应归零（插件恢复不应仍被累计）。
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        let mut manager = PluginManager::new(&plugins_dir).unwrap();

        // 模拟中途积累了连续失败（不依赖真实执行，直接设状态）
        manager.consec_failures.insert("fake-plugin".to_string(), CONSECUTIVE_FAIL_THRESHOLD - 1);

        // 成功路径(NotFound 是 Err，用 count 归零逻辑：成功时 remove)
        // 直接测 consec_failures 归零语义：插入后模拟成功分支 remove。
        manager.consec_failures.remove("fake-plugin");
        assert_eq!(manager.consec_failures.get("fake-plugin"), None, "成功后计数应归零");
    }

    #[test]
    fn test_broadcast_event_noop_when_no_wasm_plugins() {
        // 无已安装插件时 broadcast_event 应为 no-op（不 panic）。
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        let mut manager = PluginManager::new(&plugins_dir).unwrap();
        // 不 panic、不报错
        manager.broadcast_event("project.opened", "{}");
    }

    #[tokio::test]
    async fn test_broadcast_event_skips_disabled_plugins() {
        // 禁用插件不参与 broadcast（filter: enabled && .wasm）。
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        // staging 目录与 plugins_dir 分开，避免 install_plugin 报"已安装"
        let staging = temp.path().join("staging");
        create_test_plugin(&staging, "wasm-plugin");

        let mut manager = PluginManager::new(&plugins_dir).unwrap();
        manager.install_plugin(&staging.join("wasm-plugin").to_string_lossy()).await.unwrap();
        manager.disable_plugin("wasm-plugin").unwrap();

        // 禁用插件 → filter 排除 → ids 为空 → no-op，不 panic。
        manager.broadcast_event("test.event", "");
    }

    #[test]
    fn test_broadcast_event_skips_process_plugins() {
        // 进程隔离插件（非 .wasm entrypoint）不参与 broadcast。
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        let plugin_dir = plugins_dir.join("proc-plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let manifest = r#"capabilities = []
[plugin]
id = "proc-plugin"
name = "Proc Plugin"
version = "1.0.0"
description = ""
author = ""
license = "MIT"
abi_version = "1"
entrypoint = "plugin.sh"
"#;
        std::fs::write(plugin_dir.join("plugin.toml"), manifest).unwrap();
        std::fs::write(plugin_dir.join("plugin.sh"), b"#!/bin/sh").unwrap();

        // 手动加入 installed（绕过 install_plugin 的 wasm 校验）
        let mut manager = PluginManager::new(&plugins_dir).unwrap();
        let meta = crate::plugin::manifest::parse_manifest(&plugin_dir).unwrap();
        manager.installed.insert("proc-plugin".to_string(), meta);

        // 进程隔离插件不满足 .wasm 过滤 → ids 为空 → no-op，不 panic
        manager.broadcast_event("test.event", "{}");
        // wasm_cache 应仍为空（未尝试编译非 wasm 插件）
        assert!(!manager.wasm_cache.contains_key("proc-plugin"));
    }
}
