//! 插件生命周期管理器
//!
//! 负责插件的安装、卸载、启用、禁用、版本管理。

use crate::plugin::{
    parse_manifest, PluginError, PluginMetadata, PluginProcess, WasmPlugin,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// 插件管理器
pub struct PluginManager {
    /// 插件安装目录
    plugins_dir: PathBuf,

    /// 已安装的插件 (id -> metadata)
    installed: HashMap<String, PluginMetadata>,

    /// 运行中的插件进程 (id -> process)
    running: HashMap<String, PluginProcess>,
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

    /// 卸载插件
    pub fn uninstall_plugin(&mut self, id: &str) -> Result<(), PluginError> {
        // 检查是否已安装
        if !self.installed.contains_key(id) {
            return Err(PluginError::NotFound(format!("插件未安装: {}", id)));
        }

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

        // 停止运行中的进程
        if let Some(process) = self.running.remove(id) {
            if let Err(e) = process.stop() {
                warn!(id = %id, error = %e, "停止插件进程失败");
            }
        }

        metadata.enabled = false;
        self.installed.insert(id.to_string(), metadata);

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
        let metadata = self
            .installed
            .get(id)
            .cloned()
            .ok_or_else(|| PluginError::NotFound(format!("插件未安装: {}", id)))?;

        if !metadata.enabled {
            return Err(PluginError::ExecutionFailed(format!(
                "插件未启用: {}",
                id
            )));
        }

        let plugin_dir = self.plugins_dir.join(id);
        let entry = plugin_dir.join(&metadata.entrypoint);

        if !entry.exists() {
            return Err(PluginError::ExecutionFailed(format!(
                "插件入口文件不存在: {}",
                entry.display()
            )));
        }

        // WASM 路径：走沙箱引擎
        if metadata.entrypoint.ends_with(".wasm") {
            let plugin = WasmPlugin::new(metadata, &entry, &plugin_dir)?;
            // WasmPlugin::execute 当前返回明确的「需要 component model 绑定」错误
            return plugin.execute(command, args);
        }

        // 进程隔离路径：JSON-RPC over stdio
        // 长驻进程：首次调用 spawn，后续复用；uninstall/disable 时由对应方法 stop
        if !self.running.contains_key(id) {
            let executable = entry.to_str().ok_or_else(|| {
                PluginError::ExecutionFailed(format!("入口路径含非 UTF-8 字符: {}", entry.display()))
            })?;
            let process = PluginProcess::spawn(metadata.clone(), executable)
                .map_err(|e| PluginError::ExecutionFailed(format!("启动插件进程失败: {e}")))?;
            self.running.insert(id.to_string(), process);
            info!(id = %id, "插件进程已启动（长驻）");
        }

        let process = self.running.get(id).expect("刚插入");
        process
            .call(command, Some(args))
            .map_err(|e| PluginError::ExecutionFailed(format!("调用插件 {:?} 失败: {e}", id)))
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
}
