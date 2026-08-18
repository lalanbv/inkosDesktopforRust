//! strangler 只读面双端对跑（117 号）——98 号 runbook 第一步的自动化实跑。
//!
//! 同一 fixture 项目根下分别起动 **TS sidecar 真实进程**（tsx + studio
//! server.ts）与 **Rust engine 进程内全量路由**（`router_books`），对只读
//! 端点族逐个请求并做**结构化 JSON 对比**（键序无关；波动字段白名单归一）。
//! LLM 端点两侧同指进程内 mock（对跑不依赖真实 key）。
//!
//! 运行：`INKOS_DUEL=1 cargo test --test strangler_duel -- --nocapture`
//! （缺 env 直接跳过——不进基线六项）。

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn duel_enabled() -> bool {
    std::env::var("INKOS_DUEL").is_ok()
}

fn repo_root() -> PathBuf {
    // tests/ 的上两级 = 仓库根。
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}

fn write_fixture(root: &Path, llm: &str) {
    std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
    std::fs::write(
        root.join("assets").join("genres").join("xianxia.md"),
        "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\",\"高潮章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
    )
    .unwrap();
    let book = root.join("books").join("b1");
    std::fs::create_dir_all(book.join("chapters")).unwrap();
    std::fs::create_dir_all(book.join("story").join("outline")).unwrap();
    std::fs::write(
        book.join("book.json"),
        r#"{"id":"b1","title":"对跑书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z"}"#,
    )
    .unwrap();
    std::fs::write(
        book.join("chapters").join("0001_风起.md"),
        "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。",
    )
    .unwrap();
    std::fs::write(
        book.join("chapters").join("index.json"),
        r#"[{"number":1,"title":"风起","status":"ready-for-review","wordCount":18,"createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]}]"#,
    )
    .unwrap();
    std::fs::write(
        book.join("story").join("story_bible.md"),
        "# 故事圣经\n\n主角林动。",
    )
    .unwrap();
    std::fs::write(
        book.join("story").join("outline").join("story_frame.md"),
        "## 分岔点\n开篇之前。",
    )
    .unwrap();
    std::fs::write(
        root.join("inkos.json"),
        format!(
            r#"{{"version":"0.1.0","name":"对跑项目","language":"zh","llm":{{"services":{{"custom:Duel":{{"service":"custom","name":"Duel","baseUrl":"{llm}","apiFormat":"chat","stream":false}}}},"defaultModel":"duel-model"}},"notify":[]}}"#
        ),
    )
    .unwrap();
    std::fs::create_dir_all(root.join(".inkos")).unwrap();
    std::fs::write(
        root.join(".inkos").join("secrets.json"),
        r#"{"services":{"custom:Duel":{"apiKey":"duel-key"}}}"#,
    )
    .unwrap();
}

/// 进程内 mock LLM（对跑不依赖真实端点——两侧读同一 inkos.json）。
async fn spawn_mock_llm() -> String {
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(|axum::Json(_body): axum::Json<Value>| async move {
            axum::Json(serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": "OK" } }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    format!("http://{addr}")
}

/// Rust engine 进程内全量路由（bin 同款装配）。
async fn spawn_rust_engine(root: &Path, llm: &str) -> String {
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::write_next_route::WriteNextRuntime;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    let hub = std::sync::Arc::new(inkos_engine::server::sse::BroadcastHub::new());
    let state = std::sync::Arc::new(inkos_engine::state::manager::StateManager::new(
        root.to_path_buf(),
    ));
    let router = std::sync::Arc::new(AgentRouter::new(
        LlmEndpointConfig {
            base_url: llm.into(),
            api_key: "duel-key".into(),
            model: "duel-model".into(),
            max_tokens: 8192,
            extra_headers: std::collections::HashMap::new(),
        },
        std::collections::HashMap::new(),
    ));
    let runner: inkos_engine::server::write_next_route::WriteNextRunner =
        std::sync::Arc::new(
            |_state: std::sync::Arc<inkos_engine::state::manager::StateManager>,
             _book_id: String,
             _word_count: Option<u32>,
             _temperature: Option<f64>| {
                Box::pin(async move {
                    Err::<inkos_engine::pipeline::write_next::ChapterPipelineResult, String>(
                        "duel: write face not exercised".to_string(),
                    )
                })
            },
        );
    let write_next = WriteNextRuntime {
        hub: hub.clone(),
        state: state.clone(),
        runner,
        project_root: root.to_path_buf(),
    };
    let audit = inkos_engine::server::audit_route::AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: router.clone(),
        builtin_genres_dir: root.join("assets").join("genres"),
    };
    let books = BooksRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router,
        builtin_genres_dir: root.join("assets").join("genres"),
        revision_gate: Default::default(),
    };
    let app = inkos_engine::server::router_books(
        inkos_engine::server::AppState {
            version: "duel".to_string(),
        },
        hub,
        write_next,
        audit,
        books,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    format!("http://{addr}")
}

/// TS sidecar 真实进程（tsx 直跑 server 入口；dist/index.html 预置跳过前端构建）。
async fn spawn_ts_sidecar(root: &Path, port: u16) -> String {
    let repo = repo_root();
    let studio = repo.join("packages").join("studio");
    // 跳过 vite 前端自动构建：index.ts 以 dist/index.html 存在性判断。
    std::fs::create_dir_all(studio.join("dist")).unwrap();
    std::fs::write(studio.join("dist").join("index.html"), "<!doctype html><title>duel</title>")
        .unwrap();
    let mut command = std::process::Command::new(studio.join("node_modules").join(".bin").join("tsx"))
        .arg(studio.join("src").join("api").join("index.ts"))
        .arg(root)
        .env("INKOS_STUDIO_PORT", port.to_string())
        .env("INKOS_PROJECT_ROOT", root)
        .current_dir(&studio)
        .stdout(std::process::Stdio::from(std::fs::File::create("/tmp/duel-tsx-out.log").unwrap()))
        .stderr(std::process::Stdio::from(std::fs::File::create("/tmp/duel-tsx-err.log").unwrap()))
        .spawn()
        .expect("tsx 起动失败（先确认 packages/studio 依赖已安装）");
    // 测试进程退出前 kill + wait 收割（防僵尸/端口滞留）。
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(600));
        let _ = command.kill();
        let _ = command.wait();
    });
    format!("http://127.0.0.1:{port}")
}

