//! LLM 端点 API key 可选性判定。
//!
//! 移植自 `packages/core/src/utils/llm-endpoint-auth.ts`。
//! 本地/私网端点（localhost/127/私网 IP/.local/docker internal）可免 API key。

use url::{Host, Url};

/// 判定端点是否可免 API key：anthropic 永不；本地/私网 hostname 免。
///
/// 用 `url::Host` 枚举匹配，避免 IPv6 字符串表示分歧；`Ipv4Addr::is_private` 的范围
/// （10/8、172.16/12、192.168/16）与 TS `isPrivateIpv4` 完全一致。
pub fn is_api_key_optional_for_endpoint(provider: Option<&str>, base_url: Option<&str>) -> bool {
    if provider == Some("anthropic") {
        return false;
    }
    let Some(base_url) = base_url else {
        return false;
    };
    let Ok(url) = Url::parse(base_url) else {
        return false;
    };
    match url.host() {
        Some(Host::Ipv6(ip)) => ip.is_loopback(), // ::1
        Some(Host::Ipv4(ip)) => {
            let o = ip.octets();
            o == [127, 0, 0, 1] || o == [0, 0, 0, 0] || ip.is_private()
        }
        Some(Host::Domain(d)) => {
            let h = d.to_lowercase();
            h == "localhost" || h == "host.docker.internal" || h.ends_with(".local")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_never_optional() {
        assert!(!is_api_key_optional_for_endpoint(Some("anthropic"), Some("http://localhost:11434")));
    }

    #[test]
    fn missing_baseurl_not_optional() {
        assert!(!is_api_key_optional_for_endpoint(Some("openai"), None));
    }

    #[test]
    fn localhost_optional() {
        assert!(is_api_key_optional_for_endpoint(Some("openai"), Some("http://localhost:11434/v1")));
        assert!(is_api_key_optional_for_endpoint(None, Some("http://127.0.0.1:1234")));
        assert!(is_api_key_optional_for_endpoint(None, Some("http://[::1]:8080")));
        assert!(is_api_key_optional_for_endpoint(None, Some("http://0.0.0.0:8080")));
    }

    #[test]
    fn docker_internal_optional() {
        assert!(is_api_key_optional_for_endpoint(None, Some("http://host.docker.internal:8080")));
    }

    #[test]
    fn dot_local_optional() {
        assert!(is_api_key_optional_for_endpoint(None, Some("http://myllm.local:8080")));
    }

    #[test]
    fn private_ipv4_optional() {
        assert!(is_api_key_optional_for_endpoint(None, Some("http://10.0.0.5:8080")));
        assert!(is_api_key_optional_for_endpoint(None, Some("http://192.168.1.100:8080")));
        assert!(is_api_key_optional_for_endpoint(None, Some("http://172.16.5.5:8080")));
        assert!(is_api_key_optional_for_endpoint(None, Some("http://172.31.0.1:8080")));
    }

    #[test]
    fn public_hostname_not_optional() {
        assert!(!is_api_key_optional_for_endpoint(None, Some("https://api.openai.com/v1")));
        assert!(!is_api_key_optional_for_endpoint(None, Some("https://api.deepseek.com")));
    }

    #[test]
    fn invalid_url_not_optional() {
        assert!(!is_api_key_optional_for_endpoint(None, Some("not-a-url")));
    }

    #[test]
    fn non_private_ipv4_not_optional() {
        assert!(!is_api_key_optional_for_endpoint(None, Some("http://8.8.8.8:8080")));
        assert!(!is_api_key_optional_for_endpoint(None, Some("http://172.15.0.1:8080"))); // 172 但 <16
    }
}
