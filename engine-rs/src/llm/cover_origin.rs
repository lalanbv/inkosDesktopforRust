//! 924 号：封面下载 origin 策略（900 号 cover-download-origin.ts 的 Rust
//! 镜像）——模型返回 URL 属不可信数据，下载前校验；信任锚 = 用户显式配置
//! 的 provider 端点（本地推理网关场景不受约束）。
//!
//! 放行规则（与 TS 版同序同语义）：协议仅 http/https、与 provider 同
//! origin 放行、allowlist（INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS 逗号分隔
//! origin）放行、其余跨 origin 的 host 环回/私有/保留地址与「解析到私网」
//! （rebinding 常见形）拒绝、INKOS_COVER_DOWNLOAD_ORIGIN_POLICY=strict
//! 收紧为仅同 origin+allowlist、未知取值 fail-closed。
//!
//! 残余面（与 TS 版同备案）：重定向盲请求已发出但响应经终态 URL 复核拒
//! 绝（caller 侧）；resolve-then-fetch 的 TOCTOU 窗。

use std::net::{IpAddr, ToSocketAddrs};

pub const ENV_COVER_DOWNLOAD_ORIGIN_POLICY: &str = "INKOS_COVER_DOWNLOAD_ORIGIN_POLICY";
pub const ENV_COVER_DOWNLOAD_EXTRA_ORIGINS: &str = "INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS";

#[derive(Debug, PartialEq, Eq)]
pub struct CoverDownloadOriginPolicy {
    pub mode: CoverDownloadOriginMode,
    pub invalid_value: Option<String>,
    pub extra_origins: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CoverDownloadOriginMode {
    Default,
    Strict,
    Invalid,
}

pub fn resolve_cover_download_origin_policy(env: &std::collections::BTreeMap<String, String>) -> CoverDownloadOriginPolicy {
    let raw = env.get(ENV_COVER_DOWNLOAD_ORIGIN_POLICY).map(String::as_str).unwrap_or("").trim().to_lowercase();
    let mode = match raw.as_str() {
        "" | "default" => CoverDownloadOriginMode::Default,
        "strict" => CoverDownloadOriginMode::Strict,
        _ => CoverDownloadOriginMode::Invalid,
    };
    let extra_origins = env
        .get(ENV_COVER_DOWNLOAD_EXTRA_ORIGINS)
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(String::from)
        .collect();
    CoverDownloadOriginPolicy {
        mode,
        invalid_value: (mode == CoverDownloadOriginMode::Invalid).then_some(raw),
        extra_origins,
    }
}

pub fn cover_download_origin_error(reason: &str, detail: &str) -> String {
    format!("cover image download blocked by origin policy ({reason}): {detail} [COVER_DOWNLOAD_ORIGIN_BLOCKED]")
}

/// IPv4 私有/保留地址分类（0/8、10/8、CGNAT 100.64/10、环回、链路本地、
/// 内网 172.16/12 与 192.168/16、协议保留与文档段、组播与保留尾段——与
/// TS 版 cover-download-origin.ts 同表）。
fn ipv4_is_private_or_reserved(octets: [u8; 4]) -> bool {
    let (a, b, c) = (octets[0], octets[1], octets[2]);
    a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113)
        || a >= 224
}

/// IPv6 私有/保留分类（::、::1、fc00::/7、fe80::/10、组播、2001:db8::/32、
/// v4-mapped 内嵌递归——与 TS 版同表）。
fn ipv6_is_private_or_reserved(segments: &[u16; 8]) -> bool {
    let (a, b, f, g) = (segments[0], segments[1], segments[5], segments[6]);
    if a == 0 && segments[1..7].iter().all(|s| *s == 0) && segments[7] <= 1 {
        return true; // ::（全零）与 ::1（环回，尾段=1）
    }
    if (a & 0xfe00) == 0xfc00 {
        return true; // fc00::/7 unique-local
    }
    if (a & 0xffc0) == 0xfe80 {
        return true; // fe80::/10 链路本地
    }
    if (a & 0xff00) == 0xff00 {
        return true; // 组播
    }
    if a == 0x2001 && b == 0x0db8 {
        return true; // 2001:db8::/32 文档段
    }
    if a == 0 && b == 0 && f == 0xffff {
        // v4-mapped ::ffff:a.b.c.d 内嵌递归
        let hi = (g >> 8) as u8;
        let lo = (g & 0xff) as u8;
        let c = (segments[7] >> 8) as u8;
        let d = (segments[7] & 0xff) as u8;
        return ipv4_is_private_or_reserved([hi, lo, c, d]);
    }
    false
}

