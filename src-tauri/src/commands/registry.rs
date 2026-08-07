//! 插件注册表 Tauri 命令
//!
//! 拉取 / 安装 / 更新 / 检查更新 注册表插件。注册表经 Ed25519 验签（见 `plugin::registry`）。

use crate::commands::AppState;
use inkos_desktop::plugin::{
    registry::{self, RegistryEntry, UpdateInfo},
    PluginMetadata, PluginState, HOST_ABI_VERSION,
};
use semver::Version;
use std::time::Duration;

/// 读配置的 registry 来源（url + pubkey + 超时）并构造 HTTP client。多命令共用，
/// 避免 config 读取 + client 构造逻辑重复。
async fn registry_source_and_client(
    config_state: &AppState,
) -> Result<(String, Vec<u8>, reqwest::Client), String> {
    // 单次取锁提取来源参数后立即释放——不在持 config 锁时做网络 I/O（同 update_config H1）
    let (url, pubkey_hex, timeout_secs) = {
        let mut mgr = config_state.config.lock().await;
        let cfg = mgr.merged();
        match (cfg.registry.url.clone(), cfg.registry.pubkey.clone()) {
            (Some(u), Some(p)) => (u, p, cfg.network.timeout_seconds),
            _ => {
                return Err(
                    "插件注册表未配置（registry.url 与 registry.pubkey 需同时在配置中设置）"
                        .to_string(),
                );
            }
        }
    };
    let pubkey = registry::decode_hex(&pubkey_hex)
        .map_err(|e| format!("注册表 pubkey 解码失败: {}", e))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(1) as u64))
        .build()
        .map_err(|e| format!("构造 HTTP client 失败: {}", e))?;
    Ok((url, pubkey, client))
}

/// 下载 + 验签 + 安全解压 bundle 到临时目录。install/update 命令共用（安全关键
/// 逻辑单点定义，避免双处维护漂移）。返回的 TempDir 由调用方持有，install/update
/// 完成文件复制后才 drop。
async fn download_verify_extract(
    entry: &RegistryEntry,
    pubkey: &[u8],
    client: &reqwest::Client,
) -> Result<tempfile::TempDir, String> {
    entry
        .validate()
        .map_err(|e| format!("注册表条目字段非法: {e}"))?;
    let bundle = registry::http_fetch(client, &entry.download_url, registry::MAX_BUNDLE_BYTES)
        .await
        .map_err(|e| format!("下载插件包失败: {}", e))?;
    entry
        .verify_bundle(&bundle, pubkey)
        .map_err(|e| format!("插件包校验失败（可能被篡改）: {}", e))?;
    let temp = tempfile::tempdir().map_err(|e| format!("创建临时目录失败: {}", e))?;
    registry::safe_extract_tar_gz(&bundle, temp.path())
        .map_err(|e| format!("解压插件包失败: {}", e))?;
    Ok(temp)
}

/// 拉取插件注册表，按宿主兼容性过滤后返回可装条目。
#[tauri::command]
pub async fn cmd_fetch_plugin_registry(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<RegistryEntry>, String> {
    let (url, pubkey, client) = registry_source_and_client(&state).await?;
    let index = registry::fetch_registry(&url, &pubkey, |u| {
        // own url（future 不借用闭包参数 u 的生命周期）；client 为外层长效借用
        let url_owned = u.to_string();
        let client = &client;
        async move {
            registry::http_fetch(client, &url_owned, registry::MAX_REGISTRY_BYTES).await
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    let host_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("CARGO_PKG_VERSION 须为合法 semver");
    // 多版本去重：每 id 取最高兼容版本（browse 一个插件只展示最新可用版）
    let compatible: Vec<RegistryEntry> = index
        .latest_compatible(&host_version, HOST_ABI_VERSION)
        .into_iter()
        .cloned()
        .collect();
    Ok(compatible)
}

/// 从注册表条目安装插件：下载→验签→安全解压→`install_plugin`（拒绝已安装）。
#[tauri::command]
pub async fn cmd_install_from_registry(
    entry: RegistryEntry,
    config_state: tauri::State<'_, AppState>,
    plugin_state: tauri::State<'_, PluginState>,
) -> Result<PluginMetadata, String> {
    let (_url, pubkey, client) = registry_source_and_client(&config_state).await?;
    let temp = download_verify_extract(&entry, &pubkey, &client).await?;
    let source = temp.path().to_string_lossy().to_string();
    let mut manager = plugin_state.manager.lock().await;
    manager
        .install_plugin(&source)
        .await
        .map_err(|e| format!("安装失败: {}", e))
}

/// 从注册表条目更新插件：下载→验签→安全解压→`update_plugin`（覆盖已安装，无中间缺失态）。
#[tauri::command]
pub async fn cmd_update_plugin_from_registry(
    entry: RegistryEntry,
    config_state: tauri::State<'_, AppState>,
    plugin_state: tauri::State<'_, PluginState>,
) -> Result<PluginMetadata, String> {
    let (_url, pubkey, client) = registry_source_and_client(&config_state).await?;
    let temp = download_verify_extract(&entry, &pubkey, &client).await?;
    let source = temp.path().to_string_lossy().to_string();
    let mut manager = plugin_state.manager.lock().await;
    manager
        .update_plugin(&source)
        .await
        .map_err(|e| format!("更新失败: {}", e))
}

/// 检查已装插件的更新：拉取注册表 → 与已装版本 semver 比较 → 返回有更新的列表。
#[tauri::command]
pub async fn cmd_check_plugin_updates(
    config_state: tauri::State<'_, AppState>,
    plugin_state: tauri::State<'_, PluginState>,
) -> Result<Vec<UpdateInfo>, String> {
    let (url, pubkey, client) = registry_source_and_client(&config_state).await?;
    let index = registry::fetch_registry(&url, &pubkey, |u| {
        let url_owned = u.to_string();
        let client = &client;
        async move {
            registry::http_fetch(client, &url_owned, registry::MAX_REGISTRY_BYTES).await
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    let installed: Vec<(String, String)> = {
        let manager = plugin_state.manager.lock().await;
        manager
            .list_plugins()
            .into_iter()
            .map(|m| (m.id, m.version))
            .collect()
    };
    Ok(registry::check_updates(&installed, &index))
}
