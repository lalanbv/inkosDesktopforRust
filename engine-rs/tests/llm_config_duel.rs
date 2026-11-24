//! 1669 号：LLM 未配置对跑——「模型调用**之前**检出的配置问题」双腿同码同文。
//!
//! 同一**缺 key** 项目根下分别起 TS sidecar 真实进程与**未配置引擎**（bin 的 env
//! 缺省形态：discard 端口 + 空 key——桌壳直启的产品路径）的进程内全量路由，对
//! write-next / audit / agent 三面同请求对跑：两腿都必须 400 `LLM_CONFIG_ERROR`
//! 信封，且 `error` 全等（message 是 UI 文案表逐字锚）。
//!
//! 对跑不落 duel 台账：门禁的台账握手只认 `strangler_duel` 二进制（start 数 =
//! 其测试总数），本文件只以 `INKOS_DUEL` 开关真跑（门禁对 engine-rs 全量注入
//! `INKOS_DUEL=1`，故每轮门禁都实跑）。夹具为自带最小形态——`strangler_duel.rs`
//! 的同名辅助是该二进制的私有项；合并进公共模块待该文件在途重构落库后再做。
//!
//! 运行：`INKOS_DUEL=1 cargo test --test llm_config_duel -- --nocapture`。

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig, LLM_UNCONFIGURED_BASE_URL};
use inkos_engine::server::audit_route::AuditRuntime;
use inkos_engine::server::books_routes::BooksRuntime;
use inkos_engine::server::sse::BroadcastHub;
use inkos_engine::server::write_next_route::WriteNextRuntime;
use inkos_engine::state::manager::StateManager;

/// 与 TS `LLMConfigurationError("MISSING_API_KEY")` 逐字同句。
const MISSING_KEY: &str =
    "Studio LLM API key not set. Open Studio services and save an API key for the selected service.";

const SIDECAR_ERR_LOG: &str = "/tmp/duel-llm-config-tsx-err.log";
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}

/// 缺 key 的项目根：书 b1（legacy 布局）+ 指向非回环端点、secrets 缺席的服务项
/// ——回环端点免 key，测不到「缺 key」。
fn write_unconfigured_fixture(root: &Path) {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("engine-rs 必在仓库根下")
        .to_path_buf();
    std::env::set_var(
        "INKOS_BUILTIN_SKILLS_DIR",
        repo_root.join("packages").join("core").join("skills"),
    );
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
        r#"{"id":"b1","title":"未配置对跑书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z"}"#,
    )
    .unwrap();
    std::fs::write(
        book.join("chapters").join("0001_风起.md"),
        "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。",
    )
    .unwrap();
    std::fs::write(
        book.join("chapters").join("index.json"),
        r#"[{"number":1,"title":"风起","status":"ready-for-review","wordCount":18,"createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z","auditIssues":[]}]"#,
    )
    .unwrap();
    std::fs::write(
        root.join("inkos.json"),
        r#"{"version":"0.1.0","name":"未配置对跑项目","language":"zh","llm":{"services":{"custom:None":{"service":"custom","name":"None","baseUrl":"https://example.invalid/v1","apiFormat":"chat"}},"defaultModel":"none-model"},"notify":[]}"#,
    )
    .unwrap();
}

