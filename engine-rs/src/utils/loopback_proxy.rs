//! 回环端点代理旁路（1573 号，1572 号双栈同族收口）。
//!
//! reqwest `auto_sys_proxy` 无条件开启（0.12.28 `async_impl/client.rs:419`
//! `proxies.push(ProxyMatcher::system())`，非特性门控）：env 代理
//! （HTTP_PROXY/HTTPS_PROXY/ALL_PROXY）恒读取；`system-proxy` 特性（src-tauri
//! 默认启用，engine-rs 关闭）额外读 macOS SCDynamicStore——且 hyper-util
//! 0.1.21 只读 HTTP/HTTPS 开关，**不读 ExceptionsList**，系统代理豁免名单
//! 对 reqwest 无效（`localhost` 在豁免名单也救不了字面 IP 目标）。
//!
//! 1572 号实证的故障形态：代理守护进程健康时回环请求经代理「直连规则」转发
//! （+~2ms 往返，可用但脆弱）；守护进程降级/重启/规则收紧窗口内全部失败——
//! 本地推理端点（Ollama `127.0.0.1:11434`、LM Studio `127.0.0.1:1234`）、
//! 探活、通知因此获得对第三方代理守护进程的**隐性运行时依赖**。
//!
//! 本模块给出单一真源判定：目标为回环/未指定地址时构建 `.no_proxy()` client，
//! 外网目标保留代理（egress 走代理是受支持能力，不可误伤）。
//!
//! 与 TS 腿 `packages/core/src/utils/proxy-fetch.ts::isLoopbackTarget` 镜像
//! 对齐（双栈收敛）；src-tauri 三站（supervisor 探活/probe_once/SSE 订阅）
//! 目标为内部构造的 `127.0.0.1` 字面量，恒 `.no_proxy()` 无需本判定。

/// 回环/未指定地址判定（恒不走代理）。
///
/// 覆盖：`localhost` / `*.localhost` / IPv4 回环段 `127.0.0.0/8` / `::1` /
/// 未指定地址（`0.0.0.0` / `::`——连接永不出本机，走代理必错）。解析失败或
/// 非 http(s) scheme → `false`（保守：保持既有代理行为不变）。
///
/// url crate 按 WHATWG 归一化主机（`127.1` 短形 → `127.0.0.1`）；IPv6 的
/// `host_str()` **带括号**（WHATWG 序列化，与 TS `URL.hostname` 一致——
/// 实测 2.x：`[::1]` → `"[::1]"`），解析前剥括号。
pub fn is_loopback_base_url(base_url: &str) -> bool {
    let Ok(url) = url::Url::parse(base_url) else {
        return false;
    };
    if url.scheme() != "http" && url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    // WHATWG IPv6 序列化带方括号；剥括号后交 std 解析（v4 映射形如
    // `[::ffff:7f00:1]` 剥括号后仍是合法 Ipv6Addr，is_loopback=false，与 TS 对齐）。
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(&host);
    if let Ok(v4) = bare.parse::<std::net::Ipv4Addr>() {
        return v4.is_loopback() || v4.is_unspecified();
    }
    if let Ok(v6) = bare.parse::<std::net::Ipv6Addr>() {
        return v6.is_loopback() || v6.is_unspecified();
    }
    false
}

/// 在既有 builder 上施加回环旁路：目标为回环/未指定地址 → `.no_proxy()`，
/// 其余原样返回（外网保留 env/系统代理）。
pub fn no_proxy_if_loopback(
    builder: reqwest::ClientBuilder,
    base_url: &str,
) -> reqwest::ClientBuilder {
    if is_loopback_base_url(base_url) {
        builder.no_proxy()
    } else {
        builder
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表驱动：旁路集合与边界锁定（私网/外网/解析失败不旁路），与 TS 腿
    /// `proxy-fetch.test.ts` 边界镜像。
    #[test]
    fn loopback_and_unspecified_targets_are_detected() {
        for (url, expect) in [
            ("http://localhost:11434/v1", true),
            ("http://localhost", true),
            ("http://my-ollama.localhost:11434/v1", true),
            ("http://127.0.0.1:11434/v1", true),
            ("http://127.8.8.8:8080", true), // 整个 127.0.0.0/8 段
            ("http://127.1/v1", true),       // WHATWG 短形归一化
            ("http://0.0.0.0:11434", true),  // 未指定地址=本机
            ("http://[::1]:11434/v1", true),
            ("http://[::]:9200", true), // 未指定 IPv6
            ("https://api.openai.com/v1", false),
            ("http://192.168.1.10:8080/v1", false), // 私网≠回环，仍走代理
            ("http://10.0.0.5:11434", false),
            ("http://example.com", false),
            ("ftp://127.0.0.1/file", false), // 非 http(s) 不旁路
            ("not a url", false),
            ("", false),
            ("http://[::ffff:127.0.0.1]/v1", false), // v4 映射不判回环（对齐 TS）
        ] {
            assert_eq!(is_loopback_base_url(url), expect, "case: {url:?}");
        }
    }
}
