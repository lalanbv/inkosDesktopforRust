//! Rust 引擎后端集成测：真实拉起 inkos-engine-server（绞杀者终切 164 号）。
//!
//! 验证三面：
//! 1. 启动规格 env 契约（INKOS_PORT/INKOS_PROJECT_ROOT）真的被 bin 消费——
//!    服务在指定端口起来、`/api/v1/health` 恒 200；
//! 2. 纯 API 模式（无 INKOS_STATIC_DIR）`/` 404——health 探测路径须用
//!    `rustbin::HEALTH_PROBE_PATH` 而非 `/` 的依据；
//! 3. kill_tree 释放端口（进程组 SIGTERM）。
//!
//! 前置：`cd engine-rs && cargo build --release`。二进制缺失时**跳过**（打印
//! 提示、返回 Ok）——不依赖外部构建产物的环境（首次 clone / 只改壳层）不 FAIL。
//!
//! 运行：cd src-tauri && cargo test --test rust_backend_launch -- --nocapture

use std::path::PathBuf;
use std::time::Duration;

use inkos_desktop::engine::rustbin;
use inkos_desktop::paths::AppPaths;
use inkos_desktop::supervisor::{self, health_probe, kill_tree, spawn};

/// 仓根 = src-tauri/..（测试工作目录 = src-tauri）。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri 应有父目录")
        .to_path_buf()
}

/// dev 解析链：engine-rs/target/{release,debug}（debug_assertions 分支）。
fn find_server_bin() -> Option<PathBuf> {
    rustbin::resolve_server_bin(None, None, &repo_root())
}

fn probe_client() -> reqwest::Client {
    reqwest::Client::builder()
        // 1583 号：默认构造无条件读 env/系统代理（auto_sys_proxy→macOS
        // SCDynamicStore，不读 ExceptionsList）——probe 过（health_probe 谓词
        // 面已旁路）而本 client 的 GET 被劫持去 127.0.0.1:1082：代理转发回环
        // 成功时绿（劫持不可见），代理对回环 RST 时假红（ConnectionReset 的
        // "peer" 是代理不是引擎）。测试目标恒回环——同 health_probe（1572）
        // 语义，.no_proxy() 恒直连。
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest Client 构建失败")
}

#[tokio::test]
async fn rust_engine_serves_health_under_env_contract() {
    let Some(bin) = find_server_bin() else {
        eprintln!("SKIP: inkos-engine-server 未构建（cd engine-rs && cargo build --release）");
        return;
    };

    let project = tempfile::tempdir().unwrap();
    // AppPaths 只组路径不碰盘；engine 目录参数给同 temp（Rust 路径不消费它）。
    std::fs::create_dir_all(project.path().join("engine")).unwrap();
    let paths = AppPaths::new(project.path().to_path_buf(), project.path().join("engine"))
        .expect("AppPaths 解析失败");

    let port = supervisor::pick_free_port(4567).expect("无空闲端口");
    // 纯 API 模式：不注入 INKOS_STATIC_DIR（构建产物缺失的兜底形态）。
    let spec = rustbin::build_launch(&paths, port, &bin, None, None);
    assert!(!spec.env.contains_key("INKOS_STATIC_DIR"));

    let mut child = spawn(&spec).expect("spawn inkos-engine-server 失败");

    // 健康探测：恒 200 的超集端点（15s 预算——冷启首跑含动态链接加载）。
    assert!(
        health_probe(port, Duration::from_secs(15), rustbin::HEALTH_PROBE_PATH).await,
        "inkos-engine-server 应在指定端口健康（INKOS_PORT 契约）"
    );

    // 响应体契约：{"ok":true,"version":"..."}（145 号修复后的编译期版本宏）。
    // 失败分支打印引擎进程状态（1583 号：probe 200 后 GET 瞬时 ConnectionReset
    // 一次——死因三分类：Some(exit 0)=自退（watchdog/serve Err）、Some(signal)
    // =外部杀、None=仍运行（纯网络竞态）。stdio null 化（1431）令引擎自报
    // 不可达，此观测面是死因唯一直观面。
    let body: serde_json::Value = match probe_client()
        .get(format!("http://127.0.0.1:{port}/api/v1/health"))
        .send()
        .await
    {
        Ok(resp) => resp.json().await.expect("health 响应应为 JSON"),
        Err(e) => {
            let status = child.try_wait().ok().flatten();
            panic!("health 请求失败: {e}; 引擎进程状态: {status:?}（None=仍运行）");
        }
    };
    assert_eq!(body["ok"], serde_json::json!(true));
    assert!(
        body["version"]
            .as_str()
            .is_some_and(|v: &str| !v.is_empty()),
        "version 不应为空: {body}"
    );

    // 纯 API 模式 `/` 404 —— 探测路径不可用 `/` 的实证。
    let root_status = probe_client()
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await
        .expect("/ 请求失败")
        .status();
    assert_eq!(root_status.as_u16(), 404, "无静态面时 / 应 404");

    // 进程组清理 + 端口释放。
    kill_tree(&child).expect("kill_tree 失败");
    let _ = child.wait();
    let released = std::net::TcpListener::bind(("127.0.0.1", port)).is_ok();
    assert!(released, "kill_tree 后端口 {port} 应释放");
}

#[tokio::test]
async fn rust_engine_serves_spa_when_static_dir_set() {
    let Some(bin) = find_server_bin() else {
        eprintln!("SKIP: inkos-engine-server 未构建（cd engine-rs && cargo build --release）");
        return;
    };
    // 前端产物缺失（未跑 desktop-build-inkos.sh）→ 跳过静态面变体。
    let dist = repo_root().join("packages").join("studio").join("dist");
    if !dist.join("index.html").is_file() {
        eprintln!("SKIP: packages/studio/dist 缺失（./scripts/desktop-build-inkos.sh）");
        return;
    }

    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join("engine")).unwrap();
    let paths = AppPaths::new(project.path().to_path_buf(), project.path().join("engine"))
        .expect("AppPaths 解析失败");

    // 端口基座位移：与首个测试的 [4567,5567) 区间不相交——并行跑时避开
    // pick_free_port 的 TOCTOU 缝隙（两测试同瞬探测得同一端口，后绑者退出，
    // 轮询的却是先绑者的纯 API 404——实测踩中）。
    let port = supervisor::pick_free_port(5600).expect("无空闲端口");
    let spec = rustbin::build_launch(&paths, port, &bin, Some(&dist), None);
    assert_eq!(
        spec.env.get("INKOS_STATIC_DIR").unwrap(),
        &dist.to_string_lossy().into_owned()
    );

    let mut child = spawn(&spec).expect("spawn inkos-engine-server 失败");

    // 静态面模式 `/` = SPA index（200 + text/html）——webview 导航目标可达。
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let mut spa_ok = false;
    while std::time::Instant::now() < deadline {
        if let Ok(resp) = probe_client()
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
        {
            if resp.status().is_success()
                && resp
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|ct| ct.starts_with("text/html"))
            {
                spa_ok = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // 死因观测面（1583 号，同测试 A）：超时失败时区分引擎已亡（自退/外部杀）
    // 与仍运行（静态面未就绪竞态）。stderr null 化下这是死因唯一直观面。
    if !spa_ok {
        let status = child.try_wait().ok().flatten();
        panic!(
            "静态面模式 / 应返回 SPA index（text/html）; 引擎进程状态: {status:?}（None=仍运行）"
        );
    }

    kill_tree(&child).expect("kill_tree 失败");
    let _ = child.wait();
}