async fn get_json(base: &str, path: &str) -> (u16, Value) {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{base}{path}"))
        .send()
        .await
        .expect("请求失败");
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (status, parsed)
}

/// 波动键归一（递归剥除）：时间戳/版本/路径前缀类。
fn normalize(value: &mut Value) {
    const VOLATILE: [&str; 6] = [
        "startedAt", "completedAt", "timestamp", "updatedAt", "createdAt", "version",
    ];
    if let Value::Object(map) = value {
        for key in VOLATILE {
            map.remove(key);
        }
        for value in map.values_mut() {
            normalize(value);
        }
    } else if let Value::Array(items) = value {
        for item in items.iter_mut() {
            normalize(item);
        }
    }
}

async fn wait_ready(base: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(response) = reqwest::get(format!("{base}/api/v1/books")).await {
            if response.status().is_success() {
                return;
            }
        }
        assert!(Instant::now() < deadline, "{base} 60s 未就绪");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
async fn strangler_readonly_face_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过（基线六项不含本测试）");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    let rust = spawn_rust_engine(&root, &llm).await;
    // sidecar 端口选随机高位（bind 冲突由 readiness 捕获）。
    let ts_port = 4700 + (std::process::id() % 1000) as u16;
    let ts = spawn_ts_sidecar(&root, ts_port).await;
    wait_ready(&rust).await;
    wait_ready(&ts).await;

    // 98 号 runbook 只读桶（GET 面）。
    let endpoints = [
        "/api/v1/books",
        "/api/v1/books/b1",
        "/api/v1/books/b1/chapters/1",
        "/api/v1/books/b1/truth",
        "/api/v1/books/b1/analytics",
        "/api/v1/skills",
        "/api/v1/project",
        "/api/v1/prompt-packs",
        "/api/v1/sessions",
        "/api/v1/logs",
    ];

    let mut diffs: Vec<String> = Vec::new();
    for endpoint in endpoints {
        let (rust_status, mut rust_body) = get_json(&rust, endpoint).await;
        let (ts_status, mut ts_body) = get_json(&ts, endpoint).await;
        normalize(&mut rust_body);
        normalize(&mut ts_body);
        if rust_status != ts_status || rust_body != ts_body {
            diffs.push(format!(
                "DIFF {endpoint}\n  rust[{rust_status}]: {rust_body}\n  ts  [{ts_status}]: {ts_body}"
            ));
        }
    }

    if diffs.is_empty() {
        eprintln!("对跑通过：{} 个只读端点全一致", endpoints.len());
    } else {
        eprintln!("对跑差异 {} 处：", diffs.len());
        for diff in &diffs {
            eprintln!("{diff}");
        }
    }
    // 首轮目标为产出对跑报告——差异清零后此断言转为守门。
    assert!(diffs.is_empty(), "只读面双端存在契约差异（见上）");
}
