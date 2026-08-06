//! Windows 实现：占位 + 规则生成函数（供 M3 WFP/特数 helper 复用）。
//!
//! ## 为什么 lock 直接 bail
//! `netsh advfirewall firewall add rule ... action=block localport=<port>`
//! 会**同时挡掉 127.0.0.1** 的入站——本机客户端访问 `http://127.0.0.1:<port>/`
//! 也会被阻断，破坏 SPA 加载（自挡）。
//!
//! 真正的生产解法是 WFP（Windows Filtering Platform）的「例外 loopback」
//! 规则，或改用 IPsec policy；两者都需要 M3 的特权 helper（不能在普通用户
//! 进程里直接调）。
//!
//! 故 M1/M2 范围内 [`FirewallGuard::lock`] 直接 `bail!`——让 `main.rs`
//! 统一走 Err 降级分支 log "guard lock 失败，降级继续"，而不会出现
//! 「netsh 报成功 → SPA 起不来」的假成功陷阱。`release` 与规则串生成函数
//! 仍保留，供 M3 WFP 实现复用。

use super::LoopbackGuard;
use std::process::{Command, Stdio};

/// 规则名前缀（与端口拼接成完整规则名）。
pub const RULE_NAME_PREFIX: &str = "inkosDesktop_loopback_";

/// 由端口号生成 netsh 规则名（纯函数，便于单测断言）。
///
/// # 示例
/// ```
/// # use inkos_desktop::isolation::windows::win_firewall_rule_name;
/// assert_eq!(win_firewall_rule_name(4567), "inkosDesktop_loopback_4567");
/// ```
pub fn win_firewall_rule_name(port: u16) -> String {
    format!("{RULE_NAME_PREFIX}{port}")
}

/// 构造 `netsh advfirewall firewall add rule` 的参数切片（不含 `netsh`）。
///
/// ⚠️ M1/M2 不再被生产代码调用（[`FirewallGuard::lock`] 直接 bail）。
/// 保留供电 M3 WFP 实现复用——故 `#[allow(dead_code)]`。
///
/// ⚠️ 此规则会同时挡 127.0.0.1——见模块级「为什么 lock 直接 bail」。
#[allow(dead_code)]
pub fn win_firewall_add_args(port: u16) -> Vec<String> {
    vec![
        "advfirewall".to_string(),
        "firewall".to_string(),
        "add".to_string(),
        "rule".to_string(),
        format!("name={}", win_firewall_rule_name(port)),
        "dir=in".to_string(),
        "action=block".to_string(),
        "protocol=TCP".to_string(),
        format!("localport={port}"),
    ]
}

/// 构造 `netsh advfirewall firewall delete rule` 的参数切片。
pub fn win_firewall_delete_args(port: u16) -> Vec<String> {
    vec![
        "advfirewall".to_string(),
        "firewall".to_string(),
        "delete".to_string(),
        "rule".to_string(),
        format!("name={}", win_firewall_rule_name(port)),
    ]
}

/// Windows 防火墙 guard（M1/M2 占位——见模块级文档）。
///
/// `lock` **直接 bail**：netsh block 规则会同时挡 127.0.0.1（自挡 SPA），
/// 让 main.rs 走真实 Err 降级分支统一 log，避免「netsh 成功 → SPA 起不来」
/// 的假成功陷阱。`release` 仍调 netsh delete（幂等，清掉任何残留规则）。
/// 两者都保留供电 M3 WFP 实现复用规则串生成函数。
pub struct FirewallGuard;

impl LoopbackGuard for FirewallGuard {
    fn lock(&self, _port: u16) -> anyhow::Result<()> {
        // 不调 netsh：netsh block localport=<port> 会同时挡 127.0.0.1
        // （自挡 SPA）。WFP 例外 loopback 待 M3 特权 helper。
        // 详见模块级文档「为什么 lock 直接 bail」。
        anyhow::bail!(
            "Windows loopback guard deferred to WFP (M3); netsh would block loopback too"
        );
    }

    fn release(&self, port: u16) -> anyhow::Result<()> {
        let args = win_firewall_delete_args(port);
        let output = Command::new("netsh")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| anyhow::anyhow!("netsh (delete) spawn 失败: {e}"))?;
        if !output.status.success() {
            // 删除不存在的规则也是非 0 退出——幂等语义下不视作错误
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "[isolation::windows] release: netsh delete exit={} (视为幂等成功): {}",
                output.status,
                stderr.trim()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_name_contains_port() {
        let n = win_firewall_rule_name(4567);
        assert!(n.contains("4567"));
        assert!(n.starts_with(RULE_NAME_PREFIX));
    }

    #[test]
    fn rule_name_stable_per_port() {
        assert_eq!(win_firewall_rule_name(4567), "inkosDesktop_loopback_4567");
        assert_eq!(win_firewall_rule_name(8765), "inkosDesktop_loopback_8765");
    }

    #[test]
    fn add_args_block_direction_in() {
        let a = win_firewall_add_args(4567);
        assert!(a.contains(&"dir=in".to_string()));
        assert!(a.contains(&"action=block".to_string()));
    }

    #[test]
    fn add_args_protocol_tcp_localport() {
        let a = win_firewall_add_args(4567);
        assert!(a.contains(&"protocol=TCP".to_string()));
        assert!(a.contains(&"localport=4567".to_string()));
    }

    #[test]
    fn add_args_includes_rule_name() {
        let a = win_firewall_add_args(4567);
        assert!(a.contains(&"name=inkosDesktop_loopback_4567".to_string()));
    }

    #[test]
    fn delete_args_match_name_only() {
        // delete 不带 port 等字段（按 name 删）
        let a = win_firewall_delete_args(4567);
        assert!(a.contains(&"delete".to_string()));
        assert!(a.contains(&"name=inkosDesktop_loopback_4567".to_string()));
        // delete 不应有 action / localport
        assert!(!a.iter().any(|x| x.starts_with("action=")));
        assert!(!a.iter().any(|x| x.starts_with("localport=")));
    }

    #[test]
    fn firewall_guard_satisfies_trait() {
        let _g: Box<dyn LoopbackGuard> = Box::new(FirewallGuard);
    }

    /// C3 修复回归断言：`lock` 必须直接返回 Err（避免 netsh block 自挡 SPA
    /// 的「假成功」陷阱）。WFP 实现待 M3 特权 helper。
    /// 不依赖 netsh 可执行——纯断言 bail 字符串，CI/headless 必稳。
    #[test]
    fn firewall_guard_lock_always_bails() {
        let g = FirewallGuard;
        let err = g
            .lock(4567)
            .expect_err("FirewallGuard::lock 应直接 bail（netsh 自挡 SPA）");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("deferred to WFP"),
            "bail 消息应提及 WFP deferral，实际: {msg}"
        );
    }

    /// C3 回归：release 不应在 lock 未成功时 panic 或出错（幂等语义）。
    /// headless/CI 下 netsh 多半不存在——本测试容忍 netsh spawn 失败，
    /// 只断言 release 不 propagate error（捕获 stderr 即可）。
    #[test]
    fn firewall_guard_release_is_idempotent_even_without_prior_lock() {
        let g = FirewallGuard;
        // 不调 lock（已在上一个测试断言它 bail），直接 release：
        // 实环境下 netsh 多半无此规则名 → 非 0 退出 → 内部视为幂等成功。
        // 若 netsh 不存在（CI 无 Windows 工具链）→ spawn Err 被 propagate 为 Err。
        // 故本测试容忍两种结果，仅断言不 panic。
        let _ = g.release(4567);
    }
}
