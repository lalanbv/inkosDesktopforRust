//! endpoint 免 key 判定。
//!
//! 移植自 `packages/core/src/utils/llm-endpoint-auth.ts`（40 行，逐字）：
//! anthropic 家族必须带 key；openai 家族视 baseUrl hostname——本地/回环/
//! .local/私网 IPv4 视为本地部署可免 key。

use url::Url;

/// `isApiKeyOptionalForEndpoint`：本地/自托管端点（Ollama 等）免 API key。
pub fn is_api_key_optional_for_endpoint(provider: &str, base_url: Option<&str>) -> bool {
    if provider == "anthropic" {
        return false;
    }
    let Some(base_url) = base_url else {
        return false;
    };
    if base_url.is_empty() {
        return false;
    }
    let Ok(url) = Url::parse(base_url) else {
        return false;
    };
    let hostname = url.host_str().unwrap_or_default().to_lowercase();
    if hostname.is_empty() {
        return false;
    }
    hostname == "localhost"
        || hostname == "127.0.0.1"
        || hostname == "::1"
        || hostname == "[::1]"
        || hostname == "0.0.0.0"
        || hostname == "host.docker.internal"
        || hostname.ends_with(".local")
        || is_private_ipv4(&hostname)
}

fn is_private_ipv4(hostname: &str) -> bool {
    let parts: Vec<Result<u32, _>> = hostname
        .split('.')
        .map(|segment| segment.parse::<u32>())
        .collect();
    if parts.len() != 4 || parts.iter().any(|part| part.is_err()) {
        return false;
    }
    let octets: Vec<u32> = parts.into_iter().map(|p| p.unwrap()).collect();
    if octets.iter().any(|&o| o > 255) {
        return false;
    }
    if octets[0] == 10 {
        return true;
    }
    if octets[0] == 192 && octets[1] == 168 {
        return true;
    }
    if octets[0] == 172 && (16..=31).contains(&octets[1]) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_never_optional() {
        assert!(!is_api_key_optional_for_endpoint("anthropic", Some("http://localhost:11434/v1")));
    }

    #[test]
    fn local_hosts_are_optional() {
        for url in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:11434/v1",
            "http://0.0.0.0/v1",
            "http://host.docker.internal/v1",
            "http://mybox.local/v1",
            "http://10.1.2.3/v1",
            "http://192.168.1.5/v1",
            "http://172.16.0.1/v1",
        ] {
            assert!(is_api_key_optional_for_endpoint("openai", Some(url)), "{url}");
        }
    }

    #[test]
    fn public_hosts_require_key() {
        assert!(!is_api_key_optional_for_endpoint("openai", Some("https://api.deepseek.com")));
        assert!(!is_api_key_optional_for_endpoint("openai", Some("https://172.32.0.1/v1")));
        assert!(!is_api_key_optional_for_endpoint("openai", None));
        assert!(!is_api_key_optional_for_endpoint("openai", Some("")));
    }
}
