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
) -> Result<(String, Vec<u8>, reqwest::Client, std::path::PathBuf), String> {
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
    // 缓存路径（网络失败时离线回退）
    let cache_path = config_state.config_loader.paths().registry_cache();
    Ok((url, pubkey, client, cache_path))
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

/// 读本地缓存注册表索引（上次已验签接受的，作单调新鲜性锚）。任一步失败 → None。
fn read_cached_index(cache_path: &std::path::Path) -> Option<registry::PluginRegistryIndex> {
    std::fs::read_to_string(cache_path)
        .ok()
        .and_then(|s| registry::PluginRegistryIndex::parse(&s).ok())
}

/// 单调新鲜性判定：新索引的 `generated_at` 比「上次接受的」缓存更旧 → `true`（判为
/// 重放/降级，应回退缓存）。纯函数，可单测。
///
/// fail-open 策略：缺 `generated_at`（冷启动 / 旧格式注册表）或非 RFC3339 时返回
/// `false`（无法判定时不阻断可用性，仅依赖签名防伪造）。chrono 按**实际时刻**比较，
/// 兼容任意合法 RFC3339 偏移（对齐「最佳兼容性」目标）。
fn is_registry_downgrade(
    new: &registry::PluginRegistryIndex,
    cached: &registry::PluginRegistryIndex,
) -> bool {
    use chrono::DateTime;
    let (Some(prev), Some(new_g)) = (cached.generated_at.as_deref(), new.generated_at.as_deref())
    else {
        return false; // 缺时间戳 → 无法判定
    };
    let (Ok(prev_dt), Ok(new_dt)) = (
        DateTime::parse_from_rfc3339(prev),
        DateTime::parse_from_rfc3339(new_g),
    ) else {
        tracing::warn!("generated_at 非 RFC3339，跳过单调新鲜性校验");
        return false; // 非法格式 → fail-open
    };
    new_dt < prev_dt // 新的更旧 → 倒退
}

/// 拉取并验签注册表索引（`MAX_REGISTRY_BYTES` 限流）+ **缓存**：
/// 网络成功 → 序列化已验签索引到 cache_path；网络/验签/解析失败 → 回退本地缓存
/// （离线韧性）。fetch / check / list 共用。
async fn fetch_registry_index(
    url: &str,
    pubkey: &[u8],
    client: &reqwest::Client,
    cache_path: &std::path::Path,
) -> Result<registry::PluginRegistryIndex, String> {
    match registry::fetch_registry(url, pubkey, |u| {
        let url_owned = u.to_string();
        async move {
            registry::http_fetch(client, &url_owned, registry::MAX_REGISTRY_BYTES).await
        }
    })
    .await
    {
        Ok(index) => {
            // R6 单调新鲜性：防重放降级。缓存即上次已验签接受的注册表（单调锚）——
            // 若新拉取的 generated_at 比缓存的更旧，判为重放/降级攻击，回退缓存。
            // 闭包返回 cached（Some 时）保证 is_registry_downgrade 为真则缓存必存在。
            if let Some(cached) = read_cached_index(cache_path) {
                if is_registry_downgrade(&index, &cached) {
                    tracing::warn!(
                        "注册表 generated_at 倒退（疑似重放/降级），拒绝并回退本地缓存"
                    );
                    return Ok(cached);
                }
            }
            // best-effort 缓存（序列化已验签索引；写失败仅 warn，不阻断本次）
            if let Ok(toml_str) = toml::to_string(&index) {
                if let Err(e) = std::fs::write(cache_path, toml_str) {
                    tracing::warn!("写注册表缓存失败（不影响本次）: {e}");
                }
            }
            Ok(index)
        }
        Err(e) => {
            // 网络/验签/解析失败 → 回退本地缓存（离线韧性；缓存来自上次已验签索引）
            match std::fs::read_to_string(cache_path) {
                Ok(cached) => {
                    tracing::warn!("注册表拉取失败，使用本地缓存（可能过期）: {e}");
                    registry::PluginRegistryIndex::parse(&cached)
                        .map_err(|pe| format!("缓存解析失败: {pe}"))
                }
                Err(_) => Err(format!("注册表拉取失败且无本地缓存: {e}")),
            }
        }
    }
}

/// 拉取插件注册表，按宿主兼容性过滤后返回可装条目。
#[tauri::command]
pub async fn cmd_fetch_plugin_registry(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<RegistryEntry>, String> {
    let (url, pubkey, client, cache_path) = registry_source_and_client(&state).await?;
    let index = fetch_registry_index(&url, &pubkey, &client, &cache_path).await?;

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

/// 从注册表条目安装插件：无依赖→直接下载验签安装；有依赖→解析后序拓扑→按序安装
/// 依赖（跳过已装）→entry 自身最后。下载→验签→安全解压→`install_plugin`。
#[tauri::command]
pub async fn cmd_install_from_registry(
    entry: RegistryEntry,
    config_state: tauri::State<'_, AppState>,
    plugin_state: tauri::State<'_, PluginState>,
) -> Result<PluginMetadata, String> {
    let (url, pubkey, client, cache_path) = registry_source_and_client(&config_state).await?;

    // 无依赖：直接安装（避免为 deps-free 插件无谓拉取注册表）
    if entry.dependencies.is_empty() {
        let temp = download_verify_extract(&entry, &pubkey, &client).await?;
        let source = temp.path().to_string_lossy().to_string();
        let mut manager = plugin_state.manager.lock().await;
        return manager
            .install_plugin(&source)
            .await
            .map_err(|e| format!("安装失败: {}", e));
    }

    // 有依赖：解析后序拓扑（依赖在前、entry 最后）→ 按序安装，跳过已装
    let index = fetch_registry_index(&url, &pubkey, &client, &cache_path).await?;
    let order = registry::resolve_dependencies(&entry, &index)
        .map_err(|e| format!("依赖解析失败: {}", e))?;
    let installed_ids: std::collections::HashSet<String> = {
        let manager = plugin_state.manager.lock().await;
        manager
            .list_plugins()
            .into_iter()
            .map(|m| m.id)
            .collect()
    };
    let mut last_meta: Option<PluginMetadata> = None;
    for e in &order {
        if installed_ids.contains(&e.id) {
            continue; // 依赖已装，跳过
        }
        let temp = download_verify_extract(e, &pubkey, &client).await?;
        let source = temp.path().to_string_lossy().to_string();
        let meta = {
            let mut manager = plugin_state.manager.lock().await;
            manager
                .install_plugin(&source)
                .await
                .map_err(|err| format!("安装依赖 {} 失败: {}", e.id, err))?
        };
        if e.id == entry.id {
            last_meta = Some(meta);
        }
    }
    last_meta.ok_or_else(|| "插件已安装（含依赖）".to_string())
}

/// 从注册表条目更新插件：下载→验签→安全解压→`update_plugin`（覆盖已安装，无中间缺失态）。
#[tauri::command]
pub async fn cmd_update_plugin_from_registry(
    entry: RegistryEntry,
    config_state: tauri::State<'_, AppState>,
    plugin_state: tauri::State<'_, PluginState>,
) -> Result<PluginMetadata, String> {
    let (_url, pubkey, client, _cache_path) = registry_source_and_client(&config_state).await?;
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
    let (url, pubkey, client, cache_path) = registry_source_and_client(&config_state).await?;
    let index = fetch_registry_index(&url, &pubkey, &client, &cache_path).await?;

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

/// 列出某插件在注册表中的全部版本（降序），供版本选择器（锁定/回滚指定版本）。
#[tauri::command]
pub async fn cmd_list_plugin_versions(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<RegistryEntry>, String> {
    let (url, pubkey, client, cache_path) = registry_source_and_client(&state).await?;
    let index = fetch_registry_index(&url, &pubkey, &client, &cache_path).await?;
    Ok(index.find_all_versions(&id).into_iter().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx(generated_at: Option<&str>) -> registry::PluginRegistryIndex {
        registry::PluginRegistryIndex {
            version: 1,
            generated_at: generated_at.map(|s| s.to_string()),
            plugins: vec![],
        }
    }

    #[test]
    fn downgrade_rejects_older_generated_at() {
        let cached = idx(Some("2026-08-14T00:00:00Z"));
        let older = idx(Some("2026-08-07T00:00:00Z"));
        assert!(
            is_registry_downgrade(&older, &cached),
            "更旧的 generated_at 应判降级（重放/降级）"
        );
    }

    #[test]
    fn downgrade_accepts_newer_generated_at() {
        let cached = idx(Some("2026-08-07T00:00:00Z"));
        let newer = idx(Some("2026-08-14T00:00:00Z"));
        assert!(!is_registry_downgrade(&newer, &cached), "更新的应接受");
    }

    #[test]
    fn downgrade_equal_not_flagged() {
        let cached = idx(Some("2026-08-07T00:00:00Z"));
        let same = idx(Some("2026-08-07T00:00:00Z"));
        assert!(
            !is_registry_downgrade(&same, &cached),
            "相同时刻非降级（重放同份注册表 benign）"
        );
    }

    #[test]
    fn downgrade_missing_new_timestamp_fail_open() {
        let cached = idx(Some("2026-08-07T00:00:00Z"));
        let no_ts = idx(None);
        assert!(!is_registry_downgrade(&no_ts, &cached), "缺 generated_at → fail-open");
    }

    #[test]
    fn downgrade_missing_cached_timestamp_fail_open() {
        let cached = idx(None);
        let new = idx(Some("2026-08-07T00:00:00Z"));
        assert!(!is_registry_downgrade(&new, &cached), "缓存无锚（冷启动）→ fail-open");
    }

    #[test]
    fn downgrade_non_rfc3339_fail_open() {
        let cached = idx(Some("2026-08-07T00:00:00Z"));
        let bad = idx(Some("not-a-date"));
        assert!(!is_registry_downgrade(&bad, &cached), "非 RFC3339 → fail-open（兼容性）");
    }

    #[test]
    fn downgrade_compares_instant_across_offsets() {
        // 缓存 13:00+02:00 = 11:00 UTC
        let cached = idx(Some("2026-08-07T13:00:00+02:00"));
        // 新 12:00+00:00 = 12:00 UTC（晚于 11:00）→ 非降级
        let newer = idx(Some("2026-08-07T12:00:00+00:00"));
        assert!(
            !is_registry_downgrade(&newer, &cached),
            "跨偏移按实际时刻比较：12:00Z 晚于 11:00Z → 接受"
        );
        // 新 10:00Z（早于 11:00）→ 降级
        let older = idx(Some("2026-08-07T10:00:00+00:00"));
        assert!(
            is_registry_downgrade(&older, &cached),
            "10:00Z 早于 11:00Z → 降级"
        );
    }
}
