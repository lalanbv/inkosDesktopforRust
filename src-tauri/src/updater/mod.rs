//! M3d：双通道 updater（engine + shell），更新到本仓 GitHub 最新版本。
//!
//! - **engine 通道**（[`engine::EngineChannel`]）：GitHub Releases latest `v{semver}` →
//!   下载 engine bundle → SHA256 校验 → 原子替换 + engine.bak 回滚。见 [`engine`]。
//! - **shell 通道**：`tauri-plugin-updater` + Ed25519（pubkey 嵌 tauri.conf.json，
//!   M3e CI 用 private key 签 release）。命令 `cmd_check_updates`/`cmd_apply_shell_update`。
//!
//! 上游同步链路：M3e `desktop-sync-regression.yml` 定时 `git merge upstream/master` →
//! bump 版本 → tag → `desktop-build.yml` 构建 + 发布 engine bundle 到**本仓** Releases →
//! 本机 updater 拉本仓 release（单一可信源）。不直接拉 Narcooo/inkos。

pub mod delta;
pub mod engine;
pub mod sig;

use serde::Serialize;

/// 版本比较：latest_tag（形如 "v1.7.2"）是否严格新于 current（"1.7.2"）。
///
/// 返回 Some(latest_version) 表示有更新；None 表示无更新或解析失败（保守视为无更新）。
/// 容错：非 semver 串（如 "nightly"）→ None（不误报）。
pub fn is_newer(current: &str, latest_tag: &str) -> Option<semver::Version> {
    let cur = semver::Version::parse(current.trim().trim_start_matches('v')).ok()?;
    let latest = semver::Version::parse(latest_tag.trim().trim_start_matches('v')).ok()?;
    if latest > cur {
        Some(latest)
    } else {
        None
    }
}

/// 通道探测结果（前端/命令返回）。
#[derive(Debug, Clone, Serialize)]
pub struct ReleaseInfo {
    /// 通道名：engine / shell。
    pub channel: &'static str,
    /// 最新版本（无 v 前缀）。
    pub version: String,
    /// 是否需要更新（latest > current）。
    pub needs_update: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_detects_higher_version() {
        assert_eq!(
            is_newer("1.7.2", "v1.7.3").map(|v| v.to_string()),
            Some("1.7.3".to_string())
        );
        assert_eq!(
            is_newer("1.7.2", "v2.0.0").map(|v| v.to_string()),
            Some("2.0.0".to_string())
        );
    }

    #[test]
    fn is_newer_returns_none_when_not_higher() {
        assert!(is_newer("1.7.2", "v1.7.2").is_none()); // equal
        assert!(is_newer("1.7.3", "v1.7.2").is_none()); // lower
    }

    #[test]
    fn is_newer_returns_none_on_garbage() {
        // 非 semver（保守视为无更新，不误报）。
        assert!(is_newer("1.7.2", "nightly").is_none());
        assert!(is_newer("garbage", "v1.7.3").is_none());
    }

    #[test]
    fn is_newer_strips_v_and_whitespace() {
        assert_eq!(
            is_newer(" 1.7.2 ", " v1.8.0 ").map(|v| v.to_string()),
            Some("1.8.0".to_string())
        );
    }

    #[test]
    fn is_newer_sentinel_zero_means_always_older() {
        // H2 审计修复：manifest 读失败 sentinel = "0.0.0"（合法 semver，低于任何 release）
        // → 视为总需更新（不误报"已是最新"）。
        assert_eq!(
            is_newer("0.0.0", "v1.7.3").map(|v| v.to_string()),
            Some("1.7.3".to_string())
        );
    }
}