/// 未配置引擎的进程内全量路由（bin 同款装配；启动路由器 = env 缺省占位）。
async fn spawn_unconfigured_rust_engine(root: &Path) -> String {
    let hub = std::sync::Arc::new(BroadcastHub::new());
    let state = std::sync::Arc::new(StateManager::new(root.to_path_buf()));
    let router = std::sync::Arc::new(AgentRouter::new(
        LlmEndpointConfig {
            context_window_tokens: 128_000,
            base_url: LLM_UNCONFIGURED_BASE_URL.into(),
            api_key: String::new(),
            model: "default".into(),
            max_tokens: 8192,
            extra_headers: std::collections::HashMap::new(),
        },
        std::collections::HashMap::new(),
    ));
    let runner: inkos_engine::server::WriteNextRunner = std::sync::Arc::new(
        |_state, _book_id, _word_count, _temperature, _abort, _context| {
            Box::pin(async {
                Err::<inkos_engine::pipeline::write_next::ChapterPipelineResult, String>(
                    "duel: the runner must not run while unconfigured".to_string(),
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
    let audit = AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: router.clone(),
        builtin_genres_dir: root.join("assets").join("genres"),
    };
    let books = BooksRuntime {
        hub: hub.clone(),
        state,
        router,
        builtin_genres_dir: root.join("assets").join("genres"),
        revision_gate: Default::default(),
    };
    // `/api/v1/agent` 由 router_books 自身挂载——无需另加。
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
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// TS sidecar 句柄：Drop 即进程组整树收割（同 strangler_duel 的 1453 号形态）。
struct TsSidecar {
    base: String,
    child: std::process::Child,
}

/// 收割 sidecar 进程组（就绪失败路径与 Drop 共用——不留孤儿）。
fn reap(child: &mut std::process::Child) {
    // 安全性：pgid 来自 spawn 时 process_group(0) 设定的自有进程组，无误伤面。
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
    let _ = child.kill();
    let _ = child.wait();
}

impl Drop for TsSidecar {
    fn drop(&mut self) {
        reap(&mut self.child);
    }
}

impl std::ops::Deref for TsSidecar {
    type Target = str;
    fn deref(&self) -> &str {
        &self.base
    }
}

impl std::fmt::Display for TsSidecar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.base)
    }
}

fn log_tail(path: &str) -> String {
    std::fs::read_to_string(path)
        .map(|text| {
            let len = text.chars().count();
            text.chars().skip(len.saturating_sub(600)).collect()
        })
        .unwrap_or_else(|error| format!("<{path} 读取失败：{error}>"))
}

/// 回环直连 client（1583 号：默认构造读系统代理，回环目标会被劫持——恒直连）。
fn loopback_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest Client 构造失败")
}

/// 就绪判据 = `/api/v1/books` 成功；子进程早退立即判失败（附 stderr 尾）。
async fn probe_ready(
    child: &mut std::process::Child,
    base: &str,
    err_log: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().expect("子进程 try_wait") {
            return Err(format!(
                "{base} 子进程早退 {status:?}；{err_log} 尾：{}",
                log_tail(err_log)
            ));
        }
        let responded = match loopback_client()
            .get(format!("{base}/api/v1/books"))
            .timeout(PROBE_TIMEOUT)
            .send()
            .await
        {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        };
        if responded && child.try_wait().expect("子进程 try_wait").is_none() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{base} 60s 未就绪；{err_log} 尾：{}",
                log_tail(err_log)
            ));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// OS 分配空闲口 → 起 sidecar → 等就绪；失败先收割再 panic。
async fn spawn_ts_sidecar(root: &Path) -> TsSidecar {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let studio = repo_root().join("packages").join("studio");
    std::fs::create_dir_all(studio.join("dist")).unwrap();
    let index_path = studio.join("dist").join("index.html");
    if !index_path.exists() {
        std::fs::write(&index_path, "<!doctype html><title>duel</title>").unwrap();
    }
    let mut child = std::process::Command::new("node")
        .process_group(0)
        .arg(
            studio
                .join("node_modules")
                .join("tsx")
                .join("dist")
                .join("cli.mjs"),
        )
        .arg(studio.join("src").join("api").join("index.ts"))
        .arg(root)
        .env("INKOS_STUDIO_PORT", port.to_string())
        .env("INKOS_PROJECT_ROOT", root)
        .current_dir(&studio)
        .stdout(std::process::Stdio::from(
            std::fs::File::create("/tmp/duel-llm-config-tsx-out.log").unwrap(),
        ))
        .stderr(std::process::Stdio::from(
            std::fs::File::create(SIDECAR_ERR_LOG).unwrap(),
        ))
        .spawn()
        .expect("node+tsx 起动失败（先确认 packages/studio 依赖已安装）");
    let base = format!("http://127.0.0.1:{port}");
    if let Err(exit) = probe_ready(&mut child, &base, SIDECAR_ERR_LOG).await {
        reap(&mut child);
        panic!("TS sidecar 未就绪：{exit}");
    }
    TsSidecar { base, child }
}

/// 真实引擎二进制句柄：Drop 即 kill（panic 路径不留进程）。
struct EngineProcess {
    base: String,
    child: std::process::Child,
}

impl Drop for EngineProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl std::ops::Deref for EngineProcess {
    type Target = str;
    fn deref(&self) -> &str {
        &self.base
    }
}