fn ip_is_private_or_reserved(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_is_private_or_reserved(v4.octets()),
        IpAddr::V6(v6) => ipv6_is_private_or_reserved(&v6.segments()),
    }
}

/// host 分类：返回拒绝原因，None = 判定公网可放行。IP 字面量直接分类；
/// 域名先过环回/mDNS/单标签规则，再 DNS 解析逐地址校验（任何私网地址=
/// 拒绝，防 rebinding 常见形）；解析失败 fail-closed。
async fn classify_cover_download_host(hostname: &str) -> Option<&'static str> {
    // url crate 的 host_str 对 IPv6 带方括号（TS 版 URL.hostname 同款实证
    // 917/900 镜像），判定前剥离。
    let name = hostname
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_lowercase();
    if let Ok(ip) = name.parse::<IpAddr>() {
        return ip_is_private_or_reserved(ip).then_some("private-or-reserved-address");
    }
    if name == "localhost" || name.ends_with(".localhost") {
        return Some("loopback-host");
    }
    if name.ends_with(".local") {
        return Some("mdns-host");
    }
    if !name.contains('.') {
        return Some("single-label-host");
    }
    let records = {
        let name = name.clone();
        tokio::task::spawn_blocking(move || (name.as_str(), 0u16).to_socket_addrs().map(|addrs| addrs.collect::<Vec<_>>()))
            .await
            .ok()
    };
    let addrs = match records {
        Some(Ok(addrs)) => addrs,
        _ => return Some("resolve-failed"),
    };
    if addrs.is_empty() {
        return Some("resolve-failed");
    }
    for addr in &addrs {
        if ip_is_private_or_reserved(addr.ip()) {
            return Some("resolve-to-private-address");
        }
    }
    None
}

