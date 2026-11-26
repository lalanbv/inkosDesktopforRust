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

// 支撑模块（真实引擎二进制的启动命令）。crate 根文件的子模块默认在 `tests/`
// 一级目录下找——那里每个文件都是一个集成测试目标，所以显式指到同名子目录。
#[path = "strangler_duel/engine_bin.rs"]
mod engine_bin;

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
    write_fixture_with_chapters(root, llm, &[]);
}

/// `extra` = 追加章节 `(文件名词干, 标题, 正文)`，章号自 2 顺延——各 duel 的
/// 既有基线仍是 1 章（`write_fixture`）；需要多章的对跑（章节结构操作）在此
/// 补足。追加章须在迁移到 works 布局**之前**落盘，两棵树才同源。
fn write_fixture_with_chapters(root: &Path, llm: &str, extra: &[(&str, &str, &str)]) {
    write_fixture_book(root, extra);
    write_fixture_llm(root, llm);
    migrate_fixture_to_works(root);
}

/// 夹具的书与文种文件（与 LLM 配置无关的部分）——配置可用与缺 key 两种夹具
/// 共用（1672 号拆出：原独立二进制 `tests/llm_config_duel.rs` 的夹具自带一份
/// 同款书文件，并入时收编到这里）。
fn write_fixture_book(root: &Path, extra: &[(&str, &str, &str)]) {
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
    let mut index = r#"[{"number":1,"title":"风起","status":"ready-for-review","wordCount":18,"createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]}"#.to_string();
    for (offset, (stem, title, body)) in extra.iter().enumerate() {
        let number = offset + 2;
        std::fs::write(
            book.join("chapters").join(format!("{stem}.md")),
            format!("# 第{number}章 {title}\n\n{body}"),
        )
        .unwrap();
        index.push_str(&format!(
            r#",{{"number":{number},"title":"{title}","status":"ready-for-review","wordCount":{},"createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z","auditIssues":[],"lengthWarnings":[]}}"#,
            body.chars().count()
        ));
    }
    index.push(']');
    std::fs::write(book.join("chapters").join("index.json"), index).unwrap();
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
}

/// 夹具的 LLM 服务项：回环 mock + 对跑 key。
fn write_fixture_llm(root: &Path, llm: &str) {
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

/// 1672 号：**缺 key** 的项目根——服务项指向非回环端点、secrets 缺席（回环端点
/// 免 key，测不到「缺 key」）。书与 [`write_fixture`] 同一份、同样双根布局，
/// 两腿都认得这本书，对跑打的才是配置问题而不是「书不存在」。
fn write_unconfigured_fixture(root: &Path) {
    write_fixture_book(root, &[]);
    std::fs::write(
        root.join("inkos.json"),
        r#"{"version":"0.1.0","name":"未配置对跑项目","language":"zh","llm":{"services":{"custom:None":{"service":"custom","name":"None","baseUrl":"https://example.invalid/v1","apiFormat":"chat"}},"defaultModel":"none-model"},"notify":[]}"#,
    )
    .unwrap();
    migrate_fixture_to_works(root);
}

/// 1666 号：双根布局——同一份书同时存在于 `books/b1`（fork 引擎读）与
/// `works/b1/source` + `work.json`（2.0 TS 腿按 works 清单发现作品）。经 TS 侧
/// 同一个迁移函数生成（`scripts/migrate-legacy-works.mjs` →
/// `core.migrateLegacyWorks`，与 CLI `inkos work migrate` 同源）：works 清单
/// 形态不在这里手搓——手搓=第二真源，清单 schema 一变就是假红。
///
/// 只读面与「各腿写各读」的对跑据此成立。**跨引擎写读互证**（Rust 写 → TS 读
/// 可见）不因此成立：两腿写的是各自的树，见 `duel_require_shared_storage!`。
fn migrate_fixture_to_works(root: &Path) {
    let repo = repo_root();
    let output = std::process::Command::new("node")
        .arg(repo.join("scripts").join("migrate-legacy-works.mjs"))
        .arg(root)
        .current_dir(&repo)
        .output()
        .expect("node 起动失败（夹具迁移到 works 布局）");
    assert!(
        output.status.success(),
        "夹具迁移到 works 布局失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// 1666 号：请求工具表是否含 `finish_turn`（2.0 会话收尾协议——TS 腿会话
/// streamFn 恒 `toolChoice:"required"`，每轮须以工具调用收尾；Rust 腿聊天
/// 请求的工具表没有它）。
fn wants_finish_turn(body: &Value) -> bool {
    body["tools"].as_array().is_some_and(|tools| {
        tools
            .iter()
            .any(|tool| tool["function"]["name"] == "finish_turn")
    })
}

/// finish_turn 调用 id（每次请求唯一，防同一会话两轮工具调用 id 相撞）。
fn next_finish_turn_id() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("call_finish_duel_{seq}")
}

/// 进程内 mock LLM（对跑不依赖真实端点——两侧读同一 inkos.json）。
/// 548 号（R30）：响应携带 OpenAI 规范必需的 finish_reason——pi-ai 0.87 严检
/// "Stream ended without finish_reason"（0.73 宽容缺失，0.87 起报错）。
async fn spawn_mock_llm() -> String {
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
            // 1666 号：2.0 会话收尾协议——工具表含 finish_turn 时回工具调用
            //（walkthrough-mock 1663 同款）。此前只回纯文本 → TS 腿
            // MODEL_REQUIRED_TOOL_MISSING → /agent 502；Rust 腿工具表无
            // finish_turn，走原文本支不受影响。双形态：流式 → tool_calls 增量
            // + finish_reason:"tool_calls"；非流式 → 整体 JSON。
            if wants_finish_turn(&body) {
                let id = next_finish_turn_id();
                let function = serde_json::json!({
                    "name": "finish_turn",
                    "arguments": "{\"status\":\"answered\",\"message\":\"OK\"}",
                });
                if body["stream"].as_bool().unwrap_or(false) {
                    let chunk = serde_json::json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "id": id, "function": function }] }, "finish_reason": null }] });
                    let finish = serde_json::json!({ "choices": [{ "delta": {}, "finish_reason": "tool_calls" }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                    return axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {chunk}\n\ndata: {finish}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ));
                }
                return axum::response::IntoResponse::into_response(axum::Json(serde_json::json!({
                    "choices": [{
                        "message": { "role": "assistant", "content": null, "tool_calls": [{ "id": id, "type": "function", "function": function }] },
                        "finish_reason": "tool_calls",
                    }],
                    "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
                })));
            }
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

/// Rust engine 进程内全量路由（bin 同款装配）：启动路由器按 `llm` 构造（对跑
/// mock + 对跑 key）。
async fn spawn_rust_engine(root: &Path, llm: &str) -> String {
    spawn_rust_engine_with(root, Some(llm), None).await
}

/// 同 [`spawn_rust_engine`]，启动路由器可由调用方给定：`startup_router =
/// Some(..)` 时覆盖按 `llm` 构造的对跑路由器——缺 key 对跑要的是 bin 的未配置
/// 占位（discard 端点 + 空 key），不是 env 配置形态。
async fn spawn_rust_engine_with(
    root: &Path,
    llm: Option<&str>,
    startup_router: Option<std::sync::Arc<inkos_engine::llm::agent_router::AgentRouter>>,
) -> String {
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
            base_url: llm.unwrap_or_default().into(),
            api_key: "duel-key".into(),
            model: "duel-model".into(),
            max_tokens: 8192,
            extra_headers: std::collections::HashMap::new(),
        },
        std::collections::HashMap::new(),
    ));
    // 调用方给定启动路由器时，覆盖上面按 `llm` 构造的对跑路由器。
    let router = startup_router.unwrap_or(router);
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

/// 1666 号：引擎 bin 子进程守卫（与 [`TsSidecar`] 的 RAII 同纪律）——Drop 即
/// kill + wait，**断言失败的展开路径同样回收**。裸 `Child` 被 Drop 不会终止子
/// 进程：此前三处起 bin 的对跑只在成功路径末尾 `kill + wait`，测试中途 panic
/// 即留占口孤儿（实录：SSE 对跑间歇红一次，引擎 bin 存活到下一轮门禁）。
/// `Child::kill` 对已 wait 的子进程不发信号，无 PID 复用误杀面。
struct EngineBin(std::process::Child);

