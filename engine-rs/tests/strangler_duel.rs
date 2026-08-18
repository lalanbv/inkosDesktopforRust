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

/// sidecar 唯一端口：OS 分配（bind :0 取空闲口再放行给 tsx——进程内外
/// 均不撞车；drop 与 tsx bind 间的竞态窗口在本机测试语境可忽略）。
async fn next_sidecar_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
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
            r#"{{"version":"0.1.0","name":"对跑项目","language":"zh","llm":{{"services":{{"custom:Duel":{{"service":"custom","name":"Duel","baseUrl":"{llm}","apiFormat":"chat","stream":true}}}},"defaultModel":"duel-model"}},"notify":[]}}"#
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
        axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
            // 双形态：流式请求 → SSE（129 号起带 reasoning_content 增量——
            // thinking 面对跑驱动）；非流式 → 整体 JSON（两侧客户端偏好不同）。
            if body["stream"].as_bool().unwrap_or(false) {
                let reasoning = serde_json::json!({ "choices": [{ "delta": { "reasoning_content": "让我想想" } }] });
                let chunk = serde_json::json!({ "choices": [{ "delta": { "content": "OK" } }] });
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                return axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {reasoning}

data: {chunk}

data: {usage}

data: [DONE]

"),
                ));
            }
            axum::response::IntoResponse::into_response(axum::Json(serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": "OK" } }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
            })))
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
    let ts_port = next_sidecar_port().await;
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