/// 模型返回下载 URL 的发送前校验（providerUrl 为用户显式配置的可信面：
/// 其 origin 下的 URL 直接放行）。通过则 Ok(())；拒绝则 Err(带标签错误)。
pub async fn assert_cover_download_url_allowed(
    url: &str,
    provider_url: &str,
    policy: &CoverDownloadOriginPolicy,
) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| cover_download_origin_error("unparsable-url", url))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(cover_download_origin_error("scheme-not-allowed", parsed.scheme()));
    }
    if policy.mode == CoverDownloadOriginMode::Invalid {
        return Err(cover_download_origin_error(
            "invalid-policy",
            &format!("{}=\"{}\"", ENV_COVER_DOWNLOAD_ORIGIN_POLICY, policy.invalid_value.as_deref().unwrap_or("")),
        ));
    }
    let provider_origin = url::Url::parse(provider_url).ok().map(|p| p.origin().unicode_serialization());
    if provider_origin.as_deref() == Some(parsed.origin().unicode_serialization().as_str()) {
        return Ok(());
    }
    if policy.extra_origins.iter().any(|origin| *origin == parsed.origin().unicode_serialization()) {
        return Ok(());
    }
    if let Some(host) = parsed.host_str() {
        if let Some(reason) = classify_cover_download_host(&host.to_lowercase()).await {
            return Err(cover_download_origin_error(reason, host));
        }
    }
    if policy.mode == CoverDownloadOriginMode::Strict {
        return Err(cover_download_origin_error("strict-cross-origin", &parsed.origin().unicode_serialization()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn env_with(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn default_policy() -> CoverDownloadOriginPolicy {
        resolve_cover_download_origin_policy(&env_with(&[]))
    }

    fn provider() -> &'static str {
        "http://127.0.0.1:9000/v1"
    }

    #[tokio::test]
    async fn policy_resolution_defaults_strict_and_invalid() {
        assert_eq!(resolve_cover_download_origin_policy(&env_with(&[])).mode, CoverDownloadOriginMode::Default);
        assert_eq!(resolve_cover_download_origin_policy(&env_with(&[(ENV_COVER_DOWNLOAD_ORIGIN_POLICY, " STRICT ")])).mode, CoverDownloadOriginMode::Strict);
        let invalid = resolve_cover_download_origin_policy(&env_with(&[(ENV_COVER_DOWNLOAD_ORIGIN_POLICY, "banana")]));
        assert_eq!(invalid.mode, CoverDownloadOriginMode::Invalid);
        assert_eq!(invalid.invalid_value.as_deref(), Some("banana"));
        let allowlist = resolve_cover_download_origin_policy(&env_with(&[(ENV_COVER_DOWNLOAD_EXTRA_ORIGINS, " http://a.example , https://b.example ")]));
        assert_eq!(allowlist.extra_origins, vec!["http://a.example", "https://b.example"]);
    }

    #[tokio::test]
    async fn scheme_and_unparsable_fail_before_any_network() {
        for url in ["file:///etc/passwd", "data:image/png;base64,AAAA", "ftp://8.8.8.8/x"] {
            let error = assert_cover_download_url_allowed(url, provider(), &default_policy()).await.unwrap_err();
            assert!(error.contains("scheme-not-allowed"), "{url} → {error}");
        }
        assert!(assert_cover_download_url_allowed("notaurl", provider(), &default_policy()).await.unwrap_err().contains("unparsable-url"));
    }

    #[tokio::test]
    async fn provider_origin_loopback_gateway_stays_trusted() {
        assert!(assert_cover_download_url_allowed("http://127.0.0.1:9000/img.png", provider(), &default_policy()).await.is_ok());
    }

    #[tokio::test]
    async fn private_and_reserved_literals_block_without_network() {
        for host in [
            "127.0.0.1", "10.0.0.1", "192.168.1.5", "172.16.0.9", "172.31.255.254",
            "169.254.169.254", "100.64.0.1", "0.1.2.3", "198.18.0.1", "224.0.0.1",
            "240.0.0.1", "[::1]", "[fc00::1]", "[fe80::1]", "[::ffff:10.0.0.1]",
        ] {
            let error = assert_cover_download_url_allowed(&format!("http://{host}/img.png"), provider(), &default_policy())
                .await
                .unwrap_err();
            assert!(error.contains("private-or-reserved-address"), "{host} → {error}");
        }
    }

    #[tokio::test]
    async fn public_literals_pass_and_hostname_rules_apply() {
        assert!(assert_cover_download_url_allowed("http://8.8.8.8/img.png", provider(), &default_policy()).await.is_ok());
        let error = assert_cover_download_url_allowed("http://localhost/img.png", provider(), &default_policy()).await.unwrap_err();
        assert!(error.contains("loopback-host"));
        let error = assert_cover_download_url_allowed("http://box.local/img.png", provider(), &default_policy()).await.unwrap_err();
        assert!(error.contains("mdns-host"));
        let error = assert_cover_download_url_allowed("http://intranet/img.png", provider(), &default_policy()).await.unwrap_err();
        assert!(error.contains("single-label-host"));
    }

    #[tokio::test]
    async fn allowlist_and_strict_and_invalid_enforcement() {
        let allowlisted = resolve_cover_download_origin_policy(&env_with(&[(ENV_COVER_DOWNLOAD_EXTRA_ORIGINS, "http://127.0.0.1:9999")]));
        assert!(assert_cover_download_url_allowed("http://127.0.0.1:9999/img.png", provider(), &allowlisted).await.is_ok());
        assert!(assert_cover_download_url_allowed("http://127.0.0.1:9998/img.png", provider(), &allowlisted).await.is_err());

        let strict = resolve_cover_download_origin_policy(&env_with(&[(ENV_COVER_DOWNLOAD_ORIGIN_POLICY, "strict")]));
        let error = assert_cover_download_url_allowed("http://8.8.8.8/img.png", provider(), &strict).await.unwrap_err();
        assert!(error.contains("strict-cross-origin"));
        assert!(assert_cover_download_url_allowed("http://127.0.0.1:9000/img.png", provider(), &strict).await.is_ok());

        let invalid = resolve_cover_download_origin_policy(&env_with(&[(ENV_COVER_DOWNLOAD_ORIGIN_POLICY, "banana")]));
        let error = assert_cover_download_url_allowed("http://8.8.8.8/img.png", provider(), &invalid).await.unwrap_err();
        assert!(error.contains("invalid-policy"));
    }

    #[tokio::test]
    async fn labeled_error_carries_code_tag() {
        let error = assert_cover_download_url_allowed("http://10.0.0.9/img.png", provider(), &default_policy()).await.unwrap_err();
        assert!(error.contains("COVER_DOWNLOAD_ORIGIN_BLOCKED"));
        assert!(error.contains("private-or-reserved-address"));
    }
}
