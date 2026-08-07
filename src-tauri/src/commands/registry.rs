//! 插件注册表 Tauri 命令
//!
//! 拉取配置的注册表，按宿主兼容性过滤后返回可装条目。前端（settings 面板）据此
//! 展示插件市场。注册表经 Ed25519 验签（见 `plugin::registry::fetch_registry`）。

use crate::commands::AppState;
use inkos_desktop::plugin::{
    registry::{self, RegistryEntry},
    PluginMetadata, PluginState, HOST_ABI_VERSION,
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
        async move {
            registry::http_fetch(client, &url_owned, registry::MAX_REGISTRY_BYTES).await
        }
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

/// 从注册表条目安装插件：下载 tar.gz → 验签包（sha256+Ed25519，用配置公钥）→
/// 安全解压（防 tar-slip）→ 复用既有 `install_plugin`。
///
/// 信任链：注册表已验签（`cmd_fetch_plugin_registry`）→ entry 可信 → bundle 用
/// **配置的 registry 公钥**验签。即便前端被攻破传入伪造 entry，其 bundle 签名也
/// 签不过配置公钥（攻击者无私钥）→ 无法安装恶意包。
#[tauri::command]
pub async fn cmd_install_from_registry(
    entry: RegistryEntry,
    config_state: tauri::State<'_, AppState>,
    plugin_state: tauri::State<'_, PluginState>,
) -> Result<PluginMetadata, String> {
    // 0. 重新校验前端传入的 entry 字段——verify_bundle 是真信任闸，此为纵深防御
    //（挡 http url、非法 id 等字段问题，错误更直观）
    entry
        .validate()
        .map_err(|e| format!("注册表条目字段非法: {e}"))?;

    // 1. 配置公钥 + 超时（单次取锁，释放后再网络 I/O）
    let (pubkey_hex, timeout_secs) = {
        let mut mgr = config_state.config.lock().await;
        let cfg = mgr.merged();
        let pubkey = cfg
            .registry
            .pubkey
            .clone()
            .ok_or_else(|| "插件注册表未配置（registry.pubkey 缺失，无法验签插件包）".to_string())?;
        (pubkey, cfg.network.timeout_seconds)
    };
    let pubkey = registry::decode_hex(&pubkey_hex)
        .map_err(|e| format!("注册表 pubkey 解码失败: {}", e))?;

    // 2. 下载插件包（bundle = tar.gz）
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(1) as u64))
        .build()
        .map_err(|e| format!("构造 HTTP client 失败: {}", e))?;
    let bundle = registry::http_fetch(&client, &entry.download_url, registry::MAX_BUNDLE_BYTES)
        .await
        .map_err(|e| format!("下载插件包失败: {}", e))?;

    // 3. 验签包（sha256 完整性 + Ed25519 来源签名，用配置公钥）——任一失败拒绝
    entry
        .verify_bundle(&bundle, &pubkey)
        .map_err(|e| format!("插件包校验失败（可能被篡改）: {}", e))?;

    // 4. 安全解压到临时目录（防 tar-slip 路径逃逸）。temp 活到函数末尾，install_plugin
    //    在其 drop 前完成文件复制。
    let temp = tempfile::tempdir().map_err(|e| format!("创建临时目录失败: {}", e))?;
    registry::safe_extract_tar_gz(&bundle, temp.path())
        .map_err(|e| format!("解压插件包失败: {}", e))?;

    // 5. 复用既有安装流（解析 plugin.toml + 复制到 plugins_dir）
    let source = temp.path().to_string_lossy().to_string();
    let mut manager = plugin_state.manager.lock().await;
    manager
        .install_plugin(&source)
        .await
        .map_err(|e| format!("安装失败: {}", e))
}
