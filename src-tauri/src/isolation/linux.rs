//! Linux 实现：用 iptables INPUT 链挡外部入站到 `<port>`，放行 loopback。
//!
//! ## 方案
//! `iptables -A INPUT -p tcp --dport <port> ! -i lo -j DROP`：除 lo 接口外
//! 的入站 TCP 到 `<port>` 全 DROP。配套 `-C` 探测、`-D` 删除以实现幂等。
//!
//! ## 权限
//! `iptables` 需 CAP_NET_ADMIN（典型即 root）。M1 不假设 app 以 root 跑——
//! 失败时 [`IptablesGuard::lock`] 返回 `Err`，`main.rs` 仅 log 警告、不阻塞
//! 启动。完整运行时强制待 M2/M3 的 setuid helper / nftables 后端。
//!
//! ## nftables 后端
//! 现代 Debian/Fedora 默认 nftables 后端，`iptables` 命令仍可用（iptables-
//! nf_tables 兼容层）。本模块坚持 `iptables` 命令以兼容老内核；nft 原生
//! 接口待 M3。

use super::LoopbackGuard;
use std::process::{Command, Stdio};

/// iptables 规则参数（纯函数，便于单测断言）。
///
/// 返回应用于 `iptables` 的参数切片（不含 `iptables` 本身）。规则：
/// `-p tcp --dport <port> ! -i lo -j DROP`
///
/// 调用方可用于：
/// - `iptables -A INPUT <args>` 安装；
/// - `iptables -C INPUT <args>` 探测是否已存在；
/// - `iptables -D INPUT <args>` 删除。
pub fn iptables_rule_args(port: u16) -> Vec<String> {
    vec![
        "-p".to_string(),
        "tcp".to_string(),
        "--dport".to_string(),
        port.to_string(),
        "!".to_string(),
        "-i".to_string(),
        "lo".to_string(),
        "-j".to_string(),
        "DROP".to_string(),
    ]
}

/// Linux iptables guard。
pub struct IptablesGuard;

impl LoopbackGuard for IptablesGuard {
    fn lock(&self, port: u16) -> anyhow::Result<()> {
        let args = iptables_rule_args(port);
        // 幂等：先 -C 检测，已存在则跳过；不存在则 -A 追加。
        // -C 退出码：0=已存在，1=不存在，2=出错。
        let check = Command::new("iptables")
            .arg("-C")
            .arg("INPUT")
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match check {
            Ok(s) if s.success() => {
                // 规则已存在——幂等成功
                return Ok(());
            }
            Ok(s) if s.code() == Some(1) => {
                // 不存在，继续 -A
            }
            Ok(s) => {
                // 退出码 2（如无权限）或其他——降级到 -A 再试一次以获得真实错误
                eprintln!(
                    "[isolation::linux] iptables -C 返回 exit={}，尝试 -A 以获取错误",
                    s
                );
            }
            Err(e) => {
                anyhow::bail!("iptables spawn 失败 (命令缺失? {e})");
            }
        }

        let mut cmd = Command::new("iptables");
        cmd.arg("-A").arg("INPUT").args(&args);
        cmd.stdout(Stdio::null()).stderr(Stdio::piped());
        let output = cmd
            .output()
            .map_err(|e| anyhow::anyhow!("iptables -A spawn 失败: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "iptables -A 失败 (exit={}, 需 CAP_NET_ADMIN? stderr: {})",
                output.status,
                stderr.trim()
            );
        }
        Ok(())
    }

    fn release(&self, port: u16) -> anyhow::Result<()> {
        let args = iptables_rule_args(port);
        // -D 删除（幂等：若不存在退出码非 0，不视作错误）
        let output = Command::new("iptables")
            .arg("-D")
            .arg("INPUT")
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| anyhow::anyhow!("iptables -D spawn 失败: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "[isolation::linux] release: iptables -D exit={} (视为幂等成功): {}",
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
    fn args_target_tcp_dport_port() {
        let a = iptables_rule_args(4567);
        assert!(a.contains(&"--dport".to_string()));
        assert!(a.contains(&"4567".to_string()));
        assert!(a.contains(&"tcp".to_string()));
    }

    #[test]
    fn args_exclude_loopback_interface() {
        let a = iptables_rule_args(4567);
        // ! -i lo：非 lo 接口
        let bang_idx = a.iter().position(|x| x == "!");
        let iface_idx = a.iter().position(|x| x == "-i");
        assert!(bang_idx.is_some(), "应含 '!' 否定符");
        assert!(iface_idx.is_some(), "应含 -i 接口标志");
        // 否定符应紧邻 -i 之前
        assert_eq!(bang_idx.unwrap() + 1, iface_idx.unwrap());
        // 接口名 lo
        assert!(a.contains(&"lo".to_string()));
    }

    #[test]
    fn args_drop_action() {
        let a = iptables_rule_args(4567);
        assert!(a.contains(&"-j".to_string()));
        assert!(a.contains(&"DROP".to_string()));
    }

    #[test]
    fn args_port_varies_with_input() {
        // 确认非硬编码 4567
        let a = iptables_rule_args(8765);
        assert!(a.contains(&"8765".to_string()));
        assert!(!a.contains(&"4567".to_string()));
    }

    #[test]
    fn args_args_match_expected_sequence() {
        // 全量断言：保证未来重构不会偷偷改字段顺序（顺序对 iptables 解析关键）
        let a = iptables_rule_args(4567);
        let expected = vec![
            "-p", "tcp", "--dport", "4567", "!", "-i", "lo", "-j", "DROP",
        ];
        let expected: Vec<String> = expected.into_iter().map(String::from).collect();
        assert_eq!(a, expected);
    }

    #[test]
    fn iptables_guard_satisfies_trait() {
        let _g: Box<dyn LoopbackGuard> = Box::new(IptablesGuard);
    }
}