impl Drop for EngineBin {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl std::ops::Deref for EngineBin {
    type Target = std::process::Child;
    fn deref(&self) -> &std::process::Child {
        &self.0
    }
}

impl std::ops::DerefMut for EngineBin {
    fn deref_mut(&mut self) -> &mut std::process::Child {
        &mut self.0
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

/// 1666 号：按路径剥键。路径是点分串，`name[]` = 先下钻 `name` 再对数组逐
/// 元素下钻（如 `chapters[].status`）。路径不存在则无操作。
fn remove_path(value: &mut Value, path: &str) {
    let mut segments: Vec<&str> = Vec::new();
    for part in path.split('.') {
        match part.strip_suffix("[]") {
            Some(name) => {
                segments.push(name);
                segments.push("[]");
            }
            None => segments.push(part),
        }
    }
    remove_segments(value, &segments);
}

fn remove_segments(value: &mut Value, path: &[&str]) {
    let Some((segment, rest)) = path.split_first() else {
        return;
    };
    if *segment == "[]" {
        if let Value::Array(items) = value {
            for item in items.iter_mut() {
                remove_segments(item, rest);
            }
        }
    } else if rest.is_empty() {
        if let Value::Object(map) = value {
            map.remove(*segment);
        }
    } else if let Some(child) = value.get_mut(*segment) {
        remove_segments(child, rest);
    }
}

type DeclaredPaths = &'static [&'static str];

/// 一条**已声明**的双端差异（698 号口径：共有键必须相等；各腿的超集键与已
/// 备案的数据模型差须逐条声明，声明后才剥离；未声明的差异一律判红）。
struct DeclaredDiff {
    /// 端点匹配：整串相等；以 `*` 结尾 = 前缀匹配。
    endpoint: &'static str,
    /// 仅 Rust 有的键（从 Rust 侧剥）。
    rust_only: DeclaredPaths,
    /// 仅 TS 有的键（从 TS 侧剥）。
    ts_only: DeclaredPaths,
    /// 共有键但值口径不同（两侧都剥）——须附实证理由。
    both: DeclaredPaths,
    reason: &'static str,
}

/// 1666 号：双端已声明的差异清单。每条都是实测取证后的备案（强制打开守卫
/// 的差异输出见变更记录 1666 ⑧），不是「对不上就豁免」：
const DECLARED_DIFFS: &[DeclaredDiff] = &[
    DeclaredDiff {
        endpoint: "/api/v1/books",
        rust_only: &[],
        ts_only: &["incompleteWorkIds"],
        both: &[],
        reason: "书列表：2.0 加法字段 incompleteWorkIds（未完成迁移的 work 清单），fork 无",
    },
    DeclaredDiff {
        endpoint: "/api/v1/books/b1",
        rust_only: &[],
        ts_only: &["chapters[].provenance", "chapters[].observations"],
        // 1666 号章节结构对跑真执行后勘误：这三键不是「仅 Rust 有」——TS 腿对
        // split 新切出的章同样透出（两腿磁盘同写 ready-for-review 惯例行），只是
        // 迁移来的章在 TS 侧不带。改两侧都剥。
        both: &["chapters[].status", "chapters[].auditIssues", "chapters[].lengthWarnings"],
        reason: "章摘要形态：2.0（TS）迁移来的章只带 provenance/observations；status/auditIssues/lengthWarnings 两腿对结构操作新建的章都写、迁移章只在 fork（Rust）带——按键来源不定，两侧都剥",
    },
    DeclaredDiff {
        endpoint: "/api/v1/books/b1/truth",
        rust_only: &["files[].legacy"],
        ts_only: &[],
        both: &[],
        reason: "truth 文件清单：白名单双端差（TS 9 项无 story_bible / Rust 15 项有）+ Rust 的 legacy 标记——只比交集文件",
    },
    DeclaredDiff {
        endpoint: "/api/v1/books/b1/analytics",
        rust_only: &[],
        ts_only: &[],
        both: &["auditPassRate", "statusDistribution"],
        reason: "analytics：2.0 迁移不携带章状态（TS 章摘要 status 缺失→空串→通过率 0），Rust 按 legacy 索引 ready-for-review 计 100",
    },
    DeclaredDiff {
        endpoint: "/api/v1/sessions/*",
        rust_only: &["session.draftRounds", "session.events"],
        ts_only: &[
            "chatRequest",
            "session.profileId",
            "session.workId",
            "session.proposalAction",
            "session.modelOverride",
            "session.serviceOverride",
        ],
        both: &[],
        // 立案不改（fork 不变量，217/565 号）：Rust 读面只投影已提交请求，2.0
        // 把失败/中断请求也留在读面（request_failed → system 消息）。对跑夹具
        // 的回合恒提交，此差不出现在比对面上。
        reason: "会话读面超集键：2.0 的会话元数据加法键 profileId/workId/proposalAction/modelOverride/serviceOverride 与 chatRequest/task（1666 活体逐键取证，schema session-transcript-schema.ts）；fork 的 draftRounds/events（共享界面均不消费）",
    },
];

fn endpoint_matches(pattern: &str, endpoint: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => endpoint.starts_with(prefix),
        None => pattern == endpoint,
    }
}

/// truth 清单只比交集文件名（白名单双端差：一侧多出的文件不是分歧）。
fn intersect_truth_files(rust_body: &mut Value, ts_body: &mut Value) {
    let names = |body: &Value| -> std::collections::BTreeSet<String> {
        body["files"]
            .as_array()
            .map(|files| {
                files
                    .iter()
                    .filter_map(|file| file["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let rust_names = names(rust_body);
    let ts_names = names(ts_body);
    for body in [rust_body, ts_body] {
        if let Some(files) = body.get_mut("files").and_then(Value::as_array_mut) {
            files.retain(|file| {
                file["name"]
                    .as_str()
                    .is_some_and(|name| rust_names.contains(name) && ts_names.contains(name))
            });
        }
    }
}

/// 对 (endpoint, 双端 body) 应用已声明的差异；返回生效的声明（供输出留痕）。
fn apply_declared(endpoint: &str, rust_body: &mut Value, ts_body: &mut Value) -> Vec<&'static str> {
    let mut applied = Vec::new();
    for diff in DECLARED_DIFFS {
        if !endpoint_matches(diff.endpoint, endpoint) {
            continue;
        }
        for path in diff.rust_only {
            remove_path(rust_body, path);
        }
        for path in diff.ts_only {
            remove_path(ts_body, path);
        }
        for path in diff.both {
            remove_path(rust_body, path);
            remove_path(ts_body, path);
        }
        if diff.endpoint.ends_with("/truth") {
            intersect_truth_files(rust_body, ts_body);
        }
        applied.push(diff.reason);
    }
    applied
}

/// 双端读面比较：应用已声明差异 → 波动键归一 → 状态码与 body 全等。
/// 返回差异串（None = 一致）。`declared` 回收生效的声明，供调用方打印留痕。
fn compare_face(
    endpoint: &str,
    rust_status: u16,
    mut rust_body: Value,
    ts_status: u16,
    mut ts_body: Value,
    declared: &mut Vec<&'static str>,
) -> Option<String> {
    declared.extend(apply_declared(endpoint, &mut rust_body, &mut ts_body));
    normalize(&mut rust_body);
    normalize(&mut ts_body);
    if rust_status == ts_status && rust_body == ts_body {
        return None;
    }
    Some(format!(
        "DIFF {endpoint}\n  rust[{rust_status}]: {rust_body}\n  ts  [{ts_status}]: {ts_body}"
    ))
}

/// 1666 号：**共享存储前提**守卫——用于「跨引擎写读互证」类对跑（Rust 写 →
/// TS 读可见，或反之）。前提目前不成立：Rust 引擎读写 `books/<id>/`，2.0 TS
/// 腿读写 `works/<id>/source/`，旧书迁移只在 CLI `inkos work migrate` 显式执行、
/// 服务启动不自动迁移、Rust 不读 works。这不是夹具能补的：两腿写的是各自的
/// 树。复活条件 = 存储模型定线（Rust 迁到 works，或自动双向桥接）。
///
/// 与旧守卫（`/api/v1/health` 判据，对 TS 腿恒判豁免）的区别：豁免理由写在
/// 这里、按测试逐条声明，不再靠一个恒假的探测把 11 个对跑一并跳过。取证：
/// `INKOS_DUEL_FORCE_OPEN=1` 解除豁免（解除即严格计失败）。
macro_rules! duel_require_shared_storage {
    ($what:expr) => {
        if std::env::var_os("INKOS_DUEL_FORCE_OPEN").is_none() {
            record_duel_ledger("exempt");
            eprintln!(
                "[duel:exempt] {}（共享存储前提不成立：Rust books/ ↔ TS works/）——{}",
                std::thread::current().name().unwrap_or("?"),
                $what
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
    let mut declared: Vec<&'static str> = Vec::new();
    for endpoint in endpoints {
        let (rust_status, rust_body) = get_json(&rust, endpoint).await;
        let (ts_status, ts_body) = get_json(&ts, endpoint).await;
        if let Some(diff) = compare_face(
            endpoint,
            rust_status,
            rust_body,
            ts_status,
            ts_body,
            &mut declared,
        ) {
            diffs.push(diff);
        }
    }

    // 生效的声明留痕（去重）——「一致」始终指「共有契约一致」。
    declared.sort_unstable();
    declared.dedup();
    for reason in &declared {
        eprintln!("[duel:declared] {reason}");
    }
    if diffs.is_empty() {
        eprintln!(
            "对跑通过：{} 个只读端点共有契约全一致（已声明差异 {} 条）",
            endpoints.len(),
            declared.len()
        );
    } else {
        eprintln!("对跑差异 {} 处：", diffs.len());
        for diff in &diffs {
            eprintln!("{diff}");
        }
    }
    assert!(diffs.is_empty(), "只读面双端存在未声明的契约差异（见上）");
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
    duel_require_shared_storage!(
        "Rust 写 approve → 双端读 approved；TS 写 review-mode → 双端读 manual"
    );

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
        apply_declared(endpoint, &mut rust_body, &mut ts_body);
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
    // 族在位判据用**族根端点自身**（`GET /prompt-packs` 200）。本族 TS 回退腿
    // 1663 号起同构；不在位则豁免并落台账（与 `duel_require_shared_storage!`
    // 同款记账——早退不得计成一次真跑）。
    match get_json(&ts, "/api/v1/prompt-packs").await {
        (200, _) => {}
        (status, body) => {
            record_duel_ledger("exempt");
            eprintln!(
                "[duel:exempt] {}（TS 腿无 prompt-packs 族：{status}）{body}",
                std::thread::current().name().unwrap_or("?")
            );
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
    apply_declared(
        &format!("/api/v1/sessions/{rust_session}"),
        &mut rust_body,
        &mut ts_body,
    );
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
    chat_turn(&client, &ts, ts_session, "打个招呼").await;
    let (rust_status, rust_body) = get_json(&rust, &format!("/api/v1/sessions/{ts_session}")).await;
    let (ts_status, ts_body) = get_json(&ts, &format!("/api/v1/sessions/{ts_session}")).await;
    assert_eq!(rust_status, 200);
    assert_eq!(ts_status, 200);
    // 1666 号：助手回复双端都在——Rust 腿此前不投影 `display.completion.message`，
    // 读同一份 2.0 transcript 只剩 user 消息（修复前此处左值即实录）。先逐腿钉
    // 形态，防两端同为空也「全等」。
    assert_turn_face(
        "C",
        &[("user", "打个招呼"), ("assistant", "OK")],
        &rust_body,
        &ts_body,
    );
    assert_eq!(
        session_face(&rust, &ts, ts_session).await,
        Face::Equal,
        "聊天回合后双端会话读面不等价"
    );

    // D) 1666 号：R36 会话树化双端对偶。TS 再聊一轮 → branch 回第一轮末尾 →
    //    双端读面只剩第一轮（Node 腿此前读面、模型回放、缓存 Agent 三处都不按
    //    active 链，分支后弃用轮次照常出现）；分支上再聊一轮 → 第一轮 + 新轮。
    let turn_one: &[(&str, &str)] = &[("user", "打个招呼"), ("assistant", "OK")];
    let turn_two: &[(&str, &str)] = &[("user", "再问一句"), ("assistant", "OK")];
    let turn_three: &[(&str, &str)] = &[("user", "分支上的新问"), ("assistant", "OK")];
    chat_turn(&client, &ts, ts_session, "再问一句").await;
    let commits = committed_seqs(&root, ts_session);
    assert_eq!(commits.len(), 2, "两轮各一个提交点：{commits:?}");
    let response = client
        .post(format!("{ts}/api/v1/sessions/{ts_session}/branch"))
        .header("content-type", "application/json")
        .body(format!(r#"{{"toSeq":{}}}"#, commits[0]))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200, "TS branch 失败");
    assert_eq!(
        session_face(&rust, &ts, ts_session).await,
        Face::Equal,
        "分支后双端会话读面不等价"
    );
    for base in [&*rust, &*ts] {
        let (_, body) = get_json(base, &format!("/api/v1/sessions/{ts_session}")).await;
        assert_eq!(
            turn_texts(&body["session"]["messages"]),
            face(turn_one),
            "base={base} 分支后读面仍带弃用轮次"
        );
        assert_eq!(body["session"]["head"], commits[0], "base={base}");
        assert_eq!(body["session"]["branchCount"], 1, "base={base}");
    }

    chat_turn(&client, &ts, ts_session, "分支上的新问").await;
    assert_eq!(
        session_face(&rust, &ts, ts_session).await,
        Face::Equal,
        "分支上新聊一轮后双端会话读面不等价"
    );
    for base in [&*rust, &*ts] {
        let (_, body) = get_json(base, &format!("/api/v1/sessions/{ts_session}")).await;
        assert_eq!(
            turn_texts(&body["session"]["messages"]),
            face(&[turn_one, turn_three].concat()),
            "base={base} 分支上的新轮读面"
        );
    }

    // E) Rust 腿执行分支：切回弃用的第二轮（两腿写的 branch_moved 互相可读——
    //    parentSeq 戳记同构），双端读面回到「第一轮 + 第二轮」。
    let commits = committed_seqs(&root, ts_session);
    assert_eq!(commits.len(), 3, "三个提交点：{commits:?}");
    let response = client
        .post(format!("{rust}/api/v1/sessions/{ts_session}/branch"))
        .header("content-type", "application/json")
        .body(format!(r#"{{"toSeq":{}}}"#, commits[1]))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200, "Rust branch 失败");
    assert_eq!(
        session_face(&rust, &ts, ts_session).await,
        Face::Equal,
        "Rust 分支后双端会话读面不等价"
    );
    for base in [&*rust, &*ts] {
        let (_, body) = get_json(base, &format!("/api/v1/sessions/{ts_session}")).await;
        assert_eq!(
            turn_texts(&body["session"]["messages"]),
            face(&[turn_one, turn_two].concat()),
            "base={base} 切回弃用分支"
        );
        assert_eq!(body["session"]["head"], commits[1], "base={base}");
        assert_eq!(body["session"]["branchCount"], 2, "base={base}");
    }
    eprintln!(
        "会话域对跑通过：Rust 建会话双端等价 / TS 建+改名跨端可见 / 聊天回合后读面等价（助手回复双端在位）/ \
         R36 树化三步（TS 分支→双端只剩第一轮、分支上新聊→第一轮+新轮、Rust 分支切回→第一轮+第二轮）双端等价"
    );
}

/// 会话 transcript 里的提交点 seq（按出现序）。
fn committed_seqs(root: &Path, session_id: &str) -> Vec<u64> {
    let path = root
        .join(".inkos")
        .join("sessions")
        .join(format!("{session_id}.jsonl"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} 读取失败：{error}", path.display()));
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["type"] == "request_committed")
        .filter_map(|event| event["seq"].as_u64())
        .collect()
}

/// 对指定腿打一轮聊天（mock LLM 恒回 OK），断言 200。
async fn chat_turn(client: &reqwest::Client, base: &str, session_id: &str, instruction: &str) {
    let response = client
        .post(format!("{base}/api/v1/agent"))
        .header("content-type", "application/json")
        .body(format!(
            r#"{{"instruction":"{instruction}","sessionId":"{session_id}"}}"#
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status().as_u16(),
        200,
        "TS 聊天回合「{instruction}」失败: {}",
        response.text().await.unwrap_or_default()
    );
}

/// 读面消息 → `(role, content)` 序列。
fn turn_texts(messages: &Value) -> Vec<(String, String)> {
    messages
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|message| {
                    (
                        message["role"].as_str().unwrap_or_default().to_string(),
                        message["content"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 期望读面 `(role, content)` 序列。
fn face(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(role, content)| (role.to_string(), content.to_string()))
        .collect()
}

/// 逐腿钉读面消息形态（防「对齐数据=回声室」：两端同为空也全等）。
fn assert_turn_face(step: &str, expected: &[(&str, &str)], rust_body: &Value, ts_body: &Value) {
    for (leg, body) in [("rust", rust_body), ("ts", ts_body)] {
        assert_eq!(
            turn_texts(&body["session"]["messages"]),
            face(expected),
            "步骤 {step}：{leg} 腿读面消息形态"
        );
    }
}

/// 双端会话读面比对结果（差异串留在变体里，供断言消息打印）。
#[derive(Debug, PartialEq)]
enum Face {
    Equal,
    Different(String),
}

/// 双端 `GET /sessions/:id` → 应用已声明差异 + 波动键归一 → 全等判定。
async fn session_face(rust: &str, ts: &str, session_id: &str) -> Face {
    let endpoint = format!("/api/v1/sessions/{session_id}");
    let (rust_status, rust_body) = get_json(rust, &endpoint).await;
    let (ts_status, ts_body) = get_json(ts, &endpoint).await;
    match compare_face(
        &endpoint,
        rust_status,
        rust_body,
        ts_status,
        ts_body,
        &mut Vec::new(),
    ) {
        None => Face::Equal,
        Some(diff) => Face::Different(diff),
    }
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

    // D) 1666 号：服务配置是**字段级**合并（TS `{...previous, ...update}`）——
    //    更新条目里没出现的字段保留；`temperature: null` = 恢复默认（选中服务时
    //    顶层 llm.temperature 一并删除）。此前 Rust 同键整条替换：只提交预算会
    //    抹掉 baseUrl 等字段（活体走查实证：设日预算后 custom 服务端点丢失，
    //    预算读面按端点主机匹配账本行随之恒 0、确认门永不触发）。
    //    两腿写的是同一个项目根：每条腿 PUT 后**立刻读盘**，观测这条腿自己的
    //    落盘结果——读面会按选中服务重新推导温度，拿 GET /project 当「顶层键已
    //    删除」的证据不成立（对跑级变异实证：关掉删除后对跑仍绿）。
    let legs = [(&rust, "rust"), (&ts.base, "ts")];
    let stored_llm = || -> Value {
        let text = std::fs::read_to_string(root.join("inkos.json")).unwrap();
        let config: Value = serde_json::from_str(&text).unwrap();
        config["llm"].clone()
    };
    let stored_entry = |llm_config: &Value| -> Value {
        llm_config["services"]
            .as_array()
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["service"] == "custom" && entry["name"] == "Duel")
            })
            .cloned()
            .unwrap_or_else(|| panic!("custom:Duel 条目缺失：{llm_config}"))
    };
    // D1) 部分更新：只提交温度与日预算（不带 baseUrl / apiFormat / stream）。
    for (base, leg) in &legs {
        let (status, body) = send_json(
            &client,
            base,
            reqwest::Method::PUT,
            "/api/v1/services/config",
            Some(r#"{"service":"custom:Duel","services":{"custom:Duel":{"temperature":0.5,"dailyTokenBudget":1}}}"#),
        )
        .await;
        assert_eq!(status, 200, "{leg} 部分更新失败：{body}");
        let stored = stored_llm();
        let entry = stored_entry(&stored);
        assert_eq!(
            entry["baseUrl"], llm,
            "{leg} 部分更新抹掉了 baseUrl：{entry}"
        );
        assert_eq!(
            entry["apiFormat"], "chat",
            "{leg} 部分更新抹掉了 apiFormat：{entry}"
        );
        assert_eq!(
            entry["stream"], true,
            "{leg} 部分更新抹掉了 stream：{entry}"
        );
        assert_eq!(entry["temperature"], 0.5, "{leg} 温度未落盘：{entry}");
        assert_eq!(entry["dailyTokenBudget"], 1, "{leg} 预算未落盘：{entry}");
        assert_eq!(stored["service"], "custom:Duel", "{leg} 选中服务：{stored}");
        assert_eq!(
            stored["temperature"], 0.5,
            "{leg} 选中服务的温度应镜像到顶层：{stored}"
        );
    }
    // D2) 恢复默认：`temperature: null`——条目与顶层镜像的 temperature 一并消失，
    //     其余字段（端点、预算等）仍在。
    for (base, leg) in &legs {
        let (status, body) = send_json(
            &client,
            base,
            reqwest::Method::PUT,
            "/api/v1/services/config",
            Some(r#"{"services":[{"service":"custom","name":"Duel","temperature":null}]}"#),
        )
        .await;
        assert_eq!(status, 200, "{leg} 恢复默认失败：{body}");
        let stored = stored_llm();
        let entry = stored_entry(&stored);
        assert!(
            entry.get("temperature").is_none(),
            "{leg} 恢复默认后条目仍带温度：{entry}"
        );
        assert_eq!(
            entry["baseUrl"], llm,
            "{leg} 恢复默认抹掉了 baseUrl：{entry}"
        );
        assert_eq!(
            entry["apiFormat"], "chat",
            "{leg} 恢复默认抹掉了 apiFormat：{entry}"
        );
        assert_eq!(
            entry["stream"], true,
            "{leg} 恢复默认抹掉了 stream：{entry}"
        );
        assert_eq!(
            entry["dailyTokenBudget"], 1,
            "{leg} 恢复默认抹掉了预算：{entry}"
        );
        assert!(
            stored.get("temperature").is_none(),
            "{leg} 顶层温度镜像未随恢复默认删除：{stored}"
        );
    }
    // 读面复核：两腿读同一份盘上结果，形态一致。
    let mut faces = Vec::new();
    for (base, leg) in &legs {
        let (status, body) = get_json(base, "/api/v1/services/config").await;
        assert_eq!(status, 200, "{leg}");
        let entry = stored_entry(&json!({ "services": body["services"] }));
        assert_eq!(entry["baseUrl"], llm, "{leg} 读面端点：{entry}");
        assert_eq!(entry["dailyTokenBudget"], 1, "{leg} 读面预算：{entry}");
        assert!(
            entry.get("temperature").is_none(),
            "{leg} 读面温度：{entry}"
        );
        faces.push(entry);
    }
    assert_eq!(faces[0], faces[1], "恢复默认后两腿服务条目读面不等价");

    eprintln!(
        "模型配置域对跑通过：服务列表面观察 / 密钥双向跨端写读 / 深链探测等价 / \
         服务配置字段级合并（部分更新不丢字段、恢复默认温度）逐腿读盘同果"
    );
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
    duel_require_shared_storage!(
        "第二步 Rust 写 approve 后双端读一致；第三步 TS 写 review-mode 后双端读一致"
    );
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
        .map(EngineBin)
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

    // 引擎 bin 由 EngineBin 守卫在作用域结束时回收（含上面的断言失败路径）。
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
        .map(EngineBin)
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

    // 引擎 bin 由 EngineBin 守卫在作用域结束时回收。
    eprintln!("静态面双端对跑通过：/ 与深链 SPA / 资产字节级一致，API 面不受干扰；CORS 面等价");
}

/// SSE 流事件采集：连接 → **等订阅真正建立** → 触发聊天回合 → 等待终态事件后收束。
/// 返回原始 (event, payload JSON) 序列（ping/task:snapshot 已滤）。
///
/// 1666 号：订阅自同步——两端 `/api/v1/events` 都是「先订阅、连接建立即发首个
/// ping」，收到该 ping 即订阅已在服务端生效。此前 spawn 收集任务后立刻打回合：
/// Rust 腿是进程内路由，回合抢在订阅前，首个事件 `agent:start` 广播给空集
///（最终树门禁实录：`Rust 侧缺 agent:start`，间歇红）。
async fn sse_chat_turn_events(
    base: &str,
    session_id: &str,
    instruction: &str,
) -> Vec<(String, Value)> {
    let events: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let target = base.to_string();
    let (subscribed_tx, subscribed_rx) = tokio::sync::oneshot::channel::<()>();
    let mut subscribed_tx = Some(subscribed_tx);
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
                if name.is_empty() {
                    continue;
                }
                if name == "ping" {
                    if let Some(signaller) = subscribed_tx.take() {
                        let _ = signaller.send(());
                    }
                    continue;
                }
                if name == "task:snapshot" {
                    continue;
                }
                let payload = serde_json::from_str(&data).unwrap_or(Value::String(data));
                sink.lock().unwrap().push((name, payload));
            }
        }
    });
    // 订阅在服务端生效后才发起回合（未收到连接 ping = 订阅未就绪，判负而非赌时序）。
    if !matches!(
        tokio::time::timeout(Duration::from_secs(5), subscribed_rx).await,
        Ok(Ok(()))
    ) {
        panic!("{base} SSE 订阅 5s 未就绪（未收到连接 ping）");
    }

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
        .map(EngineBin)
        .expect("bin 起动失败");
    let bin = format!("http://127.0.0.1:{bin_port}");
    wait_child_ready(&mut command, &bin, "/tmp/duel-bin-sse-err.log").await;

    let instruction = "帮我看下第二章节奏";
    let ts_events =
        normalize_sse_face(sse_chat_turn_events(&ts, "1783100000001-ssets", instruction).await);
    let rust_events =
        normalize_sse_face(sse_chat_turn_events(&bin, "1783100000002-sseeb", instruction).await);

    // 七类事件齐全（Rust 侧回合健康性；llm:progress 不在普通聊天轮词汇——
    // 128 号勘误：TS 仅 pipeline 面上报；thinking 三事件 129 号补齐）。
    let ts_names: Vec<&str> = ts_events.iter().map(|(name, _)| name.as_str()).collect();
    let rust_all_names: Vec<&str> = rust_events.iter().map(|(name, _)| name.as_str()).collect();
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
            rust_all_names.contains(&expected),
            "Rust 侧缺 {expected}：{rust_all_names:?}"
        );
    }

    // 1666 号：已声明的 Rust 超集事件——fork 的流式增强（正文增量 + 思考流）。
    // 2.0 TS 腿的会话每轮以 finish_turn 工具调用收尾，这样的回合不产生文本
    // 增量/思考事件（强制打开守卫实测：TS 仅 agent:start / session:title /
    // agent:complete）。声明后从 Rust 侧剥离，跨端只比**共有事件**的名序列与
    // 负载；共有词汇里任何新出现的差异照旧判红。
    const RUST_ONLY_EVENTS: [&str; 4] = [
        "draft:delta",
        "thinking:delta",
        "thinking:end",
        "thinking:start",
    ];
    let rust_common: Vec<(String, Value)> = rust_events
        .iter()
        .filter(|(name, _)| !RUST_ONLY_EVENTS.contains(&name.as_str()))
        .cloned()
        .collect();
    let rust_common_names: Vec<&str> = rust_common.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        rust_common_names, ts_names,
        "共有事件名序列分歧\n  rust: {rust_common_names:?}\n  ts:   {ts_names:?}"
    );
    for ((rust_name, rust_payload), (ts_name, ts_payload)) in
        rust_common.iter().zip(ts_events.iter())
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
            "排序后事件错位：{rust_common:?} vs {ts_events:?}"
        );
        assert_eq!(
            rust_payload, ts_payload,
            "{rust_name} 负载分歧\n  rust: {rust_payload}\n  ts:   {ts_payload}"
        );
    }

    // 引擎 bin 由 EngineBin 守卫在作用域结束时回收。
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
///
/// 1666 号：夹具补足 3 章（此前只有 1 章——split(2)/merge(3+4)/reorder 全打在
/// 不存在的章上，两腿同为非 200，测试把「两腿全等的失败」也记成差异；三个
/// 操作从未真执行过）。
#[tokio::test]
async fn chapter_structure_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture_with_chapters(
        &root,
        &llm,
        &[
            (
                "0002_云涌",
                "云涌",
                "云海翻涌不息，山雨欲来风满楼。少年握紧了拳。",
            ),
            (
                "0003_潮生",
                "潮生",
                "潮声自远海而来，一如多年前的那个夜晚。",
            ),
        ],
    );

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;
    wait_ready(&rust).await;

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

    // ── split：2 号章按 UTF-16 offset 17 切两章（标题行 `# 第2章 云涌\n\n` 占 10，
    //    其后「云海翻涌不息，」占 7——切在首段句读之后）──
    for (base, name) in [(&rust, "rust"), (&ts.base, "ts")] {
        let (status, body) = post_json(
            base,
            "/api/v1/books/b1/chapters/2/split",
            r#"{"offset":17}"#,
        )
        .await;
        if status != 200 {
            diffs.push(format!("[{name}] split status={status} body={body}"));
        }
    }
    let (rust_status, mut rust_body) = get_json(&rust, "/api/v1/books/b1").await;
    let (ts_status, mut ts_body) = get_json(&ts, "/api/v1/books/b1").await;
    apply_declared("/api/v1/books/b1", &mut rust_body, &mut ts_body);
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
    apply_declared("/api/v1/books/b1", &mut rust_body, &mut ts_body);
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
        apply_declared(endpoint, &mut rust_body, &mut ts_body);
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

// ── 1672 号：LLM 未配置对跑 ───────────────────────────────────────────────────
//
// 1669 / 1670 号原在独立二进制 `tests/llm_config_duel.rs`：门禁的台账握手只认
// 本二进制，它从不进台账。并入后随握手对账——两个测试都真跑，不设豁免。

/// 与 TS `LLMConfigurationError("MISSING_API_KEY")` 逐字同句（UI 文案表逐字锚）。
const MISSING_KEY_MESSAGE: &str =
    "Studio LLM API key not set. Open Studio services and save an API key for the selected service.";

/// 未配置引擎的进程内全量路由：启动路由器 = bin 的 env 缺省占位（桌壳直启的
/// 产品路径）。
async fn spawn_unconfigured_rust_engine(root: &Path) -> String {
    use inkos_engine::llm::agent_router::{
        AgentRouter, LlmEndpointConfig, LLM_UNCONFIGURED_BASE_URL,
    };
    let placeholder = std::sync::Arc::new(AgentRouter::new(
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
    spawn_rust_engine_with(root, None, Some(placeholder)).await
}

/// 真实引擎二进制 + 其 base 地址（守卫即 [`EngineBin`]：Drop 回收，断言失败的
/// 展开路径同样回收）。
struct EngineProcess {
    base: String,
    child: EngineBin,
}

/// 起真实 `inkos-engine-server`。`env_llm = None` = 桌壳直启形态：不注入任何
/// `INKOS_LLM_*`——连同从测试进程继承来的一并摘掉，保证是 bin 的缺省占位；
/// `Some(baseUrl)` = 开发 / 冒烟脚本的 env 配置形态。
async fn spawn_engine_bin(root: &Path, label: &str, env_llm: Option<&str>) -> EngineProcess {
    let port = next_sidecar_port().await;
    let err_log = format!("/tmp/duel-llm-config-bin-{label}-err.log");
    let out_log = format!("/tmp/duel-llm-config-bin-{label}-out.log");
    let mut command = engine_bin::engine_command();
    command
        .env("INKOS_PROJECT_ROOT", root)
        .env("INKOS_PORT", port.to_string())
        .env(
            "INKOS_BUILTIN_GENRES_DIR",
            root.join("assets").join("genres"),
        )
        .env(
            "INKOS_BUILTIN_SKILLS_DIR",
            repo_root().join("packages").join("core").join("skills"),
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
    let child = command
        .stdout(std::process::Stdio::from(
            std::fs::File::create(&out_log).unwrap(),
        ))
        .stderr(std::process::Stdio::from(
            std::fs::File::create(&err_log).unwrap(),
        ))
        .spawn()
        .map(EngineBin)
        .expect("inkos-engine-server 起动失败");
    let mut engine = EngineProcess {
        base: format!("http://127.0.0.1:{port}"),
        child,
    };
    wait_child_ready(&mut engine.child, &engine.base, &err_log).await;
    engine
}

/// 1669 / 1670 号：「模型调用**之前**检出的配置问题」双腿同码同文。同一缺 key
/// 项目根下，TS sidecar 真实进程 × 未配置引擎的进程内全量路由，八面同请求对跑
/// ——两腿都必须 400 `LLM_CONFIG_ERROR` 信封，且 `error` 全等。两腿各自先按契约
/// 断言（防「对齐数据=回声室」），再断言全等。
#[tokio::test]
async fn llm_config_error_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write_unconfigured_fixture(&root);

    let rust = spawn_unconfigured_rust_engine(&root).await;
    let ts = spawn_ts_sidecar_ready(&root).await;

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

    // 载荷取能通过 TS 腿各自入参校验的形态（TS 的配置检查在处理器内部、校验之后）。
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
            json!({ "code": "LLM_CONFIG_ERROR", "message": MISSING_KEY_MESSAGE }),
            "[{name}] ts"
        );
        assert_eq!(
            rust_body["error"], ts_body["error"],
            "[{name}] 双腿 error 不等"
        );
    }

    eprintln!(
        "LLM 未配置对跑通过：write-next / audit / agent / 文风分析 / 仿写创建 / 雷达扫描与分析 / 导演方向 八面双腿 400 LLM_CONFIG_ERROR 同码同文"
    );
}

/// 真实引擎二进制的产品路径直证：桌壳直启不注入 `INKOS_LLM_*` → bin 的缺省
/// 端点即未配置占位；未配置项目上 write-next / audit / agent 同步 400，只读面
/// 照常。对照：同一项目根换成 env 配置形态启动，守卫放行（write-next 照常起
/// 后台任务）。进程内对跑用的是手工构造的占位路由器——这里补的是 bin 装配层。
#[tokio::test]
async fn bin_process_unconfigured_engine_answers_400() {
    if !duel_enabled() {
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
        let (status, body) = post_json(&engine.base, path, "{}").await;
        assert_eq!(status, 400, "[{name}] {body}");
        assert_eq!(body["error"]["code"], "LLM_CONFIG_ERROR", "[{name}] {body}");
        assert_eq!(
            body["error"]["message"], MISSING_KEY_MESSAGE,
            "[{name}] {body}"
        );
    }
    let session_id = "1783099000031-d169";
    let (status, body) = post_json(
        &engine.base,
        "/api/v1/sessions",
        &json!({ "sessionId": session_id, "sessionKind": "chat" }).to_string(),
    )
    .await;
    assert_eq!(status, 200, "建会话：{body}");
    let (status, body) = post_json(
        &engine.base,
        "/api/v1/agent",
        &json!({ "instruction": "你好", "sessionId": session_id }).to_string(),
    )
    .await;
    assert_eq!(status, 400, "agent: {body}");
    assert_eq!(
        body["error"]["message"], MISSING_KEY_MESSAGE,
        "agent: {body}"
    );
    // 只读面不受影响。
    let (status, body) = get_json(&engine.base, "/api/v1/books").await;
    assert_eq!(status, 200, "books: {body}");
    drop(engine);

    // 对照：env 配置形态启动——守卫放行。
    let env_engine = spawn_engine_bin(&root, "env", Some("http://127.0.0.1:9/v1")).await;
    let (status, body) = post_json(&env_engine.base, "/api/v1/books/b1/write-next", "{}").await;
    assert_eq!(status, 200, "env 配置的引擎不应被拦：{body}");
    assert_eq!(body["status"], "writing", "{body}");

    eprintln!("真实二进制验收通过：未配置引擎三面 400 LLM_CONFIG_ERROR；env 配置引擎放行");
}

// ── 1672 号：互动叙事换腿的数据根（现状钉）─────────────────────────────────────

/// TS 腿原生写面的播种器（`strangler_duel/seedPlayWorld.mjs`：core 的 PlayStore /
/// PlayDB / syncWorkSourceArtifacts——与 TS 腿同一份写入代码，不手搓夹具）。
/// `args` = 脚本的命令与其余参数（`["create", "<worldId>"]` /
/// `["append-turn", "<worldId>", "<content>"]` / `["turn-sync", "<worldId>"]` /
/// `["live-checksum", "<worldId>"]`）；脚本的参数顺序是「命令 → 项目根 → 其余」。
/// 返回脚本的 stdout。
fn seed_works_world(root: &Path, args: &[&str]) -> String {
    let output = seed_works_world_output(root, args);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        output.status.success(),
        "TS 腿原生世界播种失败：{}\n{}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

/// 同 [`seed_works_world`]，但不断言成功——对跑要断言「Node 腿拒了这次写入」时用
/// （脚本被 PlayStore 拒绝时以 4 退出，stderr 形如 `[seed-play-world] refused <code>`）。
fn seed_works_world_output(root: &Path, args: &[&str]) -> std::process::Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("strangler_duel")
        .join("seedPlayWorld.mjs");
    let (command, rest) = args.split_first().expect("seedPlayWorld：缺命令");
    let output = std::process::Command::new("node")
        .arg(&script)
        .arg(command)
        .arg(root)
        .args(rest)
        .output()
        .expect("node 起动失败（TS 腿原生世界播种）");
    output
}

fn non_empty_lines(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

/// 往转录里直接追加一轮，并把文件的修改时间钉在 `at`——模拟「这一份后来又往前
/// 玩了」：进度钟只看这几个文件的修改时间。
fn append_transcript_line_at(path: &Path, role: &str, content: &str, at: std::time::SystemTime) {
    let line = format!(
        "{}\n",
        json!({ "role": role, "content": content, "timestamp": 1 })
    );
    let mut text = std::fs::read_to_string(path).unwrap();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&line);
    std::fs::write(path, text).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// 把一份世界的四类进度文件的修改时间钉到同一时刻（不在的文件跳过）。
fn pin_progress_clock(world_dir: &Path, at: std::time::SystemTime) {
    for file in [
        "world.json",
        "runs/main/transcript.jsonl",
        "runs/main/state/current.json",
        "runs/main/play-graph.json",
    ] {
        let path = world_dir.join(file);
        if !path.is_file() {
            continue;
        }
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }
}

/// 1672 号起：互动叙事（interactive world）换腿的数据根。存储模型定线为「自动双向
/// 桥接」（1673 号，分三批）——1675 号第二批之后两个方向都已桥接：
///
/// - 「产品引擎创建 → Node 腿读写」见
///   [`interactive_world_engine_created_world_is_shared_duel`]（Node 腿首次触达按需
///   迁移 + 文件图谱导入）。
/// - 「Node 腿创建 → 产品引擎读写」见
///   [`interactive_world_node_created_world_is_shared_duel`]（第一批）。
///
/// 本测试钉的是「两棵树都在」的对账规则：**进度较新的那份为准，打平取 works**。
/// 1673 号之前这里钉的是「产品引擎一律用 `worlds/<id>`、Node 腿 404」。规则翻过来
/// 之后要守住的是 legacy 根较新的那头：显式迁移之后用户接着在产品引擎上玩过的世界，
/// 较新的进度不被旧拷贝顶掉——产品引擎仍用 legacy 根，Node 腿读得到落后副本（与
/// 此前一样）但**拒绝续写**（works 一写就成了较新的那份，产品引擎会换到这份落后
/// 副本上）。两份的对账（旧份归档）留到第三批，这里钉「拒了、且没写」。
#[tokio::test]
async fn interactive_world_storage_roots_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);

    // 产品引擎原生创建：worlds/w-rust/…（写入面 = play.rs；图谱走文件后端）。
    inkos_engine::play::create_world(
        &root,
        &inkos_engine::play::PlayWorldInput {
            id: "w-rust",
            title: "产品引擎世界",
            premise: "镜中图书馆没有出口。",
            world_contract: "",
            visual_contract: "",
            mode: "open",
            language: "zh",
        },
    )
    .await
    .unwrap();
    let rust_run = inkos_engine::play::ensure_run(&root, "w-rust", "main")
        .await
        .unwrap();
    let mut graph = inkos_engine::play_graph::open_play_graph_db(&rust_run).unwrap();
    graph.flush().unwrap();
    drop(graph);
    inkos_engine::play::append_transcript_turn(&root, "w-rust", "main", "user", "产品引擎第一问")
        .await
        .unwrap();
    inkos_engine::play::save_current_state(
        &root,
        "w-rust",
        "main",
        &json!({ "turn": 1, "lastSummary": "产品引擎第一问" }),
    )
    .await
    .unwrap();

    // TS 腿原生创建：works/w-ts/source/…（第一批已桥接，这里只留一条对照）。
    seed_works_world(&root, &["create", "w-ts"]);

    // 文件系统面：两棵树、两种图谱后端。
    assert!(
        root.join("worlds")
            .join("w-rust")
            .join("world.json")
            .is_file(),
        "产品引擎写 worlds/<id>/"
    );
    assert!(
        !root.join("works").join("w-rust").exists(),
        "产品引擎不写 works/"
    );
    assert!(
        rust_run.join("play-graph.json").is_file(),
        "产品引擎新建的运行走文件后端"
    );
    assert!(
        !rust_run.join("play.db").exists(),
        "产品引擎新建的运行不建 play.db"
    );
    assert!(
        root.join("works")
            .join("w-ts")
            .join("source")
            .join("runs")
            .join("main")
            .join("play.db")
            .is_file(),
        "TS 腿新建的运行建 play.db（sqlite）"
    );
    assert!(
        !root.join("worlds").join("w-ts").exists(),
        "TS 腿不写 worlds/"
    );

    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;
    let run_path = |id: &str| format!("/api/v1/play/runs/{id}/main");
    let turns = |body: &Value| body["transcript"].as_array().map(Vec::len).unwrap_or(0);

    // ① 产品引擎创建的世界：自己读得到。
    let (status, body) = get_json(&rust, &run_path("w-rust")).await;
    assert_eq!(
        (status, body["title"].clone(), turns(&body)),
        (200, json!("产品引擎世界"), 1),
        "{body}"
    );
    // 对照：Node 腿创建的世界两腿都读得到（第一批）。
    let (status, body) = get_json(&ts, &run_path("w-ts")).await;
    assert_eq!(
        (status, body["title"].clone(), turns(&body)),
        (200, json!("TS 腿世界"), 1),
        "{body}"
    );
    let (status, body) = get_json(&rust, &run_path("w-ts")).await;
    assert_eq!(
        (status, body["title"].clone(), turns(&body)),
        (200, json!("TS 腿世界"), 1),
        "{body}"
    );
    // 世界在、运行不在：两腿都是 PLAY_RUN_NOT_FOUND。
    let not_found = json!({ "error": { "code": "PLAY_RUN_NOT_FOUND" } });
    let (status, body) = get_json(&rust, "/api/v1/play/runs/w-rust/ghost").await;
    assert_eq!((status, body), (404, not_found.clone()));
    let (status, body) = get_json(&ts, "/api/v1/play/runs/w-ts/ghost").await;
    assert_eq!((status, body), (404, not_found));

    // ② Node 腿触达：按需迁移（拷贝，原目录保留）。两棵树都在，拷贝较新。
    let (status, body) = get_json(&ts, &run_path("w-rust")).await;
    assert_eq!((status, turns(&body)), (200, 1), "{body}");
    assert!(
        root.join("worlds")
            .join("w-rust")
            .join("world.json")
            .is_file(),
        "迁移是拷贝，原目录保留"
    );
    let copied_run = root
        .join("works")
        .join("w-rust")
        .join("source")
        .join("runs")
        .join("main");
    assert!(
        copied_run.join("play-graph.json").is_file(),
        "迁移把产品引擎的运行拷进 works/"
    );
    let transcripts = || {
        (
            non_empty_lines(&rust_run.join("transcript.jsonl")),
            non_empty_lines(&copied_run.join("transcript.jsonl")),
        )
    };

    // ③ legacy 根较新：1675 号之前产品引擎对「两棵树都在」一律写 legacy 根——显式
    //    迁移过的世界就是这样被落下的。照那个形态给 legacy 根补一轮。
    append_transcript_line_at(
        &rust_run.join("transcript.jsonl"),
        "assistant",
        "产品引擎在 legacy 根接上的一轮",
        std::time::SystemTime::now() + Duration::from_secs(60),
    );
    // 产品引擎仍用 legacy 根：较新的进度不被旧拷贝顶掉。
    let (status, body) = get_json(&rust, &run_path("w-rust")).await;
    assert_eq!((status, turns(&body)), (200, 2), "产品引擎：{body}");
    // Node 腿读得到落后副本（与此前一样，照常读）。
    let (status, body) = get_json(&ts, &run_path("w-rust")).await;
    assert_eq!((status, turns(&body)), (200, 1), "Node 腿：{body}");
    // 续写被拒：拒了，且两棵树都没变。
    let refused = seed_works_world_output(&root, &["append-turn", "w-rust", "不该写进去"]);
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(4), "{stderr}");
    assert!(
        stderr.contains("[seed-play-world] refused PLAY_WORLD_DIVERGED"),
        "{stderr}"
    );
    assert_eq!(transcripts(), (2, 1), "拒绝之后两棵树都不该变");

    // ④ 打平取 works：两边进度文件的时刻相同 → 产品引擎读 works 那份。
    let tied = std::time::SystemTime::now() + Duration::from_secs(120);
    pin_progress_clock(&root.join("worlds").join("w-rust"), tied);
    pin_progress_clock(&root.join("works").join("w-rust").join("source"), tied);
    let (status, body) = get_json(&rust, &run_path("w-rust")).await;
    assert_eq!((status, turns(&body)), (200, 1), "打平应取 works：{body}");

    eprintln!(
        "互动叙事数据根对账规则：两棵树都在时进度较新的那份为准——legacy 较新时产品引擎仍用 legacy 根、Node 腿读得到落后副本但拒绝续写；打平取 works"
    );
}

/// 1673 号（存储模型第一批）：Node 回退腿创建的世界，产品引擎读得到、接得上，而且
/// **不分叉**——产品引擎的后续回合写在同一棵树（`works/<id>/source`）上，Node 腿
/// 随后读得到那一轮。图谱层同样双向：Node 腿写进 `play.db` 的实体产品引擎读得出，
/// 产品引擎补进图谱的实体 Node 腿读得出（两腿的 sqlite 表结构差一列
/// `edges.confidence`——产品引擎读写时补上，只读快照有这列才选它）。
///
/// 世界 id 用会话 id 的形态：两腿都是「世界与会话 1:1 绑定，世界 id 即会话 id」，
/// `PlayHud` 请求的正是 `/play/runs/<sessionId>/main`。
#[tokio::test]
async fn interactive_world_node_created_world_is_shared_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);
    let session_id = "1783099000073-hud";
    let path = format!("/api/v1/play/runs/{session_id}/main");
    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;
    let shape = |body: &Value| {
        (
            body["transcript"].as_array().map(Vec::len).unwrap_or(0),
            body["graph"]["entities"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0),
        )
    };

    // PlayHud 的请求形态：世界还没创建时两腿都是 404（HUD 对 404 静默回落到空状态）。
    let not_found = json!({ "error": { "code": "PLAY_WORLD_NOT_FOUND" } });
    let (status, body) = get_json(&rust, &path).await;
    assert_eq!(
        (status, body.clone()),
        (404, not_found.clone()),
        "产品引擎：{body}"
    );
    let (status, body) = get_json(&ts, &path).await;
    assert_eq!((status, body.clone()), (404, not_found), "Node 腿：{body}");

    // Node 腿创建（与其 play_start 同一份写入面，世界 id = 会话 id）+ 图谱里一个实体。
    seed_works_world(&root, &["create", session_id]);
    let node_run = root
        .join("works")
        .join(session_id)
        .join("source")
        .join("runs")
        .join("main");

    // 产品引擎读得到：标题、转录、图谱实体都来自 Node 腿那棵树。
    let (status, body) = get_json(&rust, &path).await;
    assert_eq!(status, 200, "产品引擎读不到 Node 腿创建的世界：{body}");
    assert_eq!(body["title"], "TS 腿世界", "{body}");
    assert_eq!(
        shape(&body),
        (1, 1),
        "产品引擎读不出 Node 腿写进 play.db 的内容：{body}"
    );
    let (status, body) = get_json(&ts, &path).await;
    assert_eq!((status, shape(&body)), (200, (1, 1)), "Node 腿：{body}");

    // 不分叉：产品引擎续写一轮、图谱补一个实体，都落在 Node 腿那棵树上。
    inkos_engine::play::append_transcript_turn(
        &root,
        session_id,
        "main",
        "assistant",
        "产品引擎接上的那一轮",
    )
    .await
    .unwrap();
    let mut graph = inkos_engine::play_graph::open_play_graph_db(&node_run).unwrap();
    graph
        .upsert_entity(&json!({
            "id": "engine-hero", "type": "actor", "label": "引擎来客", "summary": "",
        }))
        .unwrap();
    drop(graph);
    assert!(
        !root.join("worlds").join(session_id).exists(),
        "产品引擎另起了 worlds/<id> 一棵树——分叉"
    );
    let transcript = std::fs::read_to_string(node_run.join("transcript.jsonl")).unwrap();
    assert_eq!(
        transcript
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count(),
        2,
        "{transcript}"
    );

    // 作品清单的版本索引（2.0 的「生成物与版本」）：Node 腿的回合收尾会把本回合改动的
    // 文件登记成当前版本（create 末尾照做了一次）；产品引擎直接写盘不登记。所以此刻
    // 索引停在 Node 腿那一轮，盘上已经是两轮——先把落差钉住。
    let manifest_path = root.join("works").join(session_id).join("work.json");
    let indexed_checksum = || -> String {
        let manifest: Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
        for artifact in manifest["artifacts"].as_array().expect("清单有 artifacts") {
            for revision in artifact["revisions"].as_array().unwrap() {
                if revision["path"] == "source/runs/main/transcript.jsonl"
                    && revision["status"] == "current"
                {
                    return revision["checksum"].as_str().unwrap().to_string();
                }
            }
        }
        panic!("转录文件应已登记为当前版本：{}", manifest["artifacts"]);
    };
    let live = seed_works_world(&root, &["live-checksum", session_id]);
    let live = live
        .lines()
        .find_map(|line| line.strip_prefix("[seed-play-world] live "))
        .unwrap_or_else(|| panic!("live-checksum 输出不成形：{live}"))
        .trim()
        .to_string();
    assert_ne!(
        indexed_checksum(),
        format!("sha256:{live}"),
        "产品引擎写盘不登记版本——索引此刻不该跟上"
    );
    // 回到 Node 腿、下一次回合收尾的同步：不冲突，并把产品引擎写的内容收进新版本。
    seed_works_world(&root, &["turn-sync", session_id]);
    assert_eq!(
        indexed_checksum(),
        format!("sha256:{live}"),
        "回合同步后索引应跟上盘上内容"
    );

    // 两腿都看到 2 轮、2 个实体。
    let (status, body) = get_json(&rust, &path).await;
    assert_eq!((status, shape(&body)), (200, (2, 2)), "产品引擎：{body}");
    let (status, body) = get_json(&ts, &path).await;
    assert_eq!((status, shape(&body)), (200, (2, 2)), "Node 腿：{body}");

    eprintln!(
        "互动叙事第一批：Node 腿创建的世界产品引擎读得到、原地续写不分叉，图谱双向可见（含 edges.confidence 补列）"
    );
}

/// 1675 号（存储模型第二批）：产品引擎创建的世界，Node 回退腿读得到、接得上，而且
/// 此后**不分叉**——Node 腿首次触达把世界拷进 `works/<id>/source`（原目录保留），
/// 产品引擎按「进度较新的那份为准」跟着换到 works，两腿的后续回合都落在同一棵树上。
///
/// 图谱层：产品引擎新建的运行走文件后端 `play-graph.json`，Node 腿只认 sqlite——Node
/// 腿第一次开库时把文件图谱导入。样本走产品引擎的真实写入面
/// （`normalize_play_mutation` → `apply_play_mutation` → 文件后端落盘），不手搓记录：
/// 模型给的别名键、模型多给的键、产品引擎给事件加的 `timeAdvance` 都原样在里面，
/// 导入按列投影才过得去。世界 id 用会话 id 的形态（`PlayHud` 的请求形态）。
#[tokio::test]
async fn interactive_world_engine_created_world_is_shared_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture(&root, &llm);
    let id = "1783099000075-engine";
    let path = format!("/api/v1/play/runs/{id}/main");
    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;
    // (转录轮数, 实体, 关系, 状态槽, 事件)
    let shape = |body: &Value| {
        let count = |key: &str| body["graph"][key].as_array().map(Vec::len).unwrap_or(0);
        (
            body["transcript"].as_array().map(Vec::len).unwrap_or(0),
            count("entities"),
            count("edges"),
            count("stateSlots"),
            count("events"),
        )
    };

    // 产品引擎原生创建 + 第一轮：模型给的一轮变更经真实写入面落到文件后端。
    inkos_engine::play::create_world(
        &root,
        &inkos_engine::play::PlayWorldInput {
            id,
            title: "产品引擎世界",
            premise: "镜中图书馆没有出口。",
            world_contract: "",
            visual_contract: "",
            mode: "open",
            language: "zh",
        },
    )
    .await
    .unwrap();
    let engine_run = inkos_engine::play::ensure_run(&root, id, "main")
        .await
        .unwrap();
    let mutation = inkos_engine::play_parser::normalize_play_mutation(&json!({
        "eventId": "e1", "turn": 1, "actionKind": "look", "summary": "玩家推门",
        "entities": { "upsert": [
            { "id": "engine-hero", "type": "actor", "label": "引擎来客", "summary": "",
              "status": "", "note": "模型多给的键" },
            { "id": "engine-gate", "type": "location", "label": "镜门", "summary": "", "status": "" },
        ] },
        "edges": { "upsert": [
            { "id": "rel-gate", "from": "引擎来客", "relation": "guards", "to": "镜门" },
        ] },
        "stateSlots": { "upsert": [
            { "id": "slot-mood", "ownerEntityId": "engine-hero", "kind": "flag", "label": "心情",
              "value": "警惕", "updatedEventId": "e1" },
        ] },
    }));
    let mut graph = inkos_engine::play_graph::open_play_graph_db(&engine_run).unwrap();
    inkos_engine::play_graph::apply_play_mutation(&mut graph, &mutation, "产品引擎第一问").unwrap();
    graph.flush().unwrap();
    drop(graph);
    inkos_engine::play::append_transcript_turn(&root, id, "main", "user", "产品引擎第一问")
        .await
        .unwrap();
    inkos_engine::play::save_current_state(
        &root,
        id,
        "main",
        &json!({ "turn": 1, "lastSummary": "产品引擎第一问" }),
    )
    .await
    .unwrap();
    // 样本确实是「脏」的：三个键都没有对应的列。
    let file_graph = std::fs::read_to_string(engine_run.join("play-graph.json")).unwrap();
    for (key, why) in [
        ("\"note\"", "模型多给的键"),
        ("\"relation\"", "别名键"),
        ("\"timeAdvance\"", "产品引擎给事件加的键"),
    ] {
        assert!(file_graph.contains(key), "样本应带着{why}：{file_graph}");
    }

    // ① Node 腿读得到：首次触达按需迁移，文件图谱导入成 sqlite 图谱。
    let (status, body) = get_json(&ts, &path).await;
    assert_eq!(status, 200, "Node 腿读不到产品引擎创建的世界：{body}");
    assert_eq!(body["title"], "产品引擎世界", "{body}");
    assert_eq!(
        shape(&body),
        (1, 2, 1, 1, 1),
        "文件图谱没有导入成 sqlite 图谱：{body}"
    );
    let hero = body["graph"]["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entity| entity["id"] == json!("engine-hero"))
        .cloned()
        .expect("导入的实体里有 engine-hero");
    assert!(
        hero.get("note").is_none(),
        "没有对应列的键不该跟进来：{hero}"
    );
    assert_eq!(
        body["graph"]["edges"][0]["fromId"],
        json!("engine-hero"),
        "按 label 解析的端点应原样导入：{}",
        body["graph"]["edges"][0]
    );
    let node_run = root
        .join("works")
        .join(id)
        .join("source")
        .join("runs")
        .join("main");
    assert!(
        node_run.join("play.db").is_file(),
        "Node 腿第一次开库应导入并建库"
    );
    assert!(
        root.join("worlds").join(id).join("world.json").is_file(),
        "迁移是拷贝，原目录保留"
    );
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("works").join(id).join("work.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (
            manifest["profileId"].clone(),
            manifest["metadata"]["legacySource"].clone()
        ),
        (json!("interactive-world"), json!(format!("worlds/{id}")))
    );

    // ② 产品引擎跟着换到 works：续写落在 works，legacy 根那份停在迁移时刻。
    inkos_engine::play::append_transcript_turn(
        &root,
        id,
        "main",
        "assistant",
        "产品引擎接上的一轮",
    )
    .await
    .unwrap();
    assert_eq!(
        (
            non_empty_lines(&node_run.join("transcript.jsonl")),
            non_empty_lines(&engine_run.join("transcript.jsonl")),
        ),
        (2, 1),
        "拷贝较新：产品引擎该写 works 那份，legacy 根那份不动"
    );
    assert!(
        !engine_run.join("play.db").exists(),
        "legacy 根那份不该被动"
    );

    // ③ Node 腿续写一轮、图谱补一个实体：产品引擎读得到。回合收尾的同步此刻有了
    //    清单可落（迁移之前第一步 loadWorkManifest 直接 ENOENT）。
    seed_works_world(&root, &["append-turn", id, "Node 腿接上的一轮"]);
    seed_works_world(&root, &["add-entity", id, "node-hero", "雾港来客"]);
    seed_works_world(&root, &["turn-sync", id]);
    let (status, body) = get_json(&rust, &path).await;
    assert_eq!(
        (status, shape(&body)),
        (200, (3, 3, 1, 1, 1)),
        "产品引擎读不到 Node 腿的续写：{body}"
    );

    // ④ 产品引擎再往图谱补一个实体（此刻解析到 works、库已在 → sqlite 后端）：Node
    //    腿读得到。两腿最终看到同一份。
    let mut graph = inkos_engine::play_graph::open_play_graph_db(
        &inkos_engine::play::run_dir(&root, id, "main").unwrap(),
    )
    .unwrap();
    graph
        .upsert_entity(&json!({
            "id": "late-hero", "type": "actor", "label": "后来的客人", "summary": "",
        }))
        .unwrap();
    drop(graph);
    let (status, body) = get_json(&ts, &path).await;
    assert_eq!(
        (status, shape(&body)),
        (200, (3, 4, 1, 1, 1)),
        "Node 腿：{body}"
    );
    let (status, body) = get_json(&rust, &path).await;
    assert_eq!(
        (status, shape(&body)),
        (200, (3, 4, 1, 1, 1)),
        "产品引擎：{body}"
    );

    eprintln!(
        "互动叙事第二批：产品引擎创建的世界 Node 腿读得到（按需迁移 + 文件图谱导入），此后两腿写同一棵树，图谱双向可见"
    );
}

// ── 1676 号：破坏性操作护栏（R58）─────────────────────────────────────────────

/// 目录下的全部文件：相对路径（POSIX 分隔）→ 字节。
fn tree_files(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    std::fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// `chapters/.trash/` 里唯一的回滚批次目录。
fn only_trash_batch(book_tree: &Path) -> PathBuf {
    let trash = book_tree.join("chapters").join(".trash");
    let batches: Vec<std::ffi::OsString> = std::fs::read_dir(&trash)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(batches.len(), 1, "{batches:?}");
    let name = batches[0].to_string_lossy().into_owned();
    assert!(
        name.starts_with("rollback-") && name.ends_with("-1"),
        "{name}"
    );
    trash.join(&batches[0])
}

/// 把两腿各自带时间戳的批次目录名归一成同一个占位——条目集才比得了。
fn normalize_batch(rel: &str, batch_name: &str) -> String {
    rel.replace(batch_name, "rollback-*")
}

/// R58/1676 号：破坏性操作护栏的双端面——同一份书在两腿上各驳回一次、各删一次，
/// 回收站里的条目集与字节逐项一致。
///
/// 两腿写各的树（`books/b1` 与 `works/b1/source`），所以不是共享存储的对跑；比的是
/// 「同样的书、同样的操作，留下的回收站是不是同一份」。夹具只放两腿恢复逻辑都认的
/// 文件——旧版可选真相文件（`particle_ledger.md` 等）只有产品引擎会在回滚时清掉，
/// 是既有的两腿差异，不在这里比。
#[tokio::test]
async fn destructive_operations_trash_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    // 第 3 章是裸章号文件 `0003.md`：1676 号时这里只能用正典文件名——Node 腿的进度守卫
    // 当时只认 `NNNN_`，这本书会让书架接口整页 500。1678 号修掉后改回来，夹具本身就是证据。
    write_fixture_book(
        &root,
        &[
            ("0002_云涌", "云涌", "江面起了雾。"),
            ("0003", "收束", "渡船靠岸。"),
        ],
    );
    write_fixture_llm(&root, &llm);
    migrate_fixture_to_works(&root);
    let rust_tree = root.join("books").join("b1");
    let ts_tree = root.join("works").join("b1").join("source");
    let seed = |tree: &Path, rel: &str, content: &str| {
        let path = tree.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    };
    for tree in [&rust_tree, &ts_tree] {
        for n in 0..=3 {
            for (file, kind) in [
                ("current_state.md", "state"),
                ("pending_hooks.md", "hooks"),
                ("chapter_summaries.md", "summaries"),
            ] {
                seed(
                    tree,
                    &format!("story/snapshots/{n}/{file}"),
                    &format!("{kind}-{n}"),
                );
            }
            seed(
                tree,
                &format!("story/snapshots/{n}/state/manifest.json"),
                &format!("manifest-{n}"),
            );
        }
        seed(tree, "story/drafts/0002_云涌.md", "draft-2");
        seed(tree, "story/drafts/0003.md", "draft-3");
        for chapter in ["01", "02", "03"] {
            seed(
                tree,
                &format!("story/runtime/chapter-{chapter}.trace.json"),
                &format!("trace-{chapter}"),
            );
        }
        seed(tree, "story/current_state.md", "state-3");
        seed(tree, "story/pending_hooks.md", "hooks-3");
        seed(tree, "story/chapter_summaries.md", "summaries-3");
        seed(tree, "story/state/manifest.json", "manifest-3");
        seed(tree, "story/memory.db", "derived");
    }
    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;
    // ⓪ 书架：索引里列着裸章号的第 3 章，两腿都列得出、章数都对。
    for base in [&rust, &ts.base] {
        let (status, body) = get_json(base, "/api/v1/books").await;
        assert_eq!(status, 200, "{base}：{body}");
        assert_eq!(body["books"][0]["id"], "b1", "{base}：{body}");
        assert_eq!(body["books"][0]["chaptersWritten"], 3, "{base}：{body}");
    }
    // 两棵树本来就有字节不同的文件（产品引擎那棵是夹具手写的紧凑 JSON，Node 腿那棵经
    // 迁移脚本规范化过：章节索引、book.json）。这些只比路径；其余逐字节。
    let rust_before = tree_files(&rust_tree);
    let ts_before = tree_files(&ts_tree);
    let rust_index_before = rust_before["chapters/index.json"].clone();
    let ts_index_before = ts_before["chapters/index.json"].clone();
    // 回滚之前两棵树里字节相同的文件——这些才跨腿逐字节比。
    let comparable_before = |rel: &String| {
        rust_before
            .get(rel)
            .is_some_and(|bytes| Some(bytes) == ts_before.get(rel))
    };

    // ① 驳回第 2 章：两腿同码同体，被废内容各进自己那棵树的回收站批次——同一份。
    let rejected = json!({
        "ok": true, "chapterNumber": 2, "status": "rejected", "rolledBackTo": 1, "discarded": [2, 3],
    });
    let (status, body) = post_json(&rust, "/api/v1/books/b1/chapters/2/reject", "").await;
    assert_eq!((status, body), (200, rejected.clone()), "产品引擎");
    let (status, body) = post_json(&ts, "/api/v1/books/b1/chapters/2/reject", "").await;
    assert_eq!((status, body), (200, rejected.clone()), "Node 腿");
    let rust_batch = only_trash_batch(&rust_tree);
    let ts_batch = only_trash_batch(&ts_tree);
    let mut rust_trash = tree_files(&rust_batch);
    let mut ts_trash = tree_files(&ts_batch);
    assert_eq!(
        rust_trash.remove("chapters/index.json"),
        Some(rust_index_before),
        "产品引擎批次里的索引应等于回滚前"
    );
    assert_eq!(
        ts_trash.remove("chapters/index.json"),
        Some(ts_index_before),
        "Node 腿批次里的索引应等于回滚前"
    );
    let only_rust: Vec<&String> = rust_trash
        .keys()
        .filter(|rel| comparable_before(rel) && !ts_trash.contains_key(*rel))
        .collect();
    let only_ts: Vec<&String> = ts_trash
        .keys()
        .filter(|rel| comparable_before(rel) && !rust_trash.contains_key(*rel))
        .collect();
    let differ: Vec<&String> = rust_trash
        .iter()
        .filter(|(rel, bytes)| {
            comparable_before(rel) && ts_trash.get(*rel).is_some_and(|other| other != *bytes)
        })
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        only_rust.is_empty() && only_ts.is_empty() && differ.is_empty(),
        "回滚批次两腿应逐项逐字节一致——只在产品引擎批次：{only_rust:?}；只在 Node 腿批次：{only_ts:?}；字节不同：{differ:?}"
    );
    // 批次里的每一项都等于回滚前自己那棵树里的那份——包括两棵树本来就不一样的。
    for (rel, bytes) in &rust_trash {
        assert_eq!(
            rust_before.get(rel),
            Some(bytes),
            "产品引擎批次里的 {rel} 应等于回滚前"
        );
    }
    for (rel, bytes) in &ts_trash {
        assert_eq!(
            ts_before.get(rel),
            Some(bytes),
            "Node 腿批次里的 {rel} 应等于回滚前"
        );
    }
    // 1682 号收口：读取面零写——`get_next_chapter_number` 的 ensure 副作用已删
    //（此前这里钉定「批次里恰好是产品引擎书架接口物化的结构化状态四件」，788 号
    // ensure 语义；夹具只播种了 `story/state/manifest.json`）。此后两棵树回滚前
    // 完全可比（无物化差异），全部批次项都进逐字节对账。
    let pre_existing: std::collections::BTreeSet<&String> = rust_trash
        .keys()
        .chain(ts_trash.keys())
        .filter(|rel| !comparable_before(rel))
        .collect();
    assert!(
        pre_existing.is_empty(),
        "读取面零写后回滚之前两棵树不应再有差异：{pre_existing:?}"
    );
    for (rel, bytes) in &rust_trash {
        assert_ne!(rel.as_str(), "story/memory.db", "派生物不进回收站");
        assert!(!bytes.is_empty(), "{rel}");
    }
    assert!(rust_trash.contains_key("chapters/0002_云涌.md"));
    assert!(rust_trash.contains_key("story/snapshots/3/current_state.md"));
    // 原位：章正文只剩第 1 章，状态回到快照 1，派生物直接删。
    for tree in [&rust_tree, &ts_tree] {
        assert!(tree.join("chapters").join("0001_风起.md").is_file());
        assert!(!tree.join("chapters").join("0002_云涌.md").exists());
        assert!(!tree.join("story").join("snapshots").join("3").exists());
        assert_eq!(
            std::fs::read_to_string(tree.join("story").join("current_state.md")).unwrap(),
            "state-1"
        );
        assert!(!tree.join("story").join("memory.db").exists());
    }

    // ② 删书：两腿同码同体，整个书目录进项目级回收站；书从两腿的书架上消失。
    let client = loopback_client();
    let deleted = json!({ "ok": true, "bookId": "b1" });
    let (status, body) = send_json(
        &client,
        &rust,
        reqwest::Method::DELETE,
        "/api/v1/books/b1",
        None,
    )
    .await;
    assert_eq!((status, body), (200, deleted.clone()), "产品引擎");
    let (status, body) = send_json(
        &client,
        &ts,
        reqwest::Method::DELETE,
        "/api/v1/books/b1",
        None,
    )
    .await;
    assert_eq!((status, body), (200, deleted.clone()), "Node 腿");
    assert!(!rust_tree.exists());
    assert!(!root.join("works").join("b1").exists());
    let trash = root.join(".trash").join("books");
    let entries: Vec<std::path::PathBuf> = std::fs::read_dir(&trash)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert_eq!(entries.len(), 2, "{entries:?}");
    // Node 腿删的是整个作品目录（带清单）；产品引擎删的是书目录。认领：有 work.json 的是 Node 腿的。
    let (ts_entry, rust_entry) = {
        let mut ts = None;
        let mut rust = None;
        for entry in &entries {
            if entry.join("work.json").is_file() {
                ts = Some(entry.clone());
            } else {
                rust = Some(entry.clone());
            }
        }
        (ts.unwrap(), rust.unwrap())
    };
    let rust_book = tree_files(&rust_entry);
    let rust_batch_name = rust_batch
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let ts_batch_name = ts_batch.file_name().unwrap().to_string_lossy().into_owned();
    let mut ts_book = std::collections::BTreeMap::new();
    for (rel, bytes) in tree_files(&ts_entry) {
        let Some(stripped) = rel.strip_prefix("source/") else {
            assert_eq!(rel, "work.json", "Node 腿的回收站条目里只该多出清单：{rel}");
            continue;
        };
        ts_book.insert(normalize_batch(stripped, &ts_batch_name), bytes);
    }
    let mut rust_book: std::collections::BTreeMap<String, Vec<u8>> = rust_book
        .into_iter()
        .map(|(rel, bytes)| (normalize_batch(&rel, &rust_batch_name), bytes))
        .collect();
    // 回滚后两腿各自写过的章节索引字节形态不同，只比路径；其余逐字节——除非回滚之前
    // 两棵树里这个文件就不一样（迁移脚本规范化过）。
    let rust_index = rust_book
        .remove("chapters/index.json")
        .expect("产品引擎回收站里有索引");
    let ts_index = ts_book
        .remove("chapters/index.json")
        .expect("Node 腿回收站里有索引");
    let same_before = |rel: &str| {
        let source = rel
            .strip_prefix("chapters/.trash/rollback-*/")
            .unwrap_or(rel);
        rust_before.get(source) == ts_before.get(source)
    };
    // 1682 号收口：读取面零写——产品引擎不再经书架接口物化结构化状态三件，两棵树
    // 回滚前的文件集与字节完全可比（manifest 两腿同是夹具播种值，不再被 bootstrap
    // 改写）；回收站条目集两腿相同，全部逐字节对账（此前这里钉定「只有产品引擎树
    // 才有的三件批次拷贝 + 被改写的 manifest 差异」）。
    let only_rust_tree = |rel: &str| {
        let source = rel
            .strip_prefix("chapters/.trash/rollback-*/")
            .unwrap_or(rel);
        rust_before.contains_key(source) && !ts_before.contains_key(source)
    };
    let mut only_rust = Vec::new();
    let mut divergent_before = Vec::new();
    for (rel, bytes) in &rust_book {
        match ts_book.get(rel) {
            None if only_rust_tree(rel) => only_rust.push(rel.clone()),
            None => panic!("Node 腿回收站缺 {rel}"),
            Some(other) if other == bytes => {}
            Some(_) if same_before(rel) => panic!("{rel} 两腿回收站字节不一致"),
            Some(_) => divergent_before.push(rel.clone()),
        }
    }
    assert!(
        only_rust.is_empty(),
        "读取面零写后不应再有「只有产品引擎树才有的文件」：{only_rust:?}"
    );
    for rel in ts_book.keys() {
        assert!(rust_book.contains_key(rel), "产品引擎回收站缺 {rel}");
    }
    assert_eq!(
        divergent_before,
        ["chapters/.trash/rollback-*/chapters/index.json"]
            .map(String::from)
            .to_vec(),
        "回滚之前两棵树里字节不同的，回收站里也只有它们（批次里的回滚前索引拷贝——夹具紧凑 JSON vs 迁移规范化；manifest 两腿同值）"
    );
    for rel in [
        "book.json",
        "chapters/0001_风起.md",
        "chapters/.trash/rollback-*/chapters/0002_云涌.md",
        "chapters/.trash/rollback-*/story/snapshots/3/current_state.md",
        "story/story_bible.md",
    ] {
        assert!(rust_book.contains_key(rel), "回收站里缺 {rel}");
    }
    for (index, leg) in [(rust_index, "产品引擎"), (ts_index, "Node 腿")] {
        let parsed: Value = serde_json::from_slice(&index).unwrap();
        let numbers: Vec<u64> = parsed
            .as_array()
            .unwrap()
            .iter()
            .map(|meta| meta["number"].as_u64().unwrap())
            .collect();
        assert_eq!(numbers, vec![1], "{leg} 回滚后的索引只该剩第 1 章");
    }

    let (_, body) = get_json(&rust, "/api/v1/books").await;
    assert_eq!(
        body["books"].as_array().map(Vec::len),
        Some(0),
        "产品引擎：{body}"
    );
    let (status, body) = get_json(&ts, "/api/v1/books").await;
    assert_eq!(status, 200);
    assert_eq!(
        body["books"].as_array().map(Vec::len),
        Some(0),
        "Node 腿：{body}"
    );
    assert_eq!(
        body["incompleteWorkIds"],
        json!([]),
        "此前 Node 腿删书只删 source/，清单留下——书挂在「未完成初始化的作品」下"
    );

    eprintln!(
        "破坏性操作护栏：驳回与删书两腿都进回收站，条目集与字节逐项一致；Node 腿删书不再留下孤儿清单"
    );
}

/// 1678 号：章文件的命名形态不影响进度——两腿对同一批旧命名的书给出同一份书架。
///
/// durable 进度此前两腿都只认正典 `NNNN_标题.md`：Node 腿的一致性守卫把索引里列着的
/// 旧命名章当成「缺文件」抛错（书架接口整页 500）；两腿在没有索引时进度都算成 0，下一章
/// 号回到 1（产品引擎的 write-next 写「第 1 章」时，同号旧文件清理会清掉原有的第 1 章）。
///
/// 夹具：带索引的三形态混排一本、不带索引的旧命名一本，连同正典形态的对照书放进两棵树
/// （双根）。Node 腿 sidecar 的就绪探测打的就是书架接口——起得来本身就是断言。
#[tokio::test]
async fn legacy_chapter_forms_progress_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    // 正典形态的对照书 `b1`（一章、带索引）。
    write_fixture_book(&root, &[]);
    let stamp = "2026-08-17T00:00:00.000Z";
    for (id, chapters, indexed) in [
        (
            "legacy-mixed",
            vec![
                ("0001-镜中醒来.md", "一"),
                ("0002.md", "二"),
                ("0003_云涌.md", "三"),
            ],
            true,
        ),
        (
            "legacy-noindex",
            vec![
                ("0001-风起.md", "一"),
                ("0002-云涌.md", "二"),
                ("0003.md", "三"),
            ],
            false,
        ),
    ] {
        let book = root.join("books").join(id);
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::write(
            book.join("book.json"),
            format!(
                r#"{{"id":"{id}","title":"{id}","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"{stamp}","updatedAt":"{stamp}"}}"#
            ),
        )
        .unwrap();
        for (name, content) in &chapters {
            std::fs::write(
                book.join("chapters").join(name),
                format!("# {name}\n\n{content}。"),
            )
            .unwrap();
        }
        if indexed {
            let mut index = String::from("[");
            for (offset, _) in chapters.iter().enumerate() {
                let number = offset + 1;
                if offset > 0 {
                    index.push(',');
                }
                index.push_str(&format!(
                    r#"{{"number":{number},"title":"第{number}章","status":"ready-for-review","wordCount":1,"createdAt":"{stamp}","updatedAt":"{stamp}","auditIssues":[],"lengthWarnings":[]}}"#
                ));
            }
            index.push(']');
            std::fs::write(book.join("chapters").join("index.json"), index).unwrap();
        }
    }
    write_fixture_llm(&root, &llm);
    migrate_fixture_to_works(&root);
    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;

    let expected = json!({ "b1": 1, "legacy-mixed": 3, "legacy-noindex": 3 });
    for base in [&rust, &ts.base] {
        let (status, body) = get_json(base, "/api/v1/books").await;
        assert_eq!(status, 200, "{base}：{body}");
        let written: std::collections::BTreeMap<String, u64> = body["books"]
            .as_array()
            .unwrap()
            .iter()
            .map(|book| {
                (
                    book["id"].as_str().unwrap().to_string(),
                    book["chaptersWritten"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            serde_json::to_value(&written).unwrap(),
            expected,
            "{base}：{body}"
        );
        // 不带索引的旧命名书：下一章号跟着盘上的章走（此前两腿都是 1）。
        let (status, body) = get_json(base, "/api/v1/books/legacy-noindex").await;
        assert_eq!(status, 200, "{base}：{body}");
        assert_eq!(body["nextChapter"], 4, "{base}：{body}");
        assert_eq!(
            body["chapters"].as_array().map(Vec::len),
            Some(3),
            "{base}：{body}"
        );
    }
    // 读面不落盘：两棵树里的不带索引的书还是没有索引。
    for tree in [
        root.join("books").join("legacy-noindex"),
        root.join("works").join("legacy-noindex").join("source"),
    ] {
        assert!(!tree.join("chapters").join("index.json").exists());
    }

    eprintln!(
        "章文件命名形态与进度：带索引的三形态混排、不带索引的旧命名书，两腿书架的章数与下一章号一致；Node 腿书架不再 500"
    );
}

/// 1681 号：导出源一致性对跑——两类「书架可读但导出校验拒绝」的书（缺章书在
/// Node 腿走不到导出：进度守卫对缺文件 fail-closed，书架整页 500——两腿读守卫
/// 的既有不对称，1678 号备案不动），两腿 GET /export 同回 409 +
/// CHAPTER_EXPORT_SOURCE_MISMATCH（修前产品引擎 200 静默导出残本/旧稿，Node 腿
/// 409——同一本书两腿行为分叉）：
/// - dual-form：索引 [1]、盘上 `0001_新稿.md` + `0001-旧稿.md` → duplicateChapterNumbers=[1]
///   （择优是需要用户裁决的状态，拒绝而非静默取正典）
/// - orphan-legacy：索引 [1]、盘上 `0001_风起.md` + 未入索引的旧命名 `0002-残留.md`
///   → unindexedFiles=["0002-残留.md"]
#[tokio::test]
async fn export_source_mismatch_duel() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let llm = spawn_mock_llm().await;
    write_fixture_book(&root, &[]);
    let stamp = "2026-08-17T00:00:00.000Z";
    let single_index = format!(
        r#"[{{"number":1,"title":"风起","status":"ready-for-review","wordCount":10,"createdAt":"{stamp}","updatedAt":"{stamp}","auditIssues":[],"lengthWarnings":[]}}]"#
    );
    let fixture_books = [
        (
            "export-dual-form",
            vec![
                ("0001_新稿.md", "# 第1章\n\n新稿。"),
                ("0001-旧稿.md", "# 第1章\n\n旧稿。"),
            ],
        ),
        (
            "export-orphan-legacy",
            vec![
                ("0001_风起.md", "# 第1章 风起\n\n林动睁开双眼。"),
                ("0002-残留.md", "# 第2章\n\n残留。"),
            ],
        ),
    ];
    for (id, chapters) in fixture_books {
        let book = root.join("books").join(id);
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::write(
            book.join("book.json"),
            format!(
                r#"{{"id":"{id}","title":"{id}","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"{stamp}","updatedAt":"{stamp}"}}"#
            ),
        )
        .unwrap();
        for (name, content) in chapters {
            std::fs::write(book.join("chapters").join(name), content).unwrap();
        }
        std::fs::write(book.join("chapters").join("index.json"), &single_index).unwrap();
    }
    write_fixture_llm(&root, &llm);
    migrate_fixture_to_works(&root);
    let rust = spawn_rust_engine(&root, &llm).await;
    let ts = spawn_ts_sidecar_ready(&root).await;

    let expectations = [
        ("export-dual-form", json!([1]), json!([])),
        ("export-orphan-legacy", json!([]), json!(["0002-残留.md"])),
    ];
    for base in [&rust, &ts.base] {
        // 书架先把两类书读进来（守卫应放行——夹具否则就轮不到导出面）。
        let (status, body) = get_json(base, "/api/v1/books").await;
        assert_eq!(status, 200, "{base}：{body}");
        for (id, duplicates, unindexed) in &expectations {
            let (status, body) =
                get_json(base, &format!("/api/v1/books/{id}/export?format=txt")).await;
            assert_eq!(status, 409, "{base}/{id}：{body}");
            assert_eq!(
                body["code"], "CHAPTER_EXPORT_SOURCE_MISMATCH",
                "{base}/{id}：{body}"
            );
            assert_eq!(
                &body["details"]["duplicateChapterNumbers"], duplicates,
                "{base}/{id}：{body}"
            );
            assert_eq!(
                &body["details"]["unindexedFiles"], unindexed,
                "{base}/{id}：{body}"
            );
        }
    }

    eprintln!(
        "导出源一致性：双形态书两腿同回 409 duplicate=[1]、孤儿旧命名书两腿同回 409 unindexed=[0002-残留.md]（拒绝而非静默残本/旧稿）"
    );
}