/// 起真实 `inkos-engine-server`（二进制定位走 assert_cmd `cargo_bin`，仓内既定
/// 形态，见 `parent_watchdog_e2e`）。`env_llm = None` = 桌壳直启形态（不注入任何
/// `INKOS_LLM_*`——连同从测试进程继承来的一并摘掉，保证是 bin 的缺省占位）；
/// `Some(baseUrl)` = 开发 / 冒烟脚本的 env 配置形态。
async fn spawn_engine_bin(root: &Path, label: &str, env_llm: Option<&str>) -> EngineProcess {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let err_log = format!("/tmp/duel-llm-config-bin-{label}-err.log");
    let out_log = format!("/tmp/duel-llm-config-bin-{label}-out.log");
    let repo_root = repo_root();
    // 产物路径由 cargo 的 CARGO_BIN_EXE 约定定位（assert_cmd::cargo::cargo_bin）。
    let engine_bin = assert_cmd::cargo::cargo_bin("inkos-engine-server");
    let mut command = std::process::Command::new(engine_bin);
    command
        .env("INKOS_PROJECT_ROOT", root)
        .env("INKOS_PORT", port.to_string())
        .env(
            "INKOS_BUILTIN_GENRES_DIR",
            root.join("assets").join("genres"),
        )
        .env(
            "INKOS_BUILTIN_SKILLS_DIR",
            repo_root.join("packages").join("core").join("skills"),
        )
        .env_remove("INKOS_LLM_BASE_URL")
        .env_remove("INKOS_LLM_API_KEY")
        .env_remove("INKOS_LLM_MODEL");
    if let Some(base_url) = env_llm {
        // 测试键：env 注入 + 非凭据回退（源码不落可用凭据字面量）。
        let key = std::env::var("INKOS_TEST_LLM_KEY")
            .unwrap_or_else(|_| "test-key-placeholder".to_string());
        command
            .env("INKOS_LLM_BASE_URL", base_url)
            .env("INKOS_LLM_API_KEY", key)
            .env("INKOS_LLM_MODEL", "env-model");
    }
    let mut child = command
        .stdout(std::process::Stdio::from(
            std::fs::File::create(&out_log).unwrap(),
        ))
        .stderr(std::process::Stdio::from(
            std::fs::File::create(&err_log).unwrap(),
        ))
        .spawn()
        .expect("inkos-engine-server 起动失败");
    let base = format!("http://127.0.0.1:{port}");
    if let Err(exit) = probe_ready(&mut child, &base, &err_log).await {
        let _ = child.kill();
        let _ = child.wait();
        panic!("引擎二进制（{label}）未就绪：{exit}");
    }
    EngineProcess { base, child }
}

