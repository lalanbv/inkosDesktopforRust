//! loopback 加固：把 inkos sidecar 默认绑的 `0.0.0.0:<port>` 锁回 `127.0.0.1`。
//!
//! ## 背景
//! 架构 §8 / §14.1：inkos Studio 经 `@hono/node-server` 默认监听所有接口
//! （Task 7 Step 1 已实测：`lsof` 显示 `*:4567 (LISTEN)`，同网段 `curl
//! http://<LAN-IP>:4567/` 返回 200）。本模块在 sidecar spawn **之前**调用
//! 平台防火墙，拒绝外部到 `<port>` 的入站，放行 loopback。
//!
//! ## 运行时强制（M1 范围声明）
//! `pfctl` / `iptables` / `netsh advfirewall` 均**需 root/管理员权限**。
//! M1 不能假设 app 以 root 跑，故 [`LoopbackGuard::lock`] 与 [`release`]
//! 采用**优雅降级**策略：
//! 1. 失败（如权限不足 / 命令缺失）→ 显式 `eprintln!` 警告，返回 `Err`；
//! 2. 调用方（`main.rs` setup）**忽略 Err 只 log**，绝不让 app 启动崩溃；
//! 3. 完整运行时强制（特权 helper：SMJOP / launchd / setuid）待 M2/M3。
//!
//! ## 测试策略
//! `pfctl` / `iptables` / `netsh` 不可在 CI/无 root 环境跑。规则生成抽成
//! 纯函数（[`macos::pf_anchor_rules`] / [`linux::iptables_rules`] /
//! [`windows::win_firewall_rule_name`]），单测断言规则串内容覆盖 ≥80%
//! 行；`platform_guard()` 仅断言可构造、不实际 lock。

pub mod linux;
pub mod macos;
pub mod windows;

/// 平台无关抽象：把端口锁回 loopback。
///
/// 实现侧（`macos::PfGuard` 等）通过平台防火墙完成实际工作；调用方
/// 仅需 `lock` / `release` 两个动作。失败语义见模块级文档「运行时强制」。
pub trait LoopbackGuard: Send + Sync {
    /// 拒绝外部入站到 `port`，放行 loopback。
    ///
    /// 返回 `Err` 表示**未能**完成加固（如权限不足）；调用方应记录并继续，
    /// 不要让 app 启动崩溃——见模块级「优雅降级」约定。
    fn lock(&self, port: u16) -> anyhow::Result<()>;

    /// 移除 `lock` 安装的规则（best-effort，幂等）。
    fn release(&self, port: u16) -> anyhow::Result<()>;
}

/// 按编译目标平台派发具体实现。
///
/// `Box<dyn LoopbackGuard>` 而非泛型：`main.rs` 单点派发、无需在调用处
/// 模板化；`Send + Sync` 让句柄可存入 Tauri managed state（与 `SidecarState`
/// 一致契约）。
#[cfg(target_os = "macos")]
pub fn platform_guard() -> Box<dyn LoopbackGuard> {
    Box::new(macos::PfGuard)
}

#[cfg(target_os = "linux")]
pub fn platform_guard() -> Box<dyn LoopbackGuard> {
    Box::new(linux::IptablesGuard)
}

#[cfg(target_os = "windows")]
pub fn platform_guard() -> Box<dyn LoopbackGuard> {
    Box::new(windows::FirewallGuard)
}

// 编译期断言：所有 LoopbackGuard 实现必须满足 Send + Sync，
// 否则无法装进 `Box<dyn LoopbackGuard>` 并存入 Tauri managed state。
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<macos::PfGuard>();
    assert_send_sync::<linux::IptablesGuard>();
    assert_send_sync::<windows::FirewallGuard>();
};

#[cfg(test)]
mod tests {
    use super::*;

    /// 仅断言当前编译平台的 guard 可构造、可装进 trait object。
    /// 不实际 lock（需 root），只验证派发逻辑。
    #[test]
    fn platform_guard_constructible() {
        let _g: Box<dyn LoopbackGuard> = platform_guard();
    }

    /// Box 显式Drop：保证 guard 句柄在 Box::new 后能正常析构（覆盖 Send+Sync 装箱路径）。
    #[test]
    fn platform_guard_can_be_dropped_without_release() {
        {
            let _g = platform_guard();
            // 不调 release 直接 drop——验证不泄漏、不 panic。
        }
    }
}
