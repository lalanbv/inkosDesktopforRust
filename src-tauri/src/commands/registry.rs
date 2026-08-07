//! 插件注册表 Tauri 命令
//!
//! 拉取配置的注册表，按宿主兼容性过滤后返回可装条目。前端（settings 面板）据此
//! 展示插件市场。注册表经 Ed25519 验签（见 `plugin::registry::fetch_registry`）。

use crate::commands::AppState;
use inkos_desktop::plugin::{
    registry::{self, RegistryEntry},
    HOST_ABI_VERSION,
};
use semver::Version;
use std::time::Duration;

/// 拉取插件注册表，按宿主兼容性过滤后返回可装条目。
///
/// 需配置 `registry.url` + `registry.pubkey`（见 `PluginRegistryConfig`）；未配置则
/// `Err`（前端据此提示去配置）。流式超时取自 `network.timeout_seconds`。
#[tauri::command]
pub async fn cmd_fetch_plugin_registry(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<RegistryEntry>, String> {
    // 单次取锁提取 url/pubkey/timeout 后立即释放——不在持 config 锁时做网络 I/O
    // （与 update_config 同守则，见 H1 修复）。
    let (url, pubkey_hex, timeout_secs) = {
        let mut mgr = state.config.lock().await;
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

    let index = registry::fetch_registry(&url, &pubkey, |u| {
        // own url（future 不借用闭包参数 u 的生命周期）；client 为外层长效借用
        let url_owned = u.to_string();
        let client = &client;
        async move { registry::http_fetch(client, &url_owned).await }
    })
    .await
    .map_err(|e| e.to_string())?;

    // 按宿主兼容性过滤（min_host_version ≤ 当前版本 + abi 精确匹配）
    let host_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("CARGO_PKG_VERSION 须为合法 semver");
    let compatible: Vec<RegistryEntry> = index
        .plugins
        .into_iter()
        .filter(|e| e.is_compatible_with(&host_version, HOST_ABI_VERSION))
        .collect();
    Ok(compatible)
}
