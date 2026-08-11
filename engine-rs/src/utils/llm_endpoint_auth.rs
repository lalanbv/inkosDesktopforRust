//! LLM 端点 API key 可选性判定。
//!
//! 移植自 `packages/core/src/utils/llm-endpoint-auth.ts`。
//! 本地/私网端点（localhost/127/私网 IP/.local/docker internal）可免 API key。

use url::Url;

/// 判定端点是否可免 API key：anthropic 永不；本地/私网 hostname 免。
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
    let Some(host) = url.host_str() else {
        return false;
    };
    let hostname = host.to_lowercase();
    hostname == "localhost"
        || hostname == "127.0.0.1"
        || hostname == "::1"
        || hostname == "0.0.0.0"
        || hostname == "host.docker.internal"
        || hostname.ends_with(".local")
        || is_private_ipv4(&hostname)
}

/// 私网 IPv4 判定：10.x / 192.168.x / 172.16-31.x。
fn is_private_ipv4(hostname: &str) -> bool {
    let parts: Vec<&str> = hostname.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let Ok(nums) = parts.iter().map(|s| s.parse::<u8>()).collect::<Result<Vec<_>, _>>() else {
        return false;
    };
    // u8 parse 已保证 0-255（TS 还需显式检查，Rust 类型保证）
    if nums[0] == 10 {
        return true;
    }
    if nums[0] == 192 && nums[1] == 168 {
        return true;
    }
    if nums[0] == 172 && (16..=31).contains(&nums[1]) {
        return true;
    }
    false
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
