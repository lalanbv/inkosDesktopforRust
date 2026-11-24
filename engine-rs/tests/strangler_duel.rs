//! strangler 只读面双端对跑（117 号）——98 号 runbook 第一步的自动化实跑。
//!
//! 同一 fixture 项目根下分别起动 **TS sidecar 真实进程**（tsx + studio
//! server.ts）与 **Rust engine 进程内全量路由**（`router_books`），对只读
//! 端点族逐个请求并做**结构化 JSON 对比**（键序无关；波动字段白名单归一）。
//! LLM 端点两侧同指进程内 mock（对跑不依赖真实 key）。
//!
//! 运行：`INKOS_DUEL=1 cargo test --test strangler_duel -- --nocapture`
//! （缺 env 直接跳过——不进基线六项）。

use serde_json::{json, Value};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 1666 号：duel 台账。`cargo test` 默认捕获测试的 stdout **与 stderr**
/// （libtest）——688 守卫「响亮记账」的 eprintln 在门禁日志里从未出现过：守卫
/// 早退不只是被计成 ok，连记账本身都是静默的。门禁经 `INKOS_DUEL_LEDGER`
/// 指定台账文件：`duel_enabled()`（每个对跑的入口）写一行 `start <test>`，
/// 守卫早退再写一行 `exempt <test>`（O_APPEND 单行小写，并行安全）。门禁据此
/// 正向确认：start 数 = 该二进制测试总数（通道存活的握手）、真跑 = start −
/// exempt，并对真跑数设地板、豁免数设上限。未设 env（手工跑）不落台账，只剩
/// eprintln（配 `-- --nocapture` 可见）。
fn record_duel_ledger(kind: &str) {
    let Some(path) = std::env::var_os("INKOS_DUEL_LEDGER") else {
        return;
    };
    let thread = std::thread::current();
    let test = thread.name().unwrap_or("?");
    let line = format!("{kind} {test}\n");
    let appended = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
    if let Err(error) = appended {
        eprintln!("[duel:ledger] 台账写入失败（{path:?}）：{error}——{kind} {test}");
    }
}

fn duel_enabled() -> bool {
    record_duel_ledger("start");
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
    // 130 号：builtin 技能包根（TS file-relative 恒指向 packages/core/skills；
    // Rust in-process 面经 env 注入同一目录，bin 进程在 spawn 点显式传）。
    // 各 duel 测试并发设置同值——无害。
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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
/// 548 号（R30）：响应携带 OpenAI 规范必需的 finish_reason——pi-ai 0.87 严检
/// "Stream ended without finish_reason"（0.73 宽容缺失，0.87 起报错）。
async fn spawn_mock_llm() -> String {
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
            // 双形态：流式请求 → SSE（129 号起带 reasoning_content 增量——
            // thinking 面对跑驱动）；非流式 → 整体 JSON（两侧客户端偏好不同）。
            if body["stream"].as_bool().unwrap_or(false) {
                let reasoning = serde_json::json!({ "choices": [{ "delta": { "reasoning_content": "让我想想" }, "finish_reason": null }] });
                let chunk = serde_json::json!({ "choices": [{ "delta": { "content": "OK" }, "finish_reason": null }] });
                let finish = serde_json::json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] });
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                return axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {reasoning}

data: {chunk}

data: {finish}

data: {usage}

data: [DONE]

"),
                ));
            }
            axum::response::IntoResponse::into_response(axum::Json(serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": "OK" }, "finish_reason": "stop" }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
            })))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// 系列回填抽取用 mock LLM：内容为围栏 JSON 的 items（测宽松提取）。
