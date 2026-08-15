//! 封面生成 provider 预设。
//!
//! 移植自 `packages/core/src/llm/cover-providers.ts`（59 行，逐字）。

use url::Url;

/// 封面 provider id。
pub type CoverProviderId = &'static str;

/// 封面 provider 预设。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverProviderPreset {
    pub service: CoverProviderId,
    pub label: &'static str,
    pub base_url: &'static str,
    pub api: &'static str,
    pub default_model: &'static str,
    pub models: &'static [&'static str],
}

/// 3 个封面 provider（逐字 COVER_PROVIDER_PRESETS）。
pub const COVER_PROVIDER_PRESETS: &[CoverProviderPreset] = &[
    CoverProviderPreset {
        service: "kkaiapi",
        label: "kkaiapi",
        base_url: "https://api.kkaiapi.com/v1",
        api: "images",
        default_model: "gpt-image-2",
        models: &["gpt-image-2"],
    },
    CoverProviderPreset {
        service: "openai",
        label: "OpenAI Images",
        base_url: "https://api.openai.com/v1",
        api: "images",
        default_model: "gpt-image-2",
        models: &["gpt-image-2"],
    },
    CoverProviderPreset {
        service: "google",
        label: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta",
        api: "gemini",
        default_model: "gemini-3.1-flash-image-preview",
        models: &["gemini-3.1-flash-image-preview", "gemini-2.5-flash-image"],
    },
];

/// 按 service 查封面预设。对齐 TS `resolveCoverProviderPreset`。
pub fn resolve_cover_provider_preset(service: Option<&str>) -> Option<&'static CoverProviderPreset> {
    COVER_PROVIDER_PRESETS
        .iter()
        .find(|provider| Some(provider.service) == service)
}

/// 校验并规范化封面 baseUrl：http(s)、无凭据/查询/锚点、去尾斜杠。
/// 对齐 TS `normalizeCoverBaseUrl`（非字符串/空 → None）。
pub fn normalize_cover_base_url(value: Option<&str>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let Ok(parsed) = Url::parse(trimmed) else {
        return None;
    };
    let scheme_ok = parsed.scheme() == "http" || parsed.scheme() == "https";
    if !scheme_ok {
        return None;
    }
    let has_credentials = !parsed.username().is_empty() || parsed.password().is_some();
    if has_credentials || parsed.query().is_some() || parsed.fragment().is_some() {
        return None;
    }
    Some(trimmed.trim_end_matches('/').to_string())
}

/// 封面 secret 的存储键：`cover:{service}`。对齐 TS `coverSecretKey`。
pub fn cover_secret_key(service: &str) -> String {
    format!("cover:{service}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_presets() {
        assert_eq!(COVER_PROVIDER_PRESETS.len(), 3);
        assert_eq!(resolve_cover_provider_preset(Some("kkaiapi")).unwrap().api, "images");
        assert!(resolve_cover_provider_preset(Some("unknown")).is_none());
        assert!(resolve_cover_provider_preset(None).is_none());
    }

    #[test]
    fn normalize_base_url_rules() {
        assert_eq!(
            normalize_cover_base_url(Some("https://api.kkaiapi.com/v1/")),
            Some("https://api.kkaiapi.com/v1".to_string())
        );
        assert_eq!(normalize_cover_base_url(Some("ftp://x.com")), None);
        assert_eq!(normalize_cover_base_url(Some("https://u:p@x.com")), None);
        assert_eq!(normalize_cover_base_url(Some("https://x.com?q=1")), None);
        assert_eq!(normalize_cover_base_url(Some("https://x.com#f")), None);
        assert_eq!(normalize_cover_base_url(Some("  ")), None);
        assert_eq!(normalize_cover_base_url(None), None);
        assert_eq!(normalize_cover_base_url(Some("not a url")), None);
    }

    #[test]
    fn secret_key_format() {
        assert_eq!(cover_secret_key("kkaiapi"), "cover:kkaiapi");
    }
}
