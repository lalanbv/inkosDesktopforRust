//! macOS 实现：用 pf anchor 把外部入站挡在 `<port>` 之外，放行 loopback。
//!
//! ## 方案
//! 用 anchor（`pfctl -a "inkosdesktop" -f`）而非全局 pf 规则，避免污染用户
//! 现有 pf 配置。anchor 内只放两条规则：block 入站非 lo + pass lo。
//!
//! ## 权限
//! `pfctl` 需要 root。M1 不假设 app 以 root 跑——失败时 [`PfGuard::lock`]
//! 返回 `Err`，`main.rs` 仅 log 警告、不阻塞启动。完整运行时强制待 M2/M3
//! 的 SMJOP / launchd 特权 helper。
//!
//! ## 为什么不用 `block in on en0 ...`
//! 接口名跨机器不固定（笔记本可能在 en0/wlan0/...）。anchor 内
//! `block in proto tcp from !lo0 to any port = <port>` 按接口语义而非名字
//! 区分：除 loopback (`lo0`) 外任何接口的入站都挡。
//!
//! ## 关于 dual-stack
//! Task 7 Step 1 实测 `lsof` 显示 `*:4567 (LISTEN)`（IPv6 socket 同时接
//! IPv4）。pf 规则在 packet 层生效，与 socket 协议族无关，故一条规则即可
//! 覆盖 IPv4 + IPv6 入站。

use super::LoopbackGuard;
use std::io::Write;
use std::process::{Command, Stdio};

/// pf anchor 名——`pfctl -a` 用此定位规则集合。
pub const ANCHOR_NAME: &str = "inkosdesktop";

/// pf anchor 规则集（纯函数，便于单测断言内容）。
///
/// 规则语义：
/// 1. `pass in quick on lo0` —— loopback 全放行（quick 跳过后续规则）。
/// 2. `block in proto tcp from !lo0 to any port = <port>` —— 非 lo0 接口的
///    TCP 入站到 `<port>` 全挡。
///
/// 返回的串以换行结尾，可直接 pipe 给 `pfctl -f /dev/stdin`。
///
/// # 示例
/// ```
/// # use inkos_desktop::isolation::macos::pf_anchor_rules;
/// let s = pf_anchor_rules(4567);
/// assert!(s.contains("pass in quick on lo0"));
/// assert!(s.contains("block in proto tcp from !lo0 to any port = 4567"));
/// ```
pub fn pf_anchor_rules(port: u16) -> String {
    format!(
        "pass in quick on lo0 proto tcp to any port = {port}\n\
         block in quick proto tcp from !lo0 to any port = {port}\n"
    )
}

/// macOS pf anchor guard。
pub struct PfGuard;

impl LoopbackGuard for PfGuard {
    fn lock(&self, port: u16) -> anyhow::Result<()> {
        let rules = pf_anchor_rules(port);
        // `pfctl -a <anchor> -f -` 从 stdin 读规则集。需要 root；非 root
        // 跑会得到 "Operation not permitted"——见模块级降级策略。
        let mut child = Command::new("/sbin/pfctl")
            .args(["-a", ANCHOR_NAME, "-f", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("pfctl spawn 失败 (需 root? {e})"))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(rules.as_bytes())
                .map_err(|e| anyhow::anyhow!("写入 pfctl stdin 失败: {e}"))?;
        }
        // drop stdin 触发 pfctl 解析并退出
        let output = child
            .wait_with_output()
            .map_err(|e| anyhow::anyhow!("wait pfctl 失败: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "pfctl 加载 anchor 失败 (exit={}): {}",
                output.status,
                stderr.trim()
            );
        }
        Ok(())
    }

    fn release(&self, _port: u16) -> anyhow::Result<()> {
        // anchor 一致，故 release 不需 port 参数——清空整个 anchor 即可。
        // `-a inkosdesktop -F rules` 清规则集；幂等（anchor 不存在时静默退出）。
        let status = Command::new("/sbin/pfctl")
            .args(["-a", ANCHOR_NAME, "-F", "rules"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| anyhow::anyhow!("pfctl (release) spawn 失败: {e}"))?;
        if !status.success() {
            // 退出码非 0 通常表示 anchor 已不存在——幂等语义下不视作错误。
            eprintln!(
                "[isolation::macos] release: pfctl -F rules exit={} (视为幂等成功)",
                status
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_pass_loopback_for_port() {
        let s = pf_anchor_rules(4567);
        assert!(
            s.contains("pass in quick on lo0 proto tcp to any port = 4567"),
            "应放行 lo0 入站到指定端口，实际: {s}"
        );
    }

    #[test]
    fn rules_block_non_loopback_for_port() {
        let s = pf_anchor_rules(4567);
        assert!(
            s.contains("block in quick proto tcp from !lo0 to any port = 4567"),
            "应挡非 lo0 入站到指定端口，实际: {s}"
        );
    }

    #[test]
    fn rules_embed_port_correctly() {
        // 换个端口确认规则不是硬编码 4567
        let s = pf_anchor_rules(8765);
        assert!(s.contains("port = 8765"), "规则应含端口 8765，实际: {s}");
        assert!(!s.contains("port = 4567"), "规则不应误含 4567");
    }

    #[test]
    fn rules_end_with_newline_for_pfctl_pipe() {
        // pfctl -f - 要求规则集以换行结尾
        let s = pf_anchor_rules(4567);
        assert!(s.ends_with('\n'), "规则应以 \\n 结尾");
    }

    #[test]
    fn anchor_name_is_stable() {
        // release 依赖此 anchor 名清规则——稳定性即契约
        assert_eq!(ANCHOR_NAME, "inkosdesktop");
    }

    /// Box<dyn LoopbackGuard> 派发路径——保证 PfGuard 满足 trait 约束。
    #[test]
    fn pf_guard_satisfies_trait() {
        let _g: Box<dyn LoopbackGuard> = Box::new(PfGuard);
    }
}
