//! Windows 实现：用 `netsh advfirewall firewall` 加 block 规则挡 `<port>`
//! 的入站，放行 loopback。
//!
//! ## 方案
//! `netsh advfirewall firewall add rule name="inkosDesktop_loopback_<port>"
//! dir=in action=block protocol=TCP localport=<port>`：入站 TCP 到
//! `<port>` 全挡。
//!
//! ## ⚠️ 已知精度缺陷（M1 范围）
//! `netsh ... action=block localport=<port>` 会**同时挡掉 127.0.0.1** 的
//! 入站——本机客户端访问 `http://127.0.0.1:<port>/` 也会被阻断，破坏 SPA
//! 加载。生产正确解法是 WFP（Windows Filtering Platform）的「例外 loopback」
//! 规则，或改用 IPsec policy。**M1 仅占位实现 + 显式 TODO**，运行时强制
//! 待 M3 的特权 helper。
//!
//! 调用方（`main.rs`）即便在 Windows 上 lock 成功，本机访问也可能失败；
//! 故 Windows 平台默认走「降级」路径——[`FirewallGuard::lock`] 先尝试，
//! 但**强烈建议** M1 在 Windows 上不启用 guard（`main.rs` 通过 `cfg`
//! 避免调 lock；详见 task-7-report.md）。

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
/// ⚠️ 此规则会同时挡 127.0.0.1——见模块级「已知精度缺陷」。
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

/// Windows 防火墙 guard。
///
/// ⚠️ M1 占位实现——见模块级文档「已知精度缺陷」。
pub struct FirewallGuard;

impl LoopbackGuard for FirewallGuard {
    fn lock(&self, port: u16) -> anyhow::Result<()> {
        let args = win_firewall_add_args(port);
        let output = Command::new("netsh")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| anyhow::anyhow!("netsh spawn 失败 (需管理员?): {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!(
                "netsh add rule 失败 (exit={}, 需管理员? stderr={} stdout={})",
                output.status,
                stderr.trim(),
                stdout.trim()
            );
        }
        Ok(())
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
}
