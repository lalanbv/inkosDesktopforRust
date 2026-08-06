//! supervisor 集成测：mock HTTP 服务 + 真实 inkos sidecar（`#[ignore]`）。
//!
//! 跑默认（mock）：
//!     cd src-tauri && cargo test --test supervisor_integration
//! 跑真实 inkos（需先 `./scripts/desktop-build-inkos.sh`）：
//!     cd src-tauri && cargo test --test supervisor_integration real_inkos -- --ignored --nocapture

use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

// ---------- mock 集成测：验证 health_probe 行为 ----------

/// 健康路径：起一个假 HTTP 服务返回 200，health_probe 应在 3s 内成功。
#[tokio::test]
async fn health_probe_succeeds_when_server_up() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind 失败");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            if let Ok((mut sock, _)) = listener.accept().await {
                // 即便写失败（连接被关）也无所谓——下一轮 accept 即可。
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        }
    });
    assert!(
        inkos_desktop::supervisor::health_probe(port, Duration::from_secs(3)).await,
        "服务已起，health_probe 应成功"
    );
}

/// 不健康路径：黑洞端口（9 = discard）应在 500ms 内返回 false。
#[tokio::test]
async fn health_probe_times_out_when_no_server() {
    // 端口 9 是 discard 协议；连接通常被立即关闭或拒绝，绝无 HTTP 200。
    let port = 9;
    assert!(
        !inkos_desktop::supervisor::health_probe(port, Duration::from_millis(500)).await,
        "无服务，health_probe 应超时返回 false"
    );
}

// ---------- 真实 inkos sidecar 集成测 ----------

/// 真实拉起 inkos sidecar，验证 SPA 在 60s 内于 :4567 可达，且 kill_tree 释放端口。
///
/// 需本机已 `./scripts/desktop-build-inkos.sh`。标记 `#[ignore]` 是因为：
/// 1. 依赖外部构建产物；
/// 2. 启动真实 HTTP 服务绑 :4567，无法在 CI 上并行。
#[tokio::test]
#[ignore]
async fn real_inkos_sidecar_serves_spa() {
    use inkos_desktop::{
        config, engine::resolve_engine_dir, paths::AppPaths,
        supervisor::{build_launch, spawn, health_probe, kill_tree},
    };
    use std::net::TcpListener;

    // 集成测从 src-tauri/ 跑；CARGO_MANIFEST_DIR = src-tauri（engine 与 Cargo.toml 同级）。
    // dev 模式：resolve_engine_dir(None, src-tauri) = src-tauri/engine（desktop-package-engine.sh 组装）。
    let dev_engine_root = std::env::var("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().expect("cwd"));
    let launch_engine = resolve_engine_dir(None, &dev_engine_root);
    let cli_entry = launch_engine.join(config::CLI_ENTRY_REL);
    assert!(
        cli_entry.exists(),
        "engine 未组装：缺 {}。请先运行 ./scripts/desktop-package-engine.sh",
        cli_entry.display()
    );

    // project_root 用 temp 目录，**对齐生产路径**（main.rs 用
    // `std::env::temp_dir().join("inkos-m1-demo")`）。inkos studio 启动时
    // `findProjectRoot()` 返回 cwd（utils.ts:31 = process.cwd()），cwd 即本测试
    // 设的 project_root；temp dir 无 inkos.json，studio action 会通过
    // `ensureProjectDirectoryInitialized` 自动 init 一个 minimal project
    // （project-bootstrap.ts:155）。本测试正是为了验证生产路径下「temp dir +
    // auto-init」真能拉起 SPA。用独立名字避免与 demo 目录冲突（demo 目录可能
    // 已被 `cargo run` 创建）。
    let project_root = std::env::temp_dir().join("inkos-m1-demo-realtest");
    std::fs::create_dir_all(&project_root)
        .expect("创建 temp project_root 失败");
    let paths = AppPaths::new(project_root, launch_engine).expect("AppPaths 解析失败");
    let port = config::DEFAULT_STUDIO_PORT;

    // 预检：端口未占用。如被占，说明上一次测试的孤儿进程残留。
    if TcpListener::bind(("127.0.0.1", port)).is_err() {
        panic!(":4567 已被占用，请先清理上一次测试的孤儿进程（lsof -i :4567）");
    }

    let spec = build_launch(&paths, port, "node");
    let mut child = spawn(&spec).expect("spawn inkos");

    let ok = health_probe(port, Duration::from_secs(60)).await;

    // 无论健康与否都清理；记录 kill 失败但不阻塞断言。
    let kill_err = kill_tree(&child).err();
    // 等 child 被 reap，避免僵尸。
    let _ = child.wait();

    if let Some(e) = kill_err {
        eprintln!("警告: kill_tree 失败: {e}");
    }

    // 端口释放验证：kill_tree 后应能重新 bind :4567，证明无孤儿 tsx 残留。
    // 给 tsx 一个让出端口的机会（SIGTERM 异步）：最多重试 5s。
    let mut port_released = false;
    for _ in 0..50 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            port_released = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(port_released, "kill_tree 后 :4567 仍被占，孤儿进程残留");

    assert!(ok, "inkos sidecar 应在 60s 内于 :{port} 提供 SPA");
}
