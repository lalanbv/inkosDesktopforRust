//! parent-watchdog 进程级活体（1459）：二进制带 `INKOS_PARENT_PID=<瞬逝
//! 进程>` 自起即退——env 契约 → 探测 → 自退全链真跑（非单元面 mock）。
//! stderr 断言「parent ... gone」防 bind 失败等其他退出路径假绿。
//!
//! 装配形态：assert_cmd `cargo_bin`+timeout（与 src-tauri e2e_startup 同款
//! 阻塞语义——timeout 到期即 kill 并返回已收输出）。

use assert_cmd::Command as BinCommand;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

/// 瞬逝父进程：`sleep 0.3`（程序与参数静态字面量）；测试侧 wait 收尸后
/// 其 pid 对引擎侧探测呈已亡（不留 zombie）。
fn short_lived_parent() -> std::process::Child {
    Command::new("sleep")
        .arg("0.3")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn engine_exits_when_injected_parent_dies() {
    let mut parent = short_lived_parent();
    let parent_pid = parent.id();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    let mut engine = BinCommand::cargo_bin("inkos-engine-server").unwrap();
    engine
        .env("INKOS_PORT", port.to_string())
        .env("INKOS_PARENT_PID", parent_pid.to_string())
        .timeout(Duration::from_secs(10)); // 首轮探测 ≤2s+启动余量

    // 先让父亡+收尸（引擎在 bind 前 watchdog 已起，随探测轮自退）。
    parent.wait().unwrap();
    std::thread::sleep(Duration::from_millis(200)); // 越过收尸可见窗口

    let output = engine.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // 自退路径断言：stderr 含 watchdog 退出行——区分 bind 失败/timeout 强杀。
    assert!(
        stderr.contains(&format!("parent {parent_pid} gone")),
        "stderr 应含 watchdog 退出行，实得：{stderr}"
    );
    assert!(
        output.status.success(),
        "watchdog 自退应为 exit 0，实得 {}",
        output.status
    );
}

/// 对照面：env 缺失=功能关闭——引擎常驻直到 timeout 强杀，且无自退行。
#[test]
fn engine_stays_without_env() {
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    let mut engine = BinCommand::cargo_bin("inkos-engine-server").unwrap();
    engine
        .env("INKOS_PORT", port.to_string())
        .env_remove("INKOS_PARENT_PID")
        .timeout(Duration::from_secs(3)); // > 轮询周期 2s 的至少一轮

    let output = engine.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("gone, exiting"),
        "env 缺失时不应有 watchdog 自退行，实得：{stderr}"
    );
    assert!(
        !output.status.success(),
        "env 缺失时引擎应存活到 timeout 强杀（非零退出），实得 {}",
        output.status
    );
}
