//! 联网搜索 + URL 抓取（85 号）。
//!
//! 移植自 `packages/core/src/utils/web-search.ts`：Tavily 搜索
//! （`searchWeb`，Bearer + body api_key 形态）与 URL 文本抓取
//! （`fetchUrl`，HTML 剥离 + 空白折叠 + UTF-16 截断）。

use serde_json::json;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Default)]
pub struct WebSearchOptions {
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
}

/// `searchWeb`：Tavily API 搜索。无 key → Err（调用方捕获降级，不炸轮次）。
pub async fn search_web(
    query: &str,
    max_results: usize,
    options: &WebSearchOptions,
) -> Result<Vec<SearchResult>, String> {
    let env_name = options.api_key_env.clone().unwrap_or_else(|| "TAVILY_API_KEY".to_string());
    let api_key = options
        .api_key
        .clone()
        .or_else(|| std::env::var(&env_name).ok().filter(|v| !v.is_empty()))
        .ok_or_else(|| {
            format!(
                "{env_name} not set. Configure Studio research search or set the env var to enable web search."
            )
        })?;
    let endpoint = options
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("https://api.tavily.com/search")
        .to_string();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&json!({
            "api_key": api_key,
            "query": query,
            "max_results": max_results,
            "search_depth": "basic",
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Tavily search failed: {} {}", status.as_u16(), body));
    }
    let data: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    Ok(data
        .get("results")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| SearchResult {
                    title: item.get("title").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    url: item.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    snippet: item.get("content").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// `fetchUrl`：抓 URL 文本。HTML → script/style/标签剥离 + 空白折叠；
/// 截断按 UTF-16 码元（TS `slice`）。
pub async fn fetch_url(url: &str, max_chars: usize) -> Result<String, String> {
    assert_public_egress_host(url)
        .await
        .map_err(|reason| format!("Fetch blocked: {reason}"))?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .header("Accept", "text/html, application/json, text/plain")
        .send()
        .await
        .map_err(|e| format!("Fetch failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Fetch failed: {} {}",
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("")
        ));
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let text = response.text().await.map_err(|e| format!("Fetch failed: {e}"))?;
    if content_type.contains("html") {
        static SCRIPT_STYLE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        static TAG: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        static BLANKS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let script_style = SCRIPT_STYLE
            .get_or_init(|| regex::Regex::new(r"(?i)<script[\s\S]*?</script>|<style[\s\S]*?</style>").unwrap());
        let tag = TAG.get_or_init(|| regex::Regex::new(r"<[^>]*>").unwrap());
        let blanks = BLANKS.get_or_init(|| regex::Regex::new(r"\s+").unwrap());
        let stripped = script_style.replace_all(&text, "");
        let stripped = tag.replace_all(&stripped, " ");
        let collapsed = blanks.replace_all(&stripped, " ").trim().to_string();
        return Ok(truncate_utf16(&collapsed, max_chars));
    }
    Ok(truncate_utf16(text.trim_start(), max_chars))
}

fn truncate_utf16(value: &str, max: usize) -> String {
    let mut units = 0usize;
    let mut out = String::new();
    for ch in value.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > max {
            break;
        }
        units += ch_units;
        out.push(ch);
    }
    out
}

// ── 出站 host 校验（SSRF 防线，556 号 NetworkEgress 语义）────────────────
//
// fetch_url 的 URL 来自搜索结果（模型经外部数据间接触达），host 面在连接
// 前拒绝回环/私网/链路本地/保留段。防线落函数级统一入口而非 ToolGuard：
// URL 在工具体内部组装（research_web 取自搜索命中），args 面拦截不可达。
// search_web 的 base_url 是用户显式配置（自建网关合法），不做此校验。
//
// 闸位定性（初判防线，备案）：DNS rebinding（解析后另行连接）与 30x 重定向
// 指内网两条旁路需连接级 pin / redirect 策略才能封死，本层为统一入口的
// host 面初判；TS 对偶（packages/core web-search.ts）同水位实现。

/// 从 URL 提取 host（仅 http/https；userinfo/端口剥除，IPv6 取方括号内）。
fn url_host(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(bracketed) = host_port.strip_prefix('[') {
        let host = &bracketed[..bracketed.find(']')?];
        return if host.is_empty() { None } else { Some(host) };
    }
    let host = host_port.split(':').next()?;
    if host.is_empty() { None } else { Some(host) }
}

/// 本地域名字面（解析前快筛）：localhost 全族与 .local（mDNS）。
fn is_blocked_host_literal(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    h == "localhost" || h.ends_with(".localhost") || h == "local" || h.ends_with(".local")
}

/// IP 段判定：v4 回环/私网/链路本地/未指定/0.0.0.0/8/广播；v6 回环/未指定/
/// ULA fc00::/7/链路本地 fe80::/10；v4-mapped 还原后按 v4 判。
fn is_blocked_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.octets()[0] == 0
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_ip(std::net::IpAddr::V4(mapped));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// 出站前校验：IP 字面量直判；域名先本地字面筛再 resolve 后对全部地址判
/// （resolve 失败不拦——连接阶段自然失败，本守卫只负责「解析成功但指向
/// 内网」与明确本地字面）。
pub async fn assert_public_egress_host(url: &str) -> Result<(), String> {
    let host = url_host(url).ok_or_else(|| "unsupported URL scheme or missing host".to_string())?;
    if is_blocked_host_literal(host) {
        return Err(format!("host '{host}' is a local network name"));
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return if is_blocked_ip(ip) {
            Err(format!("host '{host}' is a loopback/private address"))
        } else {
            Ok(())
        };
    }
    if let Ok(resolved) = tokio::net::lookup_host((host, 80)).await {
        for addr in resolved {
            if is_blocked_ip(addr.ip()) {
                return Err(format!(
                    "host '{host}' resolves to loopback/private address {}",
                    addr.ip()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_error_message() {
        let options = WebSearchOptions::default();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(search_web("query", 3, &options))
            .unwrap_err();
        // 未设 TAVILY_API_KEY 的环境（CI 常态）→ 固定文案。
        if std::env::var("TAVILY_API_KEY").is_err() {
            assert!(error.starts_with("TAVILY_API_KEY not set."), "{error}");
        }
    }

    #[test]
    fn utf16_truncation() {
        assert_eq!(truncate_utf16("abcdef", 3), "abc");
        assert_eq!(truncate_utf16("雪夜古宅", 2), "雪夜");
        assert_eq!(truncate_utf16("", 5), "");
    }

    #[test]
    fn url_host_extracts_authority() {
        assert_eq!(url_host("http://example.com/a"), Some("example.com"));
        assert_eq!(url_host("https://user:pw@Example.com:8443/x?y#z"), Some("Example.com"));
        assert_eq!(url_host("http://127.0.0.1:3000/"), Some("127.0.0.1"));
        assert_eq!(url_host("http://[::1]:8080/x"), Some("::1"));
        assert_eq!(url_host("ftp://example.com"), None);
        assert_eq!(url_host("not a url"), None);
        assert_eq!(url_host("http:///no-host"), None);
    }

    #[test]
    fn egress_guard_blocks_local_literals_and_private_ips() {
        for host in ["localhost", "a.localhost", "svc.local", "LOCALHOST", "host.local."] {
            assert!(is_blocked_host_literal(host), "{host}");
        }
        assert!(!is_blocked_host_literal("example.com"));
        assert!(!is_blocked_host_literal("notlocal.io"));

        for ip in [
            "127.0.0.1", "10.1.2.3", "172.16.0.1", "172.31.255.255", "192.168.1.1",
            "169.254.1.1", "0.0.0.0", "255.255.255.255", "::1", "fc00::1", "fd12::1",
            "fe80::1", "::ffff:10.0.0.1", "::ffff:127.0.0.1",
        ] {
            assert!(is_blocked_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "172.15.0.1", "2606:4700::1111"] {
            assert!(!is_blocked_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn egress_guard_blocks_before_any_network_use() {
        assert!(assert_public_egress_host("http://127.0.0.1/x").await.is_err());
        assert!(assert_public_egress_host("http://localhost/x").await.is_err());
        assert!(assert_public_egress_host("http://[::1]:8080/x").await.is_err());
        assert!(assert_public_egress_host("ftp://example.com/x").await.is_err());
        assert!(assert_public_egress_host("http://169.254.169.254/latest/meta-data").await.is_err());
        // 公网 IP 字面量纯本地判定放行（不发起真实请求）。
        assert!(assert_public_egress_host("http://8.8.8.8/x").await.is_ok());
    }
}