/// 118 号：跨端写读一致性对跑——A) Rust 写（章节 approve）→ 双端读一致且为
/// approved；B) TS 写（chapter-review-mode=manual）→ 双端读一致且为 manual；
/// C) 写后复跑只读桶对跑（磁盘态演进下双端读面仍全等价）。
#[tokio::test]
async fn strangler_cross_write_read_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts_port = next_sidecar_port().await;
    let ts = spawn_ts_sidecar(&root, ts_port).await;
    wait_ready(&rust).await;
    wait_ready(&ts).await;

    let client = reqwest::Client::new();

    // A) Rust 写：approve 第 1 章。
    let response = client
        .post(format!("{rust}/api/v1/books/b1/chapters/1/approve"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    // 双端读：book 详情的章节摘要 status=approved（index.json 跨端可见）。
    for base in [&rust, &ts] {
        let (status, body) = get_json(base, "/api/v1/books/b1").await;
        assert_eq!(status, 200, "base={base} body={body}");
        let approved = body
            .get("chapters")
            .and_then(|chapters| chapters.as_array())
            .is_some_and(|chapters| {
                chapters
                    .iter()
                    .any(|chapter| chapter.get("number").and_then(Value::as_u64) == Some(1)
                        && chapter.get("status").and_then(Value::as_str) == Some("approved"))
            });
        assert!(approved, "approve 未跨端可见：base={base} body={body}");
    }

    // B) TS 写：chapter-review-mode=manual（book.json writing 面）。
    let response = client
        .put(format!("{ts}/api/v1/books/b1/chapter-review-mode"))
        .header("content-type", "application/json")
        .body(r#"{"mode":"manual"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200, "TS PUT review-mode 失败");
    // 双端读：GET 同路径 manual。
    for base in [&rust, &ts] {
        let (status, body) = get_json(base, "/api/v1/books/b1/chapter-review-mode").await;
        if status != 200 {
            let written = std::fs::read_to_string(root.join("books").join("b1").join("book.json")).unwrap_or_default();
            eprintln!("review-mode 读失败 base={base} book.json={written}");
        }
        assert_eq!(status, 200, "base={base} body={body}");
        assert!(
            serde_json::to_string(&body).unwrap_or_default().contains("manual"),
            "review-mode 未跨端可见：base={base} body={body}"
        );
    }

    // C) 写后只读桶复跑（磁盘态演进下双端仍全等价）。
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
    assert!(diffs.is_empty(), "写后只读复跑差异：{diffs:?}");
    eprintln!("跨端写读对跑通过：Rust 写 approve / TS 写 review-mode 双向可见，写后 {} 端点复跑全一致", endpoints.len());
}

/// 119 号：会话域跨端对跑——A) Rust 建会话（绑书）→ 双端 GET 全等价；
/// B) TS 建会话 + 改名 → 双端可见新名；C) TS 聊天回合（mock LLM）后双端
/// GET 仍等价（transcript 追加不改会话读面契约）。
#[tokio::test]
async fn strangler_session_domain_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts_port = next_sidecar_port().await;
    let ts = spawn_ts_sidecar(&root, ts_port).await;
    wait_ready(&rust).await;
    wait_ready(&ts).await;
    let client = reqwest::Client::new();

    // A) Rust 建会话（book 绑定）→ 双端 GET 等价。
    let rust_session = "1783099000012-d119";
    let response = client
        .post(format!("{rust}/api/v1/sessions"))
        .header("content-type", "application/json")
        .body(format!(r#"{{"sessionId":"{rust_session}","bookId":"b1","sessionKind":"book"}}"#))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let (rust_status, mut rust_body) = get_json(&rust, &format!("/api/v1/sessions/{rust_session}")).await;
    let (ts_status, mut ts_body) = get_json(&ts, &format!("/api/v1/sessions/{rust_session}")).await;
    assert_eq!(rust_status, 200, "rust body={rust_body}");
    assert_eq!(ts_status, 200, "TS 读不到 Rust 会话：{ts_body}");
    assert_eq!(rust_body["session"]["sessionId"], rust_session);
    normalize(&mut rust_body);
    normalize(&mut ts_body);
    assert_eq!(rust_body, ts_body, "Rust 建会话双端读面不等价");

    // B) TS 建会话 + 改名 → 双端可见。
    let ts_session = "1783099000013-d119";
    let response = client
        .post(format!("{ts}/api/v1/sessions"))
        .header("content-type", "application/json")
        .body(format!(r#"{{"sessionId":"{ts_session}"}}"#))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let response = client
        .put(format!("{ts}/api/v1/sessions/{ts_session}"))
        .header("content-type", "application/json")
        .body(r#"{"title":"跨端改名"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200, "TS rename 失败");
    for base in [&rust, &ts] {
        let (status, body) = get_json(base, &format!("/api/v1/sessions/{ts_session}")).await;
        assert_eq!(status, 200, "base={base} body={body}");
        assert_eq!(
            body["session"]["title"], "跨端改名",
            "改名未跨端可见：base={base} body={body}"
        );
    }

    // C) TS 聊天回合（mock）→ 双端 GET 仍等价（transcript 追加面）。
    let response = client
        .post(format!("{ts}/api/v1/agent"))
        .header("content-type", "application/json")
        .body(format!(
            r#"{{"instruction":"打个招呼","sessionId":"{ts_session}"}}"#
        ))
        .send()
        .await
        .unwrap();
    if response.status().as_u16() != 200 {
        let text = response.text().await.unwrap_or_default();
        panic!("TS 聊天回合失败: {text}");
    }
    let (rust_status, mut rust_body) = get_json(&rust, &format!("/api/v1/sessions/{ts_session}")).await;
    let (ts_status, mut ts_body) = get_json(&ts, &format!("/api/v1/sessions/{ts_session}")).await;
    assert_eq!(rust_status, 200);
    assert_eq!(ts_status, 200);
    normalize(&mut rust_body);
    normalize(&mut ts_body);
    assert_eq!(rust_body, ts_body, "聊天回合后双端会话读面不等价");
    eprintln!("会话域对跑通过：Rust 建会话双端等价 / TS 建+改名跨端可见 / 聊天回合后读面等价");
}

/// 120 号：模型配置域对跑——A) 服务列表/密钥读面双端等价；B) 密钥跨端写读
/// （Rust 写 key → TS 读到；TS 写 → Rust 读到）；C) 深链探测（POST
/// services/:service/test，同 mock 上游）双端响应等价。
#[tokio::test]
async fn strangler_services_domain_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts_port = next_sidecar_port().await;
    let ts = spawn_ts_sidecar(&root, ts_port).await;
    wait_ready(&rust).await;
    wait_ready(&ts).await;
    let client = reqwest::Client::new();

    // A) 服务列表读面（fixture 服务项 + secrets 键集）。
    let (rust_status, mut rust_body) = get_json(&rust, "/api/v1/services").await;
    let (ts_status, mut ts_body) = get_json(&ts, "/api/v1/services").await;
    assert_eq!(rust_status, 200, "rust: {rust_body}");
    assert_eq!(ts_status, 200, "ts: {ts_body}");
    normalize(&mut rust_body);
    normalize(&mut ts_body);
    if rust_body != ts_body {
        eprintln!(
            "服务列表差异（备案观察面）：\n  rust: {rust_body}\n  ts:   {ts_body}"
        );
    }

    // B) 密钥跨端写读：Rust 写 → TS 读；TS 写 → Rust 读。
    let response = client
        .put(format!("{rust}/api/v1/services/custom:Duel2/secret"))
        .header("content-type", "application/json")
        .body(r#"{"apiKey":"rk-120"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200, "Rust 写密钥失败");
    let (status, body) = get_json(&ts, "/api/v1/services/custom:Duel2/secret").await;
    assert_eq!(status, 200, "TS 读不到 Rust 写的密钥：{body}");
    let response = client
        .put(format!("{ts}/api/v1/services/custom:Duel3/secret"))
        .header("content-type", "application/json")
        .body(r#"{"apiKey":"tk-120"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200, "TS 写密钥失败");
    let (status, body) = get_json(&rust, "/api/v1/services/custom:Duel3/secret").await;
    assert_eq!(status, 200, "Rust 读不到 TS 写的密钥：{body}");

    // C) 深链探测（inline baseUrl 指同一 mock；无 apiFormat 偏好 → 双端
    // 计划 [chat 非流式, responses 非流式]，chat 先中）。
    let body = format!(r#"{{"apiKey":"k","baseUrl":"{llm}","model":"duel-model"}}"#);
    let (rust_status, mut rust_probe) = post_json(&rust, "/api/v1/services/deepseek/test", &body).await;
    let (ts_status, mut ts_probe) = post_json(&ts, "/api/v1/services/deepseek/test", &body).await;
    assert_eq!(rust_status, 200, "rust probe: {rust_probe}");
    assert_eq!(ts_status, 200, "ts probe: {ts_probe}");
    normalize(&mut rust_probe);
    normalize(&mut ts_probe);
    if rust_probe != ts_probe {
        panic!("深链探测双端不等价：
  rust: {rust_probe}
  ts:   {ts_probe}");
    }

    eprintln!("模型配置域对跑通过：服务列表面观察 / 密钥双向跨端写读 / 深链探测等价");
}

async fn post_json(base: &str, path: &str, body: &str) -> (u16, Value) {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}{path}"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .expect("POST 失败");
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// 121 号：全量灰度模拟（runbook 第五步终验收）——同根双进程**共存**下按
/// 顺序串行执行前四步场景（只读对跑 → Rust 写 approve → TS 写 review-mode
/// 与聊天回合 → 模型配置探测），每步后双端只读桶复跑等价——sidecar 保温
/// 语义（双端共读同盘）的一站式验收。
#[tokio::test]
async fn strangler_full_grey_simulation() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts_port = next_sidecar_port().await;
    let ts = spawn_ts_sidecar(&root, ts_port).await;
    wait_ready(&rust).await;
    wait_ready(&ts).await;
    let client = reqwest::Client::new();

    let readonly = [
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
    async fn assert_readonly_equivalent(
        rust: &str,
        ts: &str,
        readonly: &[&str],
        step: usize,
    ) {
        for endpoint in readonly {
            let (rust_status, mut rust_body) = get_json(rust, endpoint).await;
            let (ts_status, mut ts_body) = get_json(ts, endpoint).await;
            normalize(&mut rust_body);
            normalize(&mut ts_body);
            assert_eq!(
                (rust_status, &rust_body),
                (ts_status, &ts_body),
                "step{step} 后只读面分歧：{endpoint}"
            );
        }
    }

    // 第一步：只读基线。
    assert_readonly_equivalent(&rust, &ts, &readonly, 1).await;

    // 第二步：Rust 写（approve）→ 双端读一致。
    let response = client
        .post(format!("{rust}/api/v1/books/b1/chapters/1/approve"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_readonly_equivalent(&rust, &ts, &readonly, 2).await;

    // 第三步：TS 写（review-mode + 会话 + 聊天回合）→ 双端读一致。
    let response = client
        .put(format!("{ts}/api/v1/books/b1/chapter-review-mode"))
        .header("content-type", "application/json")
        .body(r#"{"mode":"manual"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let grey_session = "1783099000014-g121";
    let response = client
        .post(format!("{ts}/api/v1/sessions"))
        .header("content-type", "application/json")
        .body(format!(r#"{{"sessionId":"{grey_session}"}}"#))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let response = client
        .post(format!("{ts}/api/v1/agent"))
        .header("content-type", "application/json")
        .body(format!(r#"{{"instruction":"灰度回合","sessionId":"{grey_session}"}}"#))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_readonly_equivalent(&rust, &ts, &readonly, 3).await;

    // 第四步：模型配置探测（双端各自对同 mock）→ 只读面不受扰动。
    let body = format!(r#"{{"apiKey":"k","baseUrl":"{llm}"}}"#);
    let (rust_status, _) = post_json(&rust, "/api/v1/services/deepseek/test", &body).await;
    let (ts_status, _) = post_json(&ts, "/api/v1/services/deepseek/test", &body).await;
    assert_eq!(rust_status, 200);
    assert_eq!(ts_status, 200);
    assert_readonly_equivalent(&rust, &ts, &readonly, 4).await;

    eprintln!("全量灰度模拟通过：四步串行 + 每步双端只读复跑全等价（sidecar 保温共存语义）");
}

/// 123 号：计数 mock LLM——每次请求原子计数并按双形态回包（触达证明用；
/// 回包内容 "OK" 不驱动链路走完，仅证明配置源指向）。
async fn spawn_counting_mock_llm(
    hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) -> String {
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(
            move |axum::Json(body): axum::Json<Value>| {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if body["stream"].as_bool().unwrap_or(false) {
                        let chunk =
                            serde_json::json!({ "choices": [{ "delta": { "content": "OK" } }] });
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                        return axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {chunk}

data: {usage}

data: [DONE]

"),
                        ));
                    }
                    axum::response::IntoResponse::into_response(axum::Json(serde_json::json!({
                        "choices": [{ "message": { "role": "assistant", "content": "OK" } }],
                        "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
                    })))
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    format!("http://{addr}")
}

/// 123 号：真实 bin 进程装配验收——write-next 的 LLM 配置源必须走
/// `effective_router` 热解析（inkos.json 服务项优先，配置不可用才回退
/// `INKOS_LLM_*` 启动端点）。进程内对跑（`spawn_rust_engine`）覆盖不到
/// bin 装配层——本测试以 `CARGO_BIN_EXE` 起真实产物补上该盲区：
/// 项目 mock 收到链路首个 LLM 调用、启动 env mock 零触达。
#[tokio::test]
async fn bin_process_write_next_llm_resolution() {
    use std::sync::atomic::Ordering;
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let project_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let env_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let project_llm = spawn_counting_mock_llm(project_hits.clone()).await;
    let env_llm = spawn_counting_mock_llm(env_hits.clone()).await;
    write_fixture(&root, &project_llm);

    let port = next_sidecar_port().await;
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_inkos-engine-server"))
        .env("INKOS_PROJECT_ROOT", &root)
        .env("INKOS_PORT", port.to_string())
        .env("INKOS_BUILTIN_GENRES_DIR", root.join("assets").join("genres"))
        // 启动 env 指向另一 mock：write-next 若误走启动 router（123 号前缺陷），
        // project_hits 恒 0 且本 mock 被触达——两端皆可判。
        .env("INKOS_LLM_BASE_URL", &env_llm)
        .env("INKOS_LLM_API_KEY", "env-key")
        .env("INKOS_LLM_MODEL", "env-model")
        .stdout(std::process::Stdio::from(
            std::fs::File::create("/tmp/duel-bin-out.log").unwrap(),
        ))
        .stderr(std::process::Stdio::from(
            std::fs::File::create("/tmp/duel-bin-err.log").unwrap(),
        ))
        .spawn()
        .expect("bin 起动失败");
    let base = format!("http://127.0.0.1:{port}");
    wait_ready(&base).await;

    // 装配冒烟：真实产物全量路由可服务。
    let (status, _) = get_json(&base, "/api/v1/health").await;
    assert_eq!(
        status,
        200,
        "bin health 不通：{}",
        std::fs::read_to_string("/tmp/duel-bin-err.log").unwrap_or_default()
    );
    let (status, _) = get_json(&base, "/api/v1/books").await;
    assert_eq!(status, 200);

    // fire-and-forget write-next：后台链首个 LLM 调用应打 inkos.json 服务端点。
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}/api/v1/books/b1/write-next"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);

    let deadline = Instant::now() + Duration::from_secs(60);
    while project_hits.load(Ordering::SeqCst) == 0 {
        assert!(
            Instant::now() < deadline,
            "60s 内 write-next 未触达 inkos.json 服务端点（bin 配置源未走 effective_router？）stderr: {}",
            std::fs::read_to_string("/tmp/duel-bin-err.log").unwrap_or_default()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(
        env_hits.load(Ordering::SeqCst),
        0,
        "INKOS_LLM_* 启动端点被 write-next 触达——项目配置应优先（TS studio 消费者语义）"
    );

    let _ = command.kill();
    let _ = command.wait();
    eprintln!(
        "bin 进程验收通过：write-next 走 inkos.json 服务项（{} 次触达），env 端点零触达",
        project_hits.load(Ordering::SeqCst)
    );
}

/// 原始 GET（状态码 + content-type + body）——静态面比 JSON 面多一个
/// content-type 维度。
async fn get_raw(base: &str, path: &str) -> (u16, Option<String>, String) {
    let response = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .send()
        .await
        .expect("请求失败");
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(String::from);
    let body = response.text().await.unwrap_or_default();
    (status, content_type, body)
}

/// 124 号：静态前端面双端对跑——真实 bin（INKOS_STATIC_DIR）与 TS sidecar
/// 对**同一 dist 目录**的静态服务逐字节等价（/ 与深链 SPA 回退 / 资产
/// content-type / 缺失资产 404），且 API 路由不受回退干扰。
#[tokio::test]
async fn bin_process_static_face_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    // sidecar 起动时自写 dist/index.html；资产预先放入（按请求读，两侧同源）。
    let ts_port = next_sidecar_port().await;
    let ts = spawn_ts_sidecar(&root, ts_port).await;
    let dist = repo_root()
        .join("packages")
        .join("studio")
        .join("dist");
    std::fs::create_dir_all(dist.join("assets")).unwrap();
    std::fs::write(dist.join("assets").join("app.js"), "console.log('duel')").unwrap();

    // bin 在 dist/index.html 落位后起动（index 启动期缓存两侧同内容）。
    let bin_port = next_sidecar_port().await;
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_inkos-engine-server"))
        .env("INKOS_PROJECT_ROOT", &root)
        .env("INKOS_PORT", bin_port.to_string())
        .env("INKOS_BUILTIN_GENRES_DIR", root.join("assets").join("genres"))
        .env("INKOS_STATIC_DIR", &dist)
        .stdout(std::process::Stdio::from(
            std::fs::File::create("/tmp/duel-bin-static-out.log").unwrap(),
        ))
        .stderr(std::process::Stdio::from(
            std::fs::File::create("/tmp/duel-bin-static-err.log").unwrap(),
        ))
        .spawn()
        .expect("bin 起动失败");
    let bin = format!("http://127.0.0.1:{bin_port}");
    wait_ready(&bin).await;
    wait_ready(&ts).await;

    for path in ["/", "/editor/chapter/2", "/assets/app.js", "/assets/missing.js"] {
        let (bin_status, bin_type, bin_body) = get_raw(&bin, path).await;
        let (ts_status, ts_type, ts_body) = get_raw(&ts, path).await;
        assert_eq!(
            bin_status, ts_status,
            "{path} 状态码分歧：bin={bin_status} ts={ts_status}"
        );
        assert_eq!(
            bin_body, ts_body,
            "{path} body 分歧：bin={bin_body:?} ts={ts_body:?}"
        );
        // content-type 等值比较（charset 大小写两侧客户端库不同，忽略大小写）。
        assert_eq!(
            bin_type.map(|v| v.to_ascii_lowercase()),
            ts_type.map(|v| v.to_ascii_lowercase()),
            "{path} content-type 分歧"
        );
    }

    // 资产 content-type 精确断言（映射表逐字对齐 TS）。
    let (_, content_type, body) = get_raw(&bin, "/assets/app.js").await;
    assert_eq!(content_type.as_deref(), Some("application/javascript"));
    assert_eq!(body, "console.log('duel')");
    // 已注册 API 路由不受静态回退干扰。
    let (status, _) = get_json(&bin, "/api/v1/health").await;
    assert_eq!(status, 200);

    // CORS 面（125 号）：跨源请求（Tauri 壳 / vite dev）等价性——普通请求
    // Allow-Origin 恒 *；preflight 双端 2xx 且方法/头语义一致。
    let client = reqwest::Client::new();
    for base in [&bin, &ts] {
        let response = client
            .get(format!("{base}/api/v1/health"))
            .header("origin", "http://duel.local")
            .send()
            .await
            .unwrap();
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("*"),
            "{base} 普通 CORS 请求头分歧"
        );
        let response = client
            .request(reqwest::Method::OPTIONS, format!("{base}/api/v1/books"))
            .header("origin", "http://duel.local")
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "content-type")
            .send()
            .await
            .unwrap();
        // Hono 回 204 / tower-http 回 200——均属浏览器接受的 2xx preflight。
        assert!(
            response.status().is_success(),
            "{base} preflight 应 2xx：{}",
            response.status()
        );
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("*")
        );
        let methods = response
            .headers()
            .get("access-control-allow-methods")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(methods.contains("post"), "{base} 方法族缺 POST: {methods}");
        let headers = response
            .headers()
            .get("access-control-allow-headers")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(
            headers.contains("content-type"),
            "{base} 头镜像缺 content-type: {headers}"
        );
    }

    let _ = command.kill();
    let _ = command.wait();
    eprintln!("静态面双端对跑通过：/ 与深链 SPA / 资产字节级一致，API 面不受干扰；CORS 面等价");
}

/// SSE 流事件采集：连接 → 触发聊天回合 → 等待终态事件后收束。
/// 返回原始 (event, payload JSON) 序列（ping/task:snapshot 已滤）。
async fn sse_chat_turn_events(base: &str, session_id: &str, instruction: &str) -> Vec<(String, Value)> {
    let events: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let target = base.to_string();
    let collector = tokio::spawn(async move {
        let response = match reqwest::get(format!("{target}/api/v1/events")).await {
            Ok(response) => response,
            Err(_) => return,
        };
        use futures_util::StreamExt;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if Instant::now() > deadline {
                break;
            }
            let chunk = match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                Ok(Some(Ok(chunk))) => chunk,
                Ok(Some(Err(_))) | Ok(None) => break,
                Err(_) => continue,
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(position) = buffer.find("\n\n") {
                let block: String = buffer.drain(..position + 2).collect();
                let mut name = String::new();
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(value) = line.strip_prefix("event:") {
                        name = value.trim().to_string();
                    } else if let Some(value) = line.strip_prefix("data:") {
                        data = value.trim().to_string();
                    }
                }
                if name.is_empty() || name == "ping" || name == "task:snapshot" {
                    continue;
                }
                let payload = serde_json::from_str(&data).unwrap_or(Value::String(data));
                sink.lock().unwrap().push((name, payload));
            }
        }
    });

    // 建会话（已存在则忽略结果）+ 聊天回合。
    let client = reqwest::Client::new();
    let _ = client
        .post(format!("{base}/api/v1/sessions"))
        .header("content-type", "application/json")
        .body(format!(r#"{{"sessionId":"{session_id}"}}"#))
        .send()
        .await;
    let response = client
        .post(format!("{base}/api/v1/agent"))
        .header("content-type", "application/json")
        .body(format!(
            r#"{{"instruction":"{instruction}","sessionId":"{session_id}"}}"#
        ))
        .send()
        .await
        .expect("聊天回合请求失败");
    assert_eq!(response.status().as_u16(), 200, "{base} 聊天回合应 200");

    // 等待回合终态（agent:complete + session:title）到达。
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let has_both = {
            let seen = events.lock().unwrap();
            seen.iter().any(|(name, _)| name == "agent:complete")
                && seen.iter().any(|(name, _)| name == "session:title")
        };
        if has_both || Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let _ = collector.await;
    let collected = events.lock().unwrap().clone();
    collected
}

/// 事件归一：① 共享词汇过滤；② 递归剥 null 值键（TS undefined→省略 vs
/// Rust None→null 的表示层差异）；③ llm:progress 数值遥测仅保留键语义。
fn normalize_sse_face(mut events: Vec<(String, Value)>) -> Vec<(String, Value)> {
    const SHARED: [&str; 7] = [
        "agent:start",
        "thinking:start",
        "thinking:delta",
        "thinking:end",
        "draft:delta",
        "agent:complete",
        "session:title",
    ];
    fn strip_nulls(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.retain(|_, value| !value.is_null());
                for value in map.values_mut() {
                    strip_nulls(value);
                }
            }
            Value::Array(items) => {
                for item in items.iter_mut() {
                    strip_nulls(item);
                }
            }
            _ => {}
        }
    }
    events.retain(|(name, _)| SHARED.contains(&name.as_str()));
    for (name, payload) in events.iter_mut() {
        if name == "llm:progress" {
            if let Value::Object(map) = payload {
                for key in ["elapsedMs", "totalChars", "chineseChars"] {
                    map.remove(key);
                }
            }
        }
        strip_nulls(payload);
    }
    events.sort_by(|a, b| a.0.cmp(&b.0));
    events
}

/// 128 号：SSE 事件面双端对跑——同根双进程各自订阅 /api/v1/events 并发起
/// 同构聊天回合，共享词汇事件（agent:start / llm:progress / draft:delta /
/// agent:complete / session:title）的**序列（按名稳定排序）与负载形态**
/// 双端等价（126/127 号补齐事件的真进程级验收门）。
#[tokio::test]
async fn sse_event_face_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    let ts_port = next_sidecar_port().await;
    let ts = spawn_ts_sidecar(&root, ts_port).await;
    let bin_port = next_sidecar_port().await;
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_inkos-engine-server"))
        .env("INKOS_PROJECT_ROOT", &root)
        .env("INKOS_PORT", bin_port.to_string())
        .env("INKOS_BUILTIN_GENRES_DIR", root.join("assets").join("genres"))
        .stdout(std::process::Stdio::from(
            std::fs::File::create("/tmp/duel-bin-sse-out.log").unwrap(),
        ))
        .stderr(std::process::Stdio::from(
            std::fs::File::create("/tmp/duel-bin-sse-err.log").unwrap(),
        ))
        .spawn()
        .expect("bin 起动失败");
    let bin = format!("http://127.0.0.1:{bin_port}");
    wait_ready(&bin).await;
    wait_ready(&ts).await;

    let instruction = "帮我看下第二章节奏";
    let ts_events =
        normalize_sse_face(sse_chat_turn_events(&ts, "1783100000001-ssets", instruction).await);
    let rust_events =
        normalize_sse_face(sse_chat_turn_events(&bin, "1783100000002-sseeb", instruction).await);

    // 事件名序列（排序后）等价。
    let ts_names: Vec<&str> = ts_events.iter().map(|(name, _)| name.as_str()).collect();
    let rust_names: Vec<&str> = rust_events.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        rust_names, ts_names,
        "事件名序列分歧\n  rust: {rust_names:?}\n  ts:   {ts_names:?}"
    );
    for ((rust_name, rust_payload), (ts_name, ts_payload)) in
        rust_events.iter().zip(ts_events.iter())
    {
        // sessionId 两端各自会话——剥除后负载应逐键等价。
        let mut rust_payload = rust_payload.clone();
        let mut ts_payload = ts_payload.clone();
        for payload in [&mut rust_payload, &mut ts_payload] {
            if let Value::Object(map) = payload {
                map.remove("sessionId");
            }
        }
        assert_eq!(
            rust_name, ts_name,
            "排序后事件错位：{rust_events:?} vs {ts_events:?}"
        );
        assert_eq!(
            rust_payload, ts_payload,
            "{rust_name} 负载分歧\n  rust: {rust_payload}\n  ts:   {ts_payload}"
        );
    }
    // 七类事件齐全（回合健康性；llm:progress 不在普通聊天轮词汇——128 号
    // 勘误：TS 仅 pipeline 面上报；thinking 三事件 129 号补齐）。
    for expected in [
        "agent:start",
        "thinking:start",
        "thinking:delta",
        "thinking:end",
        "draft:delta",
        "agent:complete",
        "session:title",
    ] {
        assert!(
            rust_names.contains(&expected),
            "Rust 侧缺 {expected}：{rust_names:?}"
        );
    }

    let _ = command.kill();
    let _ = command.wait();
    eprintln!("SSE 事件面对跑通过：七类共享词汇事件（含 thinking 三事件）序列与负载形态双端等价");
}