async fn spawn_backfill_mock_llm() -> String {
    let content = r#"抽取结果：```json
{"items": [{"id": "it-1", "category": "worldview", "title": "九品灵气", "content": "灵气分九品。"}, {"id": "it-2", "category": "character", "title": "林动", "content": "隐忍坚韧的主角。"}]}
```"#;
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(move |axum::Json(body): axum::Json<Value>| async move {
            if body["stream"].as_bool().unwrap_or(false) {
                let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                return axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {chunk}\n\ndata: [DONE]\n\n"),
                ));
            }
            axum::response::IntoResponse::into_response(axum::Json(serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": content } }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
            })))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// Rust engine 进程内全量路由（bin 同款装配）。
async fn spawn_rust_engine(root: &Path, llm: &str) -> String {
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::write_next_route::WriteNextRuntime;
    let hub = std::sync::Arc::new(inkos_engine::server::sse::BroadcastHub::new());
    let state = std::sync::Arc::new(inkos_engine::state::manager::StateManager::new(
        root.to_path_buf(),
    ));
    let router = std::sync::Arc::new(AgentRouter::new(
        LlmEndpointConfig {
            context_window_tokens: 128_000,
            base_url: llm.into(),
            api_key: "duel-key".into(),
            model: "duel-model".into(),
            max_tokens: 8192,
            extra_headers: std::collections::HashMap::new(),
        },
        std::collections::HashMap::new(),
    ));
    let runner: inkos_engine::server::write_next_route::WriteNextRunner = std::sync::Arc::new(
        |_state: std::sync::Arc<inkos_engine::state::manager::StateManager>,
         _book_id: String,
         _word_count: Option<u32>,
         _temperature: Option<f64>,
         _abort: inkos_engine::interaction::agent_loop::AbortHandle,
         _context: Option<String>| {
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
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// TS sidecar 真实进程（tsx 直跑 server 入口；dist/index.html 预置跳过前端构建）。
/// TS sidecar 句柄：Drop 即进程组整树收割（1453 号）。
///
/// 旧形态=600s sleep 兜底线程——但每个 `#[tokio::test]` 是独立进程，测试
/// 进程退出线程随之消亡，兜底 kill 永不执行；且 `.bin/tsx` 是 shell 包装
/// （tsx→node cli→exec 重执行三层），顶层 kill 杀不掉组内后续进程——两因
/// 叠加使每轮 testgate（INKOS_DUEL=1 强制真跑）泄漏双层孤儿（实测形态：
/// tsx 包装 PPID=1 + node server 挂高位随机口，PGID 留守已死组长）。
///
/// 修复=spawn 前 `process_group(0)` 令 sidecar 自成进程组（exec 重执行不
/// 换组，实测 PGID 留守语义）+ RAII Drop `killpg(SIGKILL)` 整树收割（含
/// panic 路径；SIGKILL 级杀掉测试进程的场景 Drop 同死=903 族已知边界，
/// 与旧兜底语义等价不损失）。
struct TsSidecar {
    base: String,
    child: std::process::Child,
}

impl Drop for TsSidecar {
    fn drop(&mut self) {
        // 安全性：pgid 来自本函数 spawn 时 process_group(0) 设定的自有进程
        // 组（组长=child pid，活着），非外部进程组，无误伤面。
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        let _ = self.child.kill();
        let _ = self.child.wait();
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

async fn spawn_ts_sidecar(root: &Path, port: u16) -> TsSidecar {
    let repo = repo_root();
    let studio = repo.join("packages").join("studio");
    // 跳过 vite 前端自动构建：index.ts 以 dist/index.html 存在性判断。
    // 252 号：仅在缺失时写占位——覆盖生产构建的 index.html 会让真实引擎
    // 静态面 serve 出 duel 占位页（浏览器直连白屏）。
    std::fs::create_dir_all(studio.join("dist")).unwrap();
    let index_path = studio.join("dist").join("index.html");
    if !index_path.exists() {
        std::fs::write(&index_path, "<!doctype html><title>duel</title>").unwrap();
    }
    let command = std::process::Command::new("node")
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
            std::fs::File::create("/tmp/duel-tsx-out.log").unwrap(),
        ))
        .stderr(std::process::Stdio::from(
            std::fs::File::create("/tmp/duel-tsx-err.log").unwrap(),
        ))
        .spawn()
        .expect("node+tsx 起动失败（先确认 packages/studio 依赖已安装）");
    TsSidecar {
        base: format!("http://127.0.0.1:{port}"),
        child: command,
    }
}

/// 1666 号：日志尾（早退诊断用——stderr 全文可能很长）。
fn log_tail(path: &str) -> String {
    std::fs::read_to_string(path)
        .map(|text| {
            let len = text.chars().count();
            text.chars().skip(len.saturating_sub(600)).collect()
        })
        .unwrap_or_else(|error| format!("<{path} 读取失败：{error}>"))
}

/// 1666 号：单次就绪探测的限时。探测请求本身必须限时——占口者若「收连接
/// 不应答」，不带限时的 `.send().await` 会把整个等待卡死，60s 截止永远检查
/// 不到（注入故障实验实录：被占端口挂着不回话，用例 600s 不返回）。
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// 1666 号：带子进程句柄的就绪探测——与 [`wait_ready`] 同判据
/// （`/api/v1/books` 成功），另加「进程早退」分支：进程已死却还在等就绪 =
/// 必红，立即返回并附 stderr 日志尾，不再空等 60s。
///
/// testgate 实录：TS sidecar 端口交接竞态（`next_sidecar_port` 的 bind(:0)+
/// drop 与 tsx 真正 listen 之间端口被占，EADDRINUSE 127.0.0.1:64750）→
/// 「60s 未就绪」假红，真因要到 /tmp/duel-tsx-err.log 里翻。超时不是竞态
/// 形态，沿用 `wait_ready` 的断言语义（panic，不交调用方重试）。
async fn probe_child_ready(
    child: &mut std::process::Child,
    base: &str,
    err_log: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        // 先看进程：已死就不必再探（占口者只收连接不应答时，探测要空等到
        // 限时才返回）。
        if let Some(status) = child.try_wait().expect("子进程 try_wait") {
            return Err(format!(
                "{base} 子进程早退 {status:?}；{err_log} 尾：{}",
                log_tail(err_log)
            ));
        }
        // 1583 号：回环直连（见 loopback_client）。
        let responded = match loopback_client()
            .get(format!("{base}/api/v1/books"))
            .timeout(PROBE_TIMEOUT)
            .send()
            .await
        {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        };
        // 应答成功但进程随即退出（罕见）不算就绪——下一轮落到早退分支。
        if responded && child.try_wait().expect("子进程 try_wait").is_none() {
            return Ok(());
        }
        assert!(Instant::now() < deadline, "{base} 60s 未就绪");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// 1666 号：引擎 bin 子进程的就绪等待——早退即 panic（附 stderr 尾）。
/// 不重试：Command 构造在各调用点（竞态同形但本批未实录；换口重试见
/// [`spawn_ts_sidecar_ready`]）。
async fn wait_child_ready(child: &mut std::process::Child, base: &str, err_log: &str) {
    if let Err(exit) = probe_child_ready(child, base, err_log).await {
        panic!("{exit}");
    }
}

/// 1666 号：sidecar 起动换口重试上限。
const SIDECAR_SPAWN_ATTEMPTS: u32 = 5;

/// 1666 号：TS sidecar 起动握手——取口 → 起进程 → 就绪；早退则换口重试。
///
/// 端口交接窗口内的占用是非确定性竞态（同二进制复跑 6 轮全绿；并发压力
/// 探针 26 万次交接零冲突——不可复现、无法事后还原占口者），换口重来即过。
/// 启动即崩的产品缺陷（如 schema 拒启）每次同样早退——重试耗尽后 panic
/// 并附最后一次的 stderr 尾，不被重试掩盖。
async fn spawn_ts_sidecar_ready(root: &Path) -> TsSidecar {
    let mut last_exit = String::new();
    for attempt in 1..=SIDECAR_SPAWN_ATTEMPTS {
        let port = next_sidecar_port().await;
        let mut sidecar = spawn_ts_sidecar(root, port).await;
        match probe_child_ready(&mut sidecar.child, &sidecar.base, "/tmp/duel-tsx-err.log").await {
            Ok(()) => return sidecar,
            Err(exit) => {
                eprintln!(
                    "[duel] sidecar 第 {attempt}/{SIDECAR_SPAWN_ATTEMPTS} 次起动早退，换口重试：{exit}"
                );
                last_exit = exit;
            }
        }
    }
    panic!("TS sidecar {SIDECAR_SPAWN_ATTEMPTS} 次起动均早退：{last_exit}");
}

/// 回环直连 client（1583 号）：默认构造（`Client::new()`）无条件读 env/系统
/// 代理（auto_sys_proxy → macOS SCDynamicStore，不读 ExceptionsList），回环
/// 目标被劫持去本机代理——代理转发回环成功时绿（劫持不可见），代理对回环
/// RST 时假红（ConnectionReset 的 "peer" 是代理不是被测服务）。本文件全部
/// mock/引擎都在 127.0.0.1：恒直连（同 src-tauri health_probe 1572 号谓词语
/// 义，1594 口诀「探活本机 no_proxy」）。
fn loopback_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest Client 构建失败")
}

async fn get_json(base: &str, path: &str) -> (u16, Value) {
    let client = loopback_client();
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

/// 1666 号：写面/负路径面同请求——`get_json` 的带 method + JSON 体形态。
async fn send_json(
    client: &reqwest::Client,
    base: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<&str>,
) -> (u16, Value) {
    let mut request = client.request(method, format!("{base}{path}"));
    if let Some(body) = body {
        request = request
            .header("content-type", "application/json")
            .body(body.to_string());
    }
    let response = request.send().await.expect("请求失败");
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (status, parsed)
}

/// 1666 号：双端同请求对跑——同一 method/path/body 分别打两腿，返回
/// (rust_status, rust_body, ts_status, ts_body)（body 已 normalize）。调用方
/// 既要断言**两腿各自**符合预期（防「对齐数据=回声室」），也要断言两腿全等。
async fn duel_pair(
    client: &reqwest::Client,
    rust: &str,
    ts: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<&str>,
) -> (u16, Value, u16, Value) {
    let (rust_status, mut rust_body) = send_json(client, rust, method.clone(), path, body).await;
    let (ts_status, mut ts_body) = send_json(client, ts, method, path, body).await;
    normalize(&mut rust_body);
    normalize(&mut ts_body);
    (rust_status, rust_body, ts_status, ts_body)
}

/// 波动键归一（递归剥除）：时间戳/版本/路径前缀类。
fn normalize(value: &mut Value) {
    const VOLATILE: [&str; 6] = [
        "startedAt",
        "completedAt",
        "timestamp",
        "updatedAt",
        "createdAt",
        "version",
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
        // 1583 号补：顶层便捷函数 `reqwest::get()` 内部构造默认 Client（读
        // env/系统代理）——与本文件 `Client::new()` 站点同族漏网（死代理 env
        // 下探活 60s 全败实证），统一走 loopback_client 恒直连。
        // 1666 号：单次探测限时（PROBE_TIMEOUT）——否则「收连接不应答」的
        // 占口者让下面的 60s 截止永远检查不到。
        if let Ok(response) = loopback_client()
            .get(format!("{base}/api/v1/books"))
            .timeout(PROBE_TIMEOUT)
            .send()
            .await
        {
            if response.status().is_success() {
                return;
            }
        }
        assert!(Instant::now() < deadline, "{base} 60s 未就绪");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// 688 号（1663 修订）：TS 腿契约源探测——判据原 `/api/v1/prompt-packs`
/// （1662 起 TS 回退腿同构该族，判据失效——意外复活守卫打开 12 个早已
/// 豁免的对跑=8 红黑盒真相）。迁 `/api/v1/health`（62/170 号 Rust 超集
/// 端点，TS 无=真 fork 独有面，与 scripts 三脚本 1662 判据迁移一致）。
async fn ts_leg_contract_source(ts: &str) -> &'static str {
    match get_json(ts, "/api/v1/health").await {
        (200, _) => "fork",
        _ => "upstream-2.0",
    }
}

/// 688 号契约源守卫：TS 腿非 fork 契约时，fork 契约对跑整体失效——
/// 响亮记账后跳过本测（boot/readiness 已在前序 wait_ready 验证，不静默）。
/// TS 恢复 fork 契约（2.0 迁移落地，见 688 号 §五）后守卫自动复活，无需改码。
///
/// 1666 号实测补注：判据 `/api/v1/health` 是 Rust 超集独有面——TS 腿**没有
/// 这条路由**，守卫对 TS 腿恒判「上游 2.0」= 受守卫的对跑恒跳过，而 libtest
/// 把早退计成 ok（「N passed」里混着从未执行的对跑）。强制打开守卫实测 13 个
/// 对跑：6 真绿 / 7 真红——4 个是 `write_fixture` 仍写 1.x `books/` 布局而
/// TS 2.0 按 works 清单发现作品；1 个是 mock LLM 不回 finish_turn；3 个是真实
/// 契约分歧（/project 的 temperature、series-backfill extract、会话读面形状）。
/// 复活哪些、以何判据复活属 688 号迁移欠账的处置决策；本轮只让跳过不再
/// 隐形：落 `exempt <test>` 台账行（见 `record_duel_ledger`；eprintln 被捕获，
/// 门禁日志里看不见），cargo-test-gate 据此算「真跑 = start − exempt」，对真跑
/// 数设地板、豁免数设上限。
macro_rules! duel_require_fork_contract {
    ($ts:expr) => {
        if ts_leg_contract_source($ts).await != "fork" {
            record_duel_ledger("exempt");
            eprintln!(
                "[duel:exempt] {}",
                std::thread::current().name().unwrap_or("?")
            );
            eprintln!(
                "[688 豁免记账] 契约源检测：TS 腿=上游 2.0（/api/v1/health 非 200）——fork 契约对跑失效，本测响亮跳过。\n                 [688 豁免记账] 差异面=2.0 迁移欠账（变更记录 688 号 §五），非 Rust 回归；迁移落地后本守卫自动复活。"
            );
            return;
        }
    };
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
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;
    duel_require_fork_contract!(&ts);

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
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;
    duel_require_fork_contract!(&ts);

    let client = loopback_client();

    // A) Rust 写：approve 第 1 章。
    let response = client
        .post(format!("{rust}/api/v1/books/b1/chapters/1/approve"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    // 双端读：book 详情的章节摘要 status=approved（index.json 跨端可见）。
    for base in [&rust, &ts.base] {
        let (status, body) = get_json(base, "/api/v1/books/b1").await;
        assert_eq!(status, 200, "base={base} body={body}");
        let approved = body
            .get("chapters")
            .and_then(|chapters| chapters.as_array())
            .is_some_and(|chapters| {
                chapters.iter().any(|chapter| {
                    chapter.get("number").and_then(Value::as_u64) == Some(1)
                        && chapter.get("status").and_then(Value::as_str) == Some("approved")
                })
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
    for base in [&rust, &ts.base] {
        let (status, body) = get_json(base, "/api/v1/books/b1/chapter-review-mode").await;
        if status != 200 {
            let written = std::fs::read_to_string(root.join("books").join("b1").join("book.json"))
                .unwrap_or_default();
            eprintln!("review-mode 读失败 base={base} book.json={written}");
        }
        assert_eq!(status, 200, "base={base} body={body}");
        assert!(
            serde_json::to_string(&body)
                .unwrap_or_default()
                .contains("manual"),
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
    eprintln!(
        "跨端写读对跑通过：Rust 写 approve / TS 写 review-mode 双向可见，写后 {} 端点复跑全一致",
        endpoints.len()
    );
}

/// 1666 号：prompt-packs 端点族双端对跑（1663 回座 TS 腿后的族级契约锁）。
/// 此前该族只有各腿单测——versions 端点的 :promptId 白名单漂移（Rust 404 /
/// TS 200 空列表）正是「各测各的、无人对跑」漏过去的。
/// A) 负路径矩阵：两腿逐条同请求，**各自**符合预期状态码/错误码且整个信封
///    全等（防「对齐数据=回声室」），全程零副作用；
/// B) 跨端写读：每步写操作两腿各发一次（共用同一 project root），应答全等，
///    随后双端读面（versions 列表 / prompt 列表）全等——put → 回滚 → 删除 →
///    回滚到 put 行 → 回滚到 delete 行。
#[tokio::test]
async fn strangler_prompt_packs_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;
    // 契约源探测用**族根端点自身**，不走 duel_require_fork_contract!：该宏
    // 判据 `/api/v1/health` 是 Rust 超集独有面（TS 腿无此路由）→ 对 TS 腿恒判
    // 「上游 2.0」→ 受守卫的对跑恒跳过却计 ok。本族 TS 回退腿 1663 号起同构，
    // 以 `GET /prompt-packs` 200 为「族在位」判据。
    match get_json(&ts, "/api/v1/prompt-packs").await {
        (200, _) => {}
        (status, body) => {
            eprintln!("[1666 记账] TS 腿无 prompt-packs 族（{status}）——本测响亮跳过。{body}");
            return;
        }
    }
    let client = loopback_client();
    use reqwest::Method;

    let versions_path = "/api/v1/prompt-packs/longform.writer/versions";
    let restore_path = "/api/v1/prompt-packs/longform.writer/versions/restore";
    let writer_path = "/api/v1/prompt-packs/longform.writer";
    let override_file = root.join("prompt").join("longform").join("writer.md");

    // ── A) 负路径矩阵：(method, path, body, 预期状态码, 预期错误码) ──────
    let negative: [(Method, &str, Option<&str>, u16, &str); 11] = [
        (
            Method::GET,
            "/api/v1/prompt-packs/unknown.prompt/versions",
            None,
            404,
            "PROMPT_PACK_PROMPT_NOT_FOUND",
        ),
        (
            Method::POST,
            "/api/v1/prompt-packs/Unknown.Prompt/versions/restore",
            Some("{}"),
            404,
            "PROMPT_PACK_PROMPT_NOT_FOUND",
        ),
        (
            Method::DELETE,
            "/api/v1/prompt-packs/unknown.prompt",
            None,
            404,
            "PROMPT_PACK_PROMPT_NOT_FOUND",
        ),
        (
            Method::PUT,
            "/api/v1/prompt-packs/unknown.prompt",
            Some(r#"{"content":"x"}"#),
            404,
            "PROMPT_PACK_PROMPT_NOT_FOUND",
        ),
        (
            Method::GET,
            "/api/v1/prompt-packs/%20/versions",
            None,
            404,
            "PROMPT_PACK_PROMPT_NOT_FOUND",
        ),
        (
            Method::PUT,
            writer_path,
            Some("not-json"),
            400,
            "INVALID_PROMPT_PACK_PAYLOAD",
        ),
        (
            Method::PUT,
            writer_path,
            Some(r#"{"content":123}"#),
            400,
            "INVALID_PROMPT_PACK_PAYLOAD",
        ),
        (
            Method::PUT,
            writer_path,
            Some("[1,2]"),
            400,
            "INVALID_PROMPT_PACK_PAYLOAD",
        ),
        (
            Method::POST,
            restore_path,
            Some(r#"{"ts":1}"#),
            400,
            "INVALID_PROMPT_VERSION_PAYLOAD",
        ),
        (
            Method::POST,
            restore_path,
            Some("null"),
            400,
            "INVALID_PROMPT_VERSION_PAYLOAD",
        ),
        (
            Method::POST,
            restore_path,
            Some(r#"{"ts":"1970-01-01T00:00:00.000Z"}"#),
            400,
            "PROMPT_VERSION_NOT_FOUND",
        ),
    ];
    let negative_count = negative.len();
    for (method, path, body, want_status, want_code) in negative {
        let method_label = method.as_str().to_string();
        let (rust_status, rust_body, ts_status, ts_body) =
            duel_pair(&client, &rust, &ts, method, path, body).await;
        for (leg, status, body_json) in [
            ("rust", rust_status, &rust_body),
            ("ts", ts_status, &ts_body),
        ] {
            assert_eq!(
                status, want_status,
                "[{leg}] {method_label} {path} body={body_json}"
            );
            assert_eq!(
                body_json["error"]["code"], want_code,
                "[{leg}] {method_label} {path} body={body_json}"
            );
        }
        assert_eq!(
            (rust_status, rust_body.clone()),
            (ts_status, ts_body.clone()),
            "双端 error 信封不等：{method_label} {path}"
        );
    }
    // 零副作用：负路径全程不落版本行、不落覆盖文件。
    let (rust_status, rust_body, ts_status, ts_body) =
        duel_pair(&client, &rust, &ts, Method::GET, versions_path, None).await;
    assert_eq!((rust_status, ts_status), (200, 200));
    assert_eq!(rust_body, ts_body);
    assert_eq!(rust_body["versions"].as_array().map(Vec::len), Some(0));
    assert!(!root.join("prompt").join(".versions.jsonl").exists());

    // ── B) 跨端写读：每步两腿各写一次，应答全等，随后双端读面全等 ────────
    let expect_pair = |label: &str, pair: (u16, Value, u16, Value), want_status: u16| {
        let (rust_status, rust_body, ts_status, ts_body) = pair;
        assert_eq!(rust_status, want_status, "[rust] {label}: {rust_body}");
        assert_eq!(ts_status, want_status, "[ts] {label}: {ts_body}");
        assert_eq!(rust_body, ts_body, "双端应答不等：{label}");
        rust_body
    };
    let row_ts = |rows: &Value, index: usize| -> String {
        rows["versions"][index]["ts"].as_str().unwrap().to_string()
    };

    // B1) put v1 ×2（两腿各一次）。
    let body = expect_pair(
        "put v1",
        duel_pair(
            &client,
            &rust,
            &ts,
            Method::PUT,
            writer_path,
            Some(r##"{"content":"# v1"}"##),
        )
        .await,
        200,
    );
    assert_eq!(body["prompt"]["content"], "# v1");
    assert_eq!(body["prompt"]["source"], "project");
    assert_eq!(body["prompt"]["path"], "prompt/longform/writer.md");
    assert_eq!(body["regressionHint"], "prompt-changed");
    let rows = expect_pair(
        "versions after v1",
        duel_pair(&client, &rust, &ts, Method::GET, versions_path, None).await,
        200,
    );
    assert_eq!(rows["versions"].as_array().map(Vec::len), Some(2));
    assert_eq!(rows["versions"][0]["action"], "put");
    assert_eq!(rows["versions"][0]["content"], "# v1");
    assert_eq!(rows["versions"][0]["preview"], "# v1");

    // B2) put v2 ×2 → 4 行，最新在前。
    expect_pair(
        "put v2",
        duel_pair(
            &client,
            &rust,
            &ts,
            Method::PUT,
            writer_path,
            Some(r##"{"content":"# v2"}"##),
        )
        .await,
        200,
    );
    let rows = expect_pair(
        "versions after v2",
        duel_pair(&client, &rust, &ts, Method::GET, versions_path, None).await,
        200,
    );
    assert_eq!(rows["versions"].as_array().map(Vec::len), Some(4));
    let actions: Vec<&str> = rows["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, ["put", "put", "put", "put"]);
    assert_eq!(rows["versions"][0]["content"], "# v2");
    assert_eq!(rows["versions"][3]["content"], "# v1");

    // B3) 回滚到最早那条 v1 行 ×2 → 覆盖文件 = v1，restore 行居首。
    let v1_ts = row_ts(&rows, 3);
    let body = expect_pair(
        "restore v1",
        duel_pair(
            &client,
            &rust,
            &ts,
            Method::POST,
            restore_path,
            Some(&format!(r#"{{"ts":"{v1_ts}"}}"#)),
        )
        .await,
        200,
    );
    assert_eq!(body["prompt"]["content"], "# v1");
    assert_eq!(std::fs::read_to_string(&override_file).unwrap(), "# v1");
    let rows = expect_pair(
        "versions after restore",
        duel_pair(&client, &rust, &ts, Method::GET, versions_path, None).await,
        200,
    );
    assert_eq!(rows["versions"].as_array().map(Vec::len), Some(6));
    assert_eq!(rows["versions"][0]["action"], "restore");
    assert_eq!(rows["versions"][0]["content"], "# v1");

    // B4) 删除 ×2 → 回 builtin。
    let body = expect_pair(
        "delete",
        duel_pair(&client, &rust, &ts, Method::DELETE, writer_path, None).await,
        200,
    );
    assert_eq!(body["prompt"]["source"], "builtin");
    assert_eq!(body["prompt"]["overridden"], false);
    assert!(!override_file.exists());
    let rows = expect_pair(
        "versions after delete",
        duel_pair(&client, &rust, &ts, Method::GET, versions_path, None).await,
        200,
    );
    assert_eq!(rows["versions"].as_array().map(Vec::len), Some(8));
    assert_eq!(rows["versions"][0]["action"], "delete");
    let delete_ts = row_ts(&rows, 0);

    // B5) 回滚到一条 v2 的 put 行 ×2 → 覆盖文件 = v2；prompt 列表双端全等。
    let v2_ts = row_ts(&rows, 4);
    let body = expect_pair(
        "restore v2",
        duel_pair(
            &client,
            &rust,
            &ts,
            Method::POST,
            restore_path,
            Some(&format!(r#"{{"ts":"{v2_ts}"}}"#)),
        )
        .await,
        200,
    );
    assert_eq!(body["prompt"]["content"], "# v2");
    let list = expect_pair(
        "prompt list",
        duel_pair(
            &client,
            &rust,
            &ts,
            Method::GET,
            "/api/v1/prompt-packs",
            None,
        )
        .await,
        200,
    );
    let writer = list["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|prompt| prompt["id"] == "longform.writer")
        .cloned()
        .expect("longform.writer 在列");
    assert_eq!(writer["content"], "# v2");
    assert_eq!(writer["overridden"], true);

    // B6) 回滚到 delete 行 ×2 → 移除覆盖（回 builtin）。
    let body = expect_pair(
        "restore delete",
        duel_pair(
            &client,
            &rust,
            &ts,
            Method::POST,
            restore_path,
            Some(&format!(r#"{{"ts":"{delete_ts}"}}"#)),
        )
        .await,
        200,
    );
    assert_eq!(body["prompt"]["source"], "builtin");
    assert!(!override_file.exists());
    let rows = expect_pair(
        "versions after restore delete",
        duel_pair(&client, &rust, &ts, Method::GET, versions_path, None).await,
        200,
    );
    assert_eq!(rows["versions"].as_array().map(Vec::len), Some(12));
    // 1666 号：回滚到 delete 版的新行记 delete（效果=移除覆盖）——此前记
    // `restore` + 空 content，与「回滚到空 put 版」同形（见 B7）。
    assert_eq!(rows["versions"][0]["action"], "delete");
    let rollback_ts = rows["versions"][0]["ts"].as_str().unwrap().to_string();

    // B7) 对「回滚到 delete 版」产生的新行再回滚 → 仍是 builtin（修复前双端
    //     都写入 0 字节覆盖文件：source=project、content=""）。双端共用项目根，
    //     每步两腿各落一行：12 + 2。
    let body = expect_pair(
        "restore rollback-to-builtin row",
        duel_pair(
            &client,
            &rust,
            &ts,
            Method::POST,
            restore_path,
            Some(&format!(r#"{{"ts":"{rollback_ts}"}}"#)),
        )
        .await,
        200,
    );
    assert_eq!(body["prompt"]["source"], "builtin");
    assert_eq!(body["prompt"]["overridden"], false);
    assert!(!override_file.exists());
    let rows = expect_pair(
        "versions final",
        duel_pair(&client, &rust, &ts, Method::GET, versions_path, None).await,
        200,
    );
    assert_eq!(rows["versions"].as_array().map(Vec::len), Some(14));
    eprintln!(
        "prompt-packs 族对跑通过：负路径 {} 条 + 写读 7 步，双端全等",
        negative_count
    );
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
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;
    duel_require_fork_contract!(&ts);
    let client = loopback_client();

    // A) Rust 建会话（book 绑定）→ 双端 GET 等价。
    let rust_session = "1783099000012-d119";
    let response = client
        .post(format!("{rust}/api/v1/sessions"))
        .header("content-type", "application/json")
        .body(format!(
            r#"{{"sessionId":"{rust_session}","bookId":"b1","sessionKind":"book"}}"#
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let (rust_status, mut rust_body) =
        get_json(&rust, &format!("/api/v1/sessions/{rust_session}")).await;
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
    for base in [&rust, &ts.base] {
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
    let (rust_status, mut rust_body) =
        get_json(&rust, &format!("/api/v1/sessions/{ts_session}")).await;
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
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;
    duel_require_fork_contract!(&ts);
    let client = loopback_client();

    // A) 服务列表读面（fixture 服务项 + secrets 键集）。
    let (rust_status, mut rust_body) = get_json(&rust, "/api/v1/services").await;
    let (ts_status, mut ts_body) = get_json(&ts, "/api/v1/services").await;
    assert_eq!(rust_status, 200, "rust: {rust_body}");
    assert_eq!(ts_status, 200, "ts: {ts_body}");
    normalize(&mut rust_body);
    normalize(&mut ts_body);
    if rust_body != ts_body {
        eprintln!("服务列表差异（备案观察面）：\n  rust: {rust_body}\n  ts:   {ts_body}");
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
    let (rust_status, mut rust_probe) =
        post_json(&rust, "/api/v1/services/deepseek/test", &body).await;
    let (ts_status, mut ts_probe) = post_json(&ts, "/api/v1/services/deepseek/test", &body).await;
    assert_eq!(rust_status, 200, "rust probe: {rust_probe}");
    assert_eq!(ts_status, 200, "ts probe: {ts_probe}");
    normalize(&mut rust_probe);
    normalize(&mut ts_probe);
    if rust_probe != ts_probe {
        panic!(
            "深链探测双端不等价：
  rust: {rust_probe}
  ts:   {ts_probe}"
        );
    }

    eprintln!("模型配置域对跑通过：服务列表面观察 / 密钥双向跨端写读 / 深链探测等价");
}

async fn post_json(base: &str, path: &str, body: &str) -> (u16, Value) {
    let client = loopback_client();
    let response = client
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
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;
    duel_require_fork_contract!(&ts);
    let client = loopback_client();

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
    async fn assert_readonly_equivalent(rust: &str, ts: &str, readonly: &[&str], step: usize) {
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
        .body(format!(
            r#"{{"instruction":"灰度回合","sessionId":"{grey_session}"}}"#
        ))
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
async fn spawn_counting_mock_llm(hits: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> String {
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
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
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
        .env(
            "INKOS_BUILTIN_GENRES_DIR",
            root.join("assets").join("genres"),
        )
        .env(
            "INKOS_BUILTIN_SKILLS_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("packages")
                .join("core")
                .join("skills"),
        )
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
    wait_child_ready(&mut command, &base, "/tmp/duel-bin-err.log").await;

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
    let client = loopback_client();
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
    let response = loopback_client()
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
    let ts = spawn_ts_sidecar_ready(&root).await;
    // 711 号：契约源守卫前置到 bin spawn 之前——守卫只探测 TS 腿，豁免早退
    // 路径不再起动引擎子进程（clippy zombie_processes：spawn 后 early return
    // 无 wait 的丢弃路径消除；跳过态还省一次引擎冷启）。
    duel_require_fork_contract!(&ts);
    let dist = repo_root().join("packages").join("studio").join("dist");
    std::fs::create_dir_all(dist.join("assets")).unwrap();
    std::fs::write(dist.join("assets").join("app.js"), "console.log('duel')").unwrap();

    // bin 在 dist/index.html 落位后起动（index 启动期缓存两侧同内容）。
    let bin_port = next_sidecar_port().await;
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_inkos-engine-server"))
        .env("INKOS_PROJECT_ROOT", &root)
        .env("INKOS_PORT", bin_port.to_string())
        .env(
            "INKOS_BUILTIN_GENRES_DIR",
            root.join("assets").join("genres"),
        )
        .env(
            "INKOS_BUILTIN_SKILLS_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("packages")
                .join("core")
                .join("skills"),
        )
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
    wait_child_ready(&mut command, &bin, "/tmp/duel-bin-static-err.log").await;

    for path in [
        "/",
        "/editor/chapter/2",
        "/assets/app.js",
        "/assets/missing.js",
    ] {
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

    // CORS 面（125 号引入，173 号 W-A4a 收紧为回环 Origin 反射）：回环
    // Origin（vite dev 形态）双端反射 ACAO=Origin；远端 Origin 被守卫
    // 403 且无 CORS 头（bin/TS 侧守卫均在 CORS 之外先行短路）；preflight
    // （回环）双端 2xx 且反射 + 方法/头语义一致。
    let client = loopback_client();
    for base in [&bin, &ts.base] {
        // GET 用双端同位的 /api/v1/books（TS 侧无 /api/v1/health——404 面
        // 不在本 duel 断言范围）。
        let response = client
            .get(format!("{base}/api/v1/books"))
            .header("origin", "http://localhost:5173")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200, "{base} 回环 Origin 应放行");
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("http://localhost:5173"),
            "{base} 回环 Origin 应反射"
        );
        let response = client
            .get(format!("{base}/api/v1/books"))
            .header("origin", "http://duel.local")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 403, "{base} 远端 Origin 应 403");
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "{base} 403 响应不应携带 ACAO"
        );
        let response = client
            .request(reqwest::Method::OPTIONS, format!("{base}/api/v1/books"))
            .header("origin", "http://localhost:5173")
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
            Some("http://localhost:5173"),
            "{base} preflight 应反射回环 Origin"
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
        let response = client
            .request(reqwest::Method::OPTIONS, format!("{base}/api/v1/books"))
            .header("origin", "http://duel.local")
            .header("access-control-request-method", "POST")
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status().as_u16(),
            403,
            "{base} 远端 preflight 应被守卫 403"
        );
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "{base} 远端 preflight 不应携带 ACAO"
        );
    }

    let _ = command.kill();
    let _ = command.wait();
    eprintln!("静态面双端对跑通过：/ 与深链 SPA / 资产字节级一致，API 面不受干扰；CORS 面等价");
}

/// SSE 流事件采集：连接 → 触发聊天回合 → 等待终态事件后收束。
/// 返回原始 (event, payload JSON) 序列（ping/task:snapshot 已滤）。
async fn sse_chat_turn_events(
    base: &str,
    session_id: &str,
    instruction: &str,
) -> Vec<(String, Value)> {
    let events: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let target = base.to_string();
    let collector = tokio::spawn(async move {
        // 1583 号：顶层便捷函数同族漏网（读 env/系统代理）——恒直连。
        let response = match loopback_client()
            .get(format!("{target}/api/v1/events"))
            .send()
            .await
        {
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
    let client = loopback_client();
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

    let ts = spawn_ts_sidecar_ready(&root).await;
    // 711 号：契约源守卫前置到 bin spawn 之前（zombie_processes 早退丢弃路径
    // 消除，与 static_face 同型；守卫只探测 TS 腿，语义等价）。
    duel_require_fork_contract!(&ts);
    let bin_port = next_sidecar_port().await;
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_inkos-engine-server"))
        .env("INKOS_PROJECT_ROOT", &root)
        .env("INKOS_PORT", bin_port.to_string())
        .env(
            "INKOS_BUILTIN_GENRES_DIR",
            root.join("assets").join("genres"),
        )
        .env(
            "INKOS_BUILTIN_SKILLS_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("packages")
                .join("core")
                .join("skills"),
        )
        .stdout(std::process::Stdio::from(
            std::fs::File::create("/tmp/duel-bin-sse-out.log").unwrap(),
        ))
        .stderr(std::process::Stdio::from(
            std::fs::File::create("/tmp/duel-bin-sse-err.log").unwrap(),
        ))
        .spawn()
        .expect("bin 起动失败");
    let bin = format!("http://127.0.0.1:{bin_port}");
    wait_child_ready(&mut command, &bin, "/tmp/duel-bin-sse-err.log").await;

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

/// 185 号：timeline 双端契约对跑——GET 缺文件 `timeline:null`、PUT 合法落盘
/// roundtrip、PUT 非法（version≠1）400。normalize 剥除 updatedAt/version 后
/// 双端 DTO 必须等价（185 号：把「测试路由 vs 生产装配」「双端 DTO 等价」
/// 从单测提升到契约层——两台真进程对跑，装配差异无所遁形）。
#[tokio::test]
async fn strangler_timeline_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;
    duel_require_fork_contract!(&ts);

    let valid_timeline = json!({
        "version": 1,
        "bookId": "b1",
        "updatedAt": "2026-09-07T00:00:00.000Z",
        "plotlines": [
            { "id": "main", "name": "主线", "cells": [{ "chapter": 1, "title": "风起", "note": "主角入场" }] },
            { "id": "side", "name": "支线", "cells": [] }
        ]
    });
    let mut diffs: Vec<String> = Vec::new();

    // A) 缺文件 → 双端 200 + timeline:null。
    for base in [&rust, &ts.base] {
        let response = loopback_client()
            .get(format!("{base}/api/v1/books/b1/timeline"))
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let mut body: Value = response.json().await.unwrap_or(Value::Null);
        normalize(&mut body);
        if status != 200 || body != json!({ "timeline": null }) {
            diffs.push(format!("timeline GET 缺文件 {base} [{status}]: {body}"));
        }
    }

    // B) PUT 合法 → 双端 200 ok；PUT 非法 version → 双端 400。
    for base in [&rust, &ts.base] {
        let client = loopback_client();
        let ok = client
            .put(format!("{base}/api/v1/books/b1/timeline"))
            .json(&valid_timeline)
            .send()
            .await
            .unwrap();
        if ok.status().as_u16() != 200 {
            diffs.push(format!(
                "timeline PUT 合法 {base} [{:x}]",
                ok.status().as_u16()
            ));
        }
        let mut bad = valid_timeline.clone();
        bad["version"] = json!(2);
        let bad_response = client
            .put(format!("{base}/api/v1/books/b1/timeline"))
            .json(&bad)
            .send()
            .await
            .unwrap();
        if bad_response.status().as_u16() != 400 {
            diffs.push(format!(
                "timeline PUT 非法 {base} [{:x}]",
                bad_response.status().as_u16()
            ));
        }
    }

    // C) roundtrip：双端 GET（normalize 后等价——plotlines/cells 结构一致）。
    let mut bodies: Vec<(u16, Value)> = Vec::new();
    for base in [&rust, &ts.base] {
        let response = loopback_client()
            .get(format!("{base}/api/v1/books/b1/timeline"))
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let mut body: Value = response.json().await.unwrap_or(Value::Null);
        normalize(&mut body);
        bodies.push((status, body));
    }
    if bodies[0].0 != bodies[1].0 || bodies[0].1 != bodies[1].1 {
        diffs.push(format!(
            "timeline GET roundtrip 不等价\n  rust[{:?}]\n  ts  [{:?}]",
            bodies[0], bodies[1]
        ));
    }

    assert!(
        diffs.is_empty(),
        "timeline 契约差异 {} 处：\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

/// 185 号：series-backfill 双端契约对跑——extract 走 mock LLM（围栏 JSON 宽松
/// 解析）→ 草稿 DTO normalize 等价 → apply 勾选子集 → 校验面 400 语义。
#[tokio::test]
async fn strangler_series_backfill_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_backfill_mock_llm().await;
    write_fixture(&root, &llm);
    // 目标书 b2（extract 校验目标必须存在——双端同水位）。
    std::fs::create_dir_all(root.join("books").join("b2").join("story")).unwrap();
    std::fs::write(
        root.join("books").join("b2").join("book.json"),
        r#"{"id":"b2","title":"回填新书","platform":"other","genre":"xianxia","status":"active","targetChapters":10,"chapterWordCount":300,"language":"zh","createdAt":"2026-09-07T00:00:00.000Z","updatedAt":"2026-09-07T00:00:00.000Z"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("books")
            .join("b2")
            .join("story")
            .join("story_bible.md"),
        "新书圣经",
    )
    .unwrap();

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;
    duel_require_fork_contract!(&ts);

    let mut diffs: Vec<String> = Vec::new();

    // A) apply 无草稿 → 双端 400。
    for base in [&rust, &ts.base] {
        let response = loopback_client()
            .post(format!("{base}/api/v1/books/b1/series-backfill/apply"))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        if response.status().as_u16() != 400 {
            diffs.push(format!(
                "apply 无草稿 {base} [{:x}]",
                response.status().as_u16()
            ));
        }
    }

    // B) extract 缺 sourceBookId → 双端 400。
    for base in [&rust, &ts.base] {
        let response = loopback_client()
            .post(format!("{base}/api/v1/books/b1/series-backfill/extract"))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        if response.status().as_u16() != 400 {
            diffs.push(format!(
                "extract 缺参 {base} [{:x}]",
                response.status().as_u16()
            ));
        }
    }

    // C) extract 合法（走 mock LLM）→ 双端 200，草稿 normalize 后等价。
    let mut drafts: Vec<(u16, Value)> = Vec::new();
    for base in [&rust, &ts.base] {
        let response = loopback_client()
            .post(format!("{base}/api/v1/books/b2/series-backfill/extract"))
            .json(&json!({ "sourceBookId": "b1" }))
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let mut body: Value = response.json().await.unwrap_or(Value::Null);
        normalize(&mut body);
        drafts.push((status, body));
    }
    if drafts[0].0 != 200 || drafts[1].0 != 200 {
        diffs.push(format!(
            "extract 非双 200：{:?} / {:?}",
            drafts[0].0, drafts[1].0
        ));
    } else {
        let rust_items = drafts[0].1["draft"]["items"].clone();
        let ts_items = drafts[1].1["draft"]["items"].clone();
        if rust_items != ts_items {
            diffs.push(format!(
                "extract items DTO 不等价\n  rust: {rust_items}\n  ts:   {ts_items}"
            ));
        }
        // 宽松解析有效性：围栏 JSON 被提取为 2 条。
        for (label, body) in [("rust", &drafts[0].1), ("ts", &drafts[1].1)] {
            if body["draft"]["items"].as_array().map(|a| a.len()) != Some(2) {
                diffs.push(format!("extract {label} items 数量异常: {body}"));
            }
        }
    }

    // D) apply 勾选子集 → 双端 200 + applied:1。
    for base in [&rust, &ts.base] {
        let response = loopback_client()
            .post(format!("{base}/api/v1/books/b2/series-backfill/apply"))
            .json(&json!({ "itemIds": ["it-1"] }))
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let mut body: Value = response.json().await.unwrap_or(Value::Null);
        let applied = body.get("applied").cloned().unwrap_or(Value::Null);
        normalize(&mut body);
        if status != 200 || applied != json!(1) {
            diffs.push(format!("apply 勾选 {base} [{status}]: {body}"));
        }
    }

    assert!(
        diffs.is_empty(),
        "series-backfill 契约差异 {} 处：\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

/// R47/1640 号：全局搜索面 fork 契约对跑（688 守卫豁免记账同族——fork 契约
/// 复活后自动生效；当前双端同构的活体认证走 engine-contract-diff search
/// 通道，本测为复活面）。fixture：write_fixture 基线之外追加 search-book
/// 双布局播种（scripts/search-fixture.mjs 同内容内联——TS 腿读 works/<id>/
/// source 正典，Rust 腿读 books/<id>）；hits[].score 剥离后比对（686 备案
/// 双端 SQLite bm25 常数差 + ICU 版本漂移容忍面——token 序列由
/// search-tokenizer-vectors golden 锁定，命中集/序/片段=契约面）。
fn write_search_fixture(root: &Path) {
    let chapters: [(&str, &str); 4] = [
        (
            "0001_风起.md",
            "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。\n",
        ),
        (
            "0002_灵脉.md",
            "# 第2章 灵脉\n\n灵气紊乱，林动被迫闭关，灵气再度翻涌。\n",
        ),
        (
            "0003_林动传.md",
            "# 第3章 林动传\n\n与主角无关的旁支剧情。\n",
        ),
        ("0004_收束.md", "# 第4章 收束\n\n故事在此收束。\n"),
    ];
    let index = r#"[{"number":1,"title":"风起","status":"ready-for-review","wordCount":18,"createdAt":"2026-11-21T00:00:00.000Z","updatedAt":"2026-11-21T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]},{"number":2,"title":"灵脉","status":"ready-for-review","wordCount":20,"createdAt":"2026-11-21T00:00:00.000Z","updatedAt":"2026-11-21T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]},{"number":3,"title":"林动传","status":"ready-for-review","wordCount":12,"createdAt":"2026-11-21T00:00:00.000Z","updatedAt":"2026-11-21T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]},{"number":4,"title":"收束","status":"ready-for-review","wordCount":8,"createdAt":"2026-11-21T00:00:00.000Z","updatedAt":"2026-11-21T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]}]"#;
    let codex = r#"{"version":1,"cards":[{"name":"林动","aliases":["武祖"],"kind":"character","summary":"主角，修炼天才。","facts":["曾在青阳镇外门试炼","佩祖符玉佩"],"relationships":[{"target":"青阳镇","note":"出身地"}]}]}"#;
    let roster = "# 实体名册（Entity Roster）\n\n## entity-qingyang\n- name: 青阳镇\n- aliases: \n- kind: location\n- registeredAt: 0\n";
    let bible = "# 故事圣经\n\n主角林动，出身青阳镇，随身玉佩藏祖符来历。\n";
    let state_md = "| 字段 | 值 |\n|---|---|\n| 当前位置 | 碎镜崖之谜待解 |\n";
    for base in [
        root.join("books").join("search-book"),
        root.join("works").join("search-book").join("source"),
    ] {
        std::fs::create_dir_all(base.join("chapters")).unwrap();
        std::fs::create_dir_all(base.join("story")).unwrap();
        for (name, content) in chapters {
            std::fs::write(base.join("chapters").join(name), content).unwrap();
        }
        std::fs::write(base.join("chapters").join("index.json"), index).unwrap();
        std::fs::write(base.join("story").join("entity_codex.json"), codex).unwrap();
        std::fs::write(base.join("story").join("entity_roster.md"), roster).unwrap();
        std::fs::write(base.join("story").join("story_bible.md"), bible).unwrap();
        std::fs::write(base.join("story").join("current_state.md"), state_md).unwrap();
    }
    std::fs::write(
        root.join("books").join("search-book").join("book.json"),
        r#"{"id":"search-book","title":"搜索校准书","platform":"qidian","genre":"xuanhuan","status":"active","targetChapters":8,"chapterWordCount":1600,"language":"zh","createdAt":"2026-11-21T00:00:00.000Z","updatedAt":"2026-11-21T00:00:00.000Z"}"#,
    )
    .unwrap();
}

#[tokio::test]
async fn search_face_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);
    write_search_fixture(&root);

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;
    duel_require_fork_contract!(&ts);

    // score 剥离（词法实现面）：递归删除对象键 score。
    fn strip_scores(value: &mut Value) {
        match value {
            Value::Array(items) => items.iter_mut().for_each(strip_scores),
            Value::Object(map) => {
                map.remove("score");
                map.values_mut().for_each(strip_scores);
            }
            _ => {}
        }
    }

    let endpoints = [
        "/api/v1/books/search-book/search?q=%E7%81%B5%E6%B0%94&scope=chapters",
        "/api/v1/books/search-book/search?q=%E6%9E%97%E5%8A%A8&scope=chapters",
        "/api/v1/books/search-book/search?q=%E7%81%B5%E6%B0%94&scope=chapters&limit=1",
        "/api/v1/books/search-book/search?q=%E6%9E%97%E5%8A%A8&scope=entity",
        "/api/v1/books/search-book/search?q=%E9%9D%92%E9%98%B3%E9%95%87&scope=entity",
        "/api/v1/books/search-book/search?q=%E4%B9%8B%E8%B0%9C&scope=truth",
        "/api/v1/books/search-book/search?q=%E5%AE%8C%E5%85%A8%E4%B8%8D%E7%9B%B8%E5%85%B3&scope=chapters",
        "/api/v1/books/search-book/search?q=",
        "/api/v1/books/search-book/search?q=x&scope=bogus",
    ];

    let mut diffs: Vec<String> = Vec::new();
    for endpoint in endpoints {
        let (rust_status, mut rust_body) = get_json(&rust, endpoint).await;
        let (ts_status, mut ts_body) = get_json(&ts, endpoint).await;
        strip_scores(&mut rust_body);
        strip_scores(&mut ts_body);
        normalize(&mut rust_body);
        normalize(&mut ts_body);
        if rust_status != ts_status || rust_body != ts_body {
            diffs.push(format!(
                "DIFF {endpoint}\n  rust[{rust_status}]: {rust_body}\n  ts  [{ts_status}]: {ts_body}"
            ));
        }
    }

    assert!(
        diffs.is_empty(),
        "搜索面双端契约差异 {} 处：\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

/// R48a/1641 号：章节结构操作写面对跑（fork 契约复活面，688 豁免记账同族
/// ——当前双端逐位对齐的活体认证走 contract-diff 变异探针尾批 + 双端单测，
/// 本测为 fork 契约复活面）。三操作在双端各自 root 上顺序执行同参数
/// split→merge→reorder，随后对齐 `GET /books/b1` 的章清单与 `GET
/// /chapters/:num` 内容（normalize 剥易变键后逐位比对）。
#[tokio::test]
async fn chapter_structure_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;
    duel_require_fork_contract!(&ts);

    async fn post_json(base: &str, path: &str, body: &str) -> (u16, Value) {
        let response = loopback_client()
            .post(format!("{base}{path}"))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .expect("请求失败");
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        let parsed = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, parsed)
    }

    let mut diffs: Vec<String> = Vec::new();

    // ── split：2 号章按 UTF-16 offset 8 切两章 ──
    for (base, name) in [(&rust, "rust"), (&ts.base, "ts")] {
        let (status, body) =
            post_json(base, "/api/v1/books/b1/chapters/2/split", r#"{"offset":8}"#).await;
        if status != 200 {
            diffs.push(format!("[{name}] split status={status} body={body}"));
        }
    }
    let (rust_status, mut rust_body) = get_json(&rust, "/api/v1/books/b1").await;
    let (ts_status, mut ts_body) = get_json(&ts, "/api/v1/books/b1").await;
    normalize(&mut rust_body);
    normalize(&mut ts_body);
    if rust_status != ts_status || rust_body != ts_body {
        diffs.push(format!(
            "DIFF after split\n  rust[{rust_status}]: {rust_body}\n  ts  [{ts_status}]: {ts_body}"
        ));
    }

    // ── merge：3+4 相邻合并（split 后新章与原 3 号）──
    for (base, name) in [(&rust, "rust"), (&ts.base, "ts")] {
        let (status, body) = post_json(
            base,
            "/api/v1/books/b1/chapters/3/merge",
            r#"{"mergeWith":4}"#,
        )
        .await;
        if status != 200 {
            diffs.push(format!("[{name}] merge status={status} body={body}"));
        }
    }
    let (rust_status, mut rust_body) = get_json(&rust, "/api/v1/books/b1").await;
    let (ts_status, mut ts_body) = get_json(&ts, "/api/v1/books/b1").await;
    normalize(&mut rust_body);
    normalize(&mut ts_body);
    if rust_status != ts_status || rust_body != ts_body {
        diffs.push(format!(
            "DIFF after merge\n  rust[{rust_status}]: {rust_body}\n  ts  [{ts_status}]: {ts_body}"
        ));
    }

    // ── reorder：倒序全排列 ──
    for (base, name) in [(&rust, "rust"), (&ts.base, "ts")] {
        let (status, body) = post_json(
            base,
            "/api/v1/books/b1/chapters/reorder",
            r#"{"order":[3,2,1]}"#,
        )
        .await;
        if status != 200 {
            diffs.push(format!("[{name}] reorder status={status} body={body}"));
        }
    }
    // 终态逐位对齐：书目详情（章清单）+ 首末章内容。
    for endpoint in [
        "/api/v1/books/b1",
        "/api/v1/books/b1/chapters/1",
        "/api/v1/books/b1/chapters/3",
    ] {
        let (rust_status, mut rust_body) = get_json(&rust, endpoint).await;
        let (ts_status, mut ts_body) = get_json(&ts, endpoint).await;
        normalize(&mut rust_body);
        normalize(&mut ts_body);
        if rust_status != ts_status || rust_body != ts_body {
            diffs.push(format!("DIFF {endpoint}\n  rust[{rust_status}]: {rust_body}\n  ts  [{ts_status}]: {ts_body}"));
        }
    }

    assert!(
        diffs.is_empty(),
        "章节结构双端契约差异 {} 处：\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}