async fn get_json(base: &str, path: &str) -> (u16, Value) {
    let response = loopback_client()
        .get(format!("{base}{path}"))
        .send()
        .await
        .expect("GET 失败");
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

async fn post_json(base: &str, path: &str, body: &str) -> (u16, Value) {
    let response = loopback_client()
        .post(format!("{base}{path}"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .expect("POST 失败");
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

#[tokio::test]
async fn llm_config_error_duel() {
    if std::env::var("INKOS_DUEL").is_err() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write_unconfigured_fixture(&root);

    let rust = spawn_unconfigured_rust_engine(&root).await;
    let ts = spawn_ts_sidecar(&root).await;

    // agent 面的检查在会话查找之后（顺序同改动前）——先建一个真实会话。两腿
    // 共读同一项目根，一次建立双端可见。
    let session_id = "1783099000021-d169";
    let (status, body) = post_json(
        &rust,
        "/api/v1/sessions",
        &json!({ "sessionId": session_id, "sessionKind": "chat" }).to_string(),
    )
    .await;
    assert_eq!(status, 200, "建会话失败：{body}");

    // 两腿各自必须符合契约（防「对齐数据=回声室」），再断言 error 全等。载荷取
    // 能通过 TS 腿各自入参校验的形态（TS 的配置检查在处理器内部、校验之后）。
    let agent_body = json!({ "instruction": "你好", "sessionId": session_id }).to_string();
    let reference_text = "这是一段足够长的参考文本，用来做文风分析。".repeat(6);
    let faces: Vec<(&str, &str, String)> = vec![
        (
            "write-next",
            "/api/v1/books/b1/write-next",
            "{}".to_string(),
        ),
        ("audit", "/api/v1/books/b1/audit/1", "{}".to_string()),
        ("agent", "/api/v1/agent", agent_body),
        // 1670 号第二批守卫里两腿都有的面。
        (
            "style analyze",
            "/api/v1/style/analyze",
            json!({ "text": reference_text }).to_string(),
        ),
        (
            "imitation init",
            "/api/v1/imitation/init",
            json!({
                "title": "仿写",
                "referenceText": reference_text,
                "storyIdea": "一个点子",
            })
            .to_string(),
        ),
        ("radar scan", "/api/v1/radar/scan", "{}".to_string()),
        (
            "radar analyze",
            "/api/v1/radar/analyze",
            json!({ "selection": { "platforms": [], "categories": [] } }).to_string(),
        ),
        (
            "director directions",
            "/api/v1/director/directions",
            json!({ "inspiration": { "premise": "镜中图书馆" }, "count": 3 }).to_string(),
        ),
    ];
    for (name, path, body) in faces {
        let (ts_status, ts_body) = post_json(&ts, path, &body).await;
        let (rust_status, rust_body) = post_json(&rust, path, &body).await;
        assert_eq!(ts_status, 400, "[{name}] ts: {ts_body}");
        assert_eq!(rust_status, 400, "[{name}] rust: {rust_body}");
        assert_eq!(
            ts_body["error"],
            json!({ "code": "LLM_CONFIG_ERROR", "message": MISSING_KEY }),
            "[{name}] ts"
        );
        assert_eq!(
            rust_body["error"], ts_body["error"],
            "[{name}] 双腿 error 不等"
        );
    }

    eprintln!("LLM 未配置对跑通过：write-next / audit / agent / 文风分析 / 仿写创建 / 雷达扫描与分析 / 导演方向 八面双腿 400 LLM_CONFIG_ERROR 同码同文");
}

/// 真实引擎二进制的产品路径直证：桌壳直启不注入 `INKOS_LLM_*` → bin 的缺省
/// 端点即未配置占位；未配置项目上 write-next / audit / agent 同步 400，只读面
/// 照常。对照：同一项目根换成 env 配置形态启动，守卫放行（write-next 照常起
/// 后台任务）。进程内测试用的是手工构造的占位路由器——这里补的是 bin 装配层。
#[tokio::test]
async fn bin_process_unconfigured_engine_answers_400() {
    if std::env::var("INKOS_DUEL").is_err() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write_unconfigured_fixture(&root);

    // 产品路径：不注入任何 INKOS_LLM_*。
    let engine = spawn_engine_bin(&root, "unconfigured", None).await;
    for (name, path) in [
        ("write-next", "/api/v1/books/b1/write-next"),
        ("audit", "/api/v1/books/b1/audit/1"),
    ] {
        let (status, body) = post_json(&engine, path, "{}").await;
        assert_eq!(status, 400, "[{name}] {body}");
        assert_eq!(body["error"]["code"], "LLM_CONFIG_ERROR", "[{name}] {body}");
        assert_eq!(body["error"]["message"], MISSING_KEY, "[{name}] {body}");
    }
    let session_id = "1783099000031-d169";
    let (status, body) = post_json(
        &engine,
        "/api/v1/sessions",
        &json!({ "sessionId": session_id, "sessionKind": "chat" }).to_string(),
    )
    .await;
    assert_eq!(status, 200, "建会话：{body}");
    let (status, body) = post_json(
        &engine,
        "/api/v1/agent",
        &json!({ "instruction": "你好", "sessionId": session_id }).to_string(),
    )
    .await;
    assert_eq!(status, 400, "agent: {body}");
    assert_eq!(body["error"]["message"], MISSING_KEY, "agent: {body}");
    // 只读面不受影响。
    let (status, body) = get_json(&engine, "/api/v1/books").await;
    assert_eq!(status, 200, "books: {body}");
    drop(engine);

    // 对照：env 配置形态启动——守卫放行。
    let env_engine = spawn_engine_bin(&root, "env", Some("http://127.0.0.1:9/v1")).await;
    let (status, body) = post_json(&env_engine, "/api/v1/books/b1/write-next", "{}").await;
    assert_eq!(status, 200, "env 配置的引擎不应被拦：{body}");
    assert_eq!(body["status"], "writing", "{body}");

    eprintln!("真实二进制验收通过：未配置引擎三面 400 LLM_CONFIG_ERROR；env 配置引擎放行");
}
