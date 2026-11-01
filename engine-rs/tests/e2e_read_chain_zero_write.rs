//! E2E 读链零写回归锁：785 号同型面巡检的**性质锁**（787 号）。
//!
//! 784 号台账实证：读端点里藏着每读一次的 ensure 落盘（list_books 8.93ms /
//! book_detail 9.96ms，fsync 原子写）——785 号快路径清偿后，「书域 GET 读不落盘」
//! 成为应被守护的性质。本测试以 784 号 bench 同款生产形态装配（router_books +
//! tower oneshot 端到端，非裸内核），打全部书域 GET 端点，前后对整棵项目树
//! 快照（文件路径集 + 长度 + mtime + 目录集）断言逐位不变。
//!
//! 性质依据（787 号普查，见变更记录文档/20261101/787 档）：生产 GET 处理器
//! 20 个逐一追写面——书域 12 端点全部零写（785 快路径覆盖 get_next；eval/export
//! 均为读+内存产物；timeline/review-mode/series-backfill/truth_file 纯读）。
//! **by-contract 例外不在锁面**：`/api/v1/backup/export`（备份 tar 产物本身，
//! 779 号 fsync 硬化）与非书域端点（health/static/debug/run-log/doctor/
//! services/skills/events——静态普查认证）。

use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

use tower::util::ServiceExt;

type WriteNextFuture = futures_util::future::BoxFuture<
    'static,
    Result<inkos_engine::pipeline::write_next::ChapterPipelineResult, String>,
>;

/// 项目树快照：相对路径 → (文件长度, mtime)；目录单列一集。
/// mtime 用 SystemTime 全精度（APFS 纳秒）——fsync 原子写必刷新 mtime，
/// 内容相同但重写过的文件在此现形（785 号 spy 计数锁的真 FS 等价物）。
fn snapshot(root: &Path) -> (BTreeMap<String, (u64, SystemTime)>, BTreeMap<String, ()>) {
    fn walk(
        dir: &Path,
        rel: &str,
        files: &mut BTreeMap<String, (u64, SystemTime)>,
        dirs: &mut BTreeMap<String, ()>,
    ) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
        for entry in entries {
            let entry = entry.expect("dir entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let ty = entry.file_type().expect("file_type");
            if ty.is_dir() {
                dirs.insert(child_rel.clone(), ());
                walk(&dir.join(&name), &child_rel, files, dirs);
            } else {
                let meta = entry.metadata().expect("metadata");
                files.insert(child_rel, (meta.len(), meta.modified().expect("mtime")));
            }
        }
    }
    let mut files = BTreeMap::new();
    let mut dirs = BTreeMap::new();
    walk(root, "", &mut files, &mut dirs);
    (files, dirs)
}

fn seed(root: &Path) {
    let book_dir = root.join("books").join("b1");
    std::fs::create_dir_all(book_dir.join("chapters")).expect("mkdir chapters");
    std::fs::create_dir_all(book_dir.join("story").join("outline")).expect("mkdir story");
    std::fs::create_dir_all(root.join("genres")).expect("mkdir genres");
    std::fs::write(
        root.join("inkos.json"),
        r#"{ "writing": { "reviewMode": "manual" } }"#,
    )
    .expect("inkos.json");
    std::fs::write(
        book_dir.join("book.json"),
        r#"{"id":"b1","title":"镜花缘","platform":"qidian","genre":"xuanhuan","status":"active","targetChapters":12,"chapterWordCount":1600,"language":"zh","createdAt":"2026-11-01T00:00:00.000Z","updatedAt":"2026-11-01T00:00:00.000Z"}"#,
    )
    .expect("book.json");
    let paragraph = "林动握紧手中的祖符玉佩，坊市的喧嚣在身后渐渐远去，山道上的风带着初秋的凉意。他回望这座生活了十六年的小城，目光里没有留恋，只有对前路的笃定。";
    let body: String = (0..24).map(|_| paragraph).collect::<Vec<_>>().join("\n\n");
    let mut index_rows = Vec::new();
    for i in 1..=12 {
        let title = format!("第{i}章 风起于青萍之末");
        std::fs::write(
            book_dir.join("chapters").join(format!("{:04}_{}.md", i, title)),
            format!("# {title}\n\n{body}\n"),
        )
        .expect("chapter file");
        index_rows.push(serde_json::json!({
            "number": i, "title": title, "wordCount": body.replace('\n', "").chars().count(),
            "createdAt": "2026-11-01T00:00:00.000Z", "updatedAt": "2026-11-01T00:00:00.000Z",
            "observations": [], "provenance": "generated",
        }));
    }
    std::fs::write(
        book_dir.join("chapters").join("index.json"),
        serde_json::to_string(&index_rows).expect("index"),
    )
    .expect("index.json");
    std::fs::write(book_dir.join("story").join("brief.md"), "# 简介\n\n试炼之路。\n").expect("truth");
    std::fs::write(
        book_dir.join("story").join("outline").join("story_frame.md"),
        "# 故事框架\n\n三幕。\n",
    )
    .expect("outline");
    std::fs::write(
        root.join("genres").join("cozy.md"),
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../packages/core/genres/cozy.md"
        ))
        .expect("仓库内 cozy.md 应存在"),
    )
    .expect("genre");
}

fn assemble(root: &Path) -> axum::Router {
    let hub = std::sync::Arc::new(inkos_engine::server::sse::BroadcastHub::new());
    let state = std::sync::Arc::new(inkos_engine::state::manager::StateManager::new(
        root.display().to_string().as_str(),
    ));
    let agent_router = std::sync::Arc::new(
        inkos_engine::llm::agent_router::AgentRouter::new(
            inkos_engine::llm::agent_router::LlmEndpointConfig {
                context_window_tokens: 128_000,
                base_url: "http://127.0.0.1:9".into(),
                api_key: "k".into(),
                model: "m".into(),
                max_tokens: 1024,
                extra_headers: Default::default(),
            },
            Default::default(),
        ),
    );
    let runner: inkos_engine::server::write_next_route::WriteNextRunner = std::sync::Arc::new(
        |_state, _book, _target, _quality, _abort, _mode| {
            Box::pin(async { Err("零写锁不触写链".to_string()) }) as WriteNextFuture
        },
    );
    let books = inkos_engine::server::books_routes::BooksRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: agent_router.clone(),
        builtin_genres_dir: root.to_path_buf(),
        revision_gate: inkos_engine::pipeline::merged_audit::RevisionGate::default(),
    };
    let audit = inkos_engine::server::audit_route::AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: agent_router,
        builtin_genres_dir: root.to_path_buf(),
    };
    let write_next = inkos_engine::server::write_next_route::WriteNextRuntime {
        hub: hub.clone(),
        state,
        runner,
        project_root: root.to_path_buf(),
    };
    inkos_engine::server::router_books(
        inkos_engine::server::AppState { version: "zero-write-lock".into() },
        hub,
        write_next,
        audit,
        books,
    )
}

/// 全部书域 GET 端点（787 号普查清单）：每端点先断言 2xx（712 号教训：
/// 404/5xx 链拒绝静默入锁），再断言树快照不变。
///
/// 两相编码（787 号锁首轮即现形的行为面）：**legacy 书（无状态四件）首读会
/// 一次性 ensure 补齐**（784 号台账记为 legacy 可用性所依，load-bearing）——
/// 故先跑 warmup 扫链让 ensure 物化并断言四件确实出现；随后 steady-state
/// 扫链必须逐位零写（785 号快路径性质）。
#[tokio::test]
async fn read_chain_get_endpoints_leave_project_tree_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    seed(&root);

    let endpoints: &[&str] = &[
        "/api/v1/books",
        "/api/v1/books/b1",
        "/api/v1/books/b1/chapters/1",
        "/api/v1/books/b1/chapters/1/workspace",
        "/api/v1/books/b1/truth",
        "/api/v1/books/b1/eval",
        "/api/v1/books/b1/timeline",
        "/api/v1/books/b1/chapter-review-mode",
        "/api/v1/books/b1/series-backfill/existing",
        "/api/v1/books/b1/analytics",
        // export 产物为内存 payload（787 号普查），此锁同时是它的零写证明。
        "/api/v1/books/b1/export",
        "/api/v1/genres",
    ];

    let app = assemble(&root);

    async fn sweep(app: &axum::Router, endpoints: &[&str]) {
        for path in endpoints {
            let request = axum::http::Request::builder()
                .uri(*path)
                .body(axum::body::Body::empty())
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("oneshot");
            let status = response.status();
            assert!(
                status.is_success(),
                "读链自检失败：{path} → {status}（fixture 与路由失配，712 号教训）"
            );
        }
    }

    // 相一：legacy 书 warmup——首读 ensure 物化四件（784 号行为面保持探针）。
    sweep(&app, endpoints).await;
    let state_dir = root.join("books/b1/story/state");
    for file in ["manifest.json", "current_state.json", "hooks.json", "chapter_summaries.json"] {
        assert!(
            state_dir.join(file).exists(),
            "legacy 书首读应 ensure 补齐 {file}（ensure 语义不得被快路径吞掉）"
        );
    }

    // 相二：steady-state 零写——物化后再全量扫链，树快照逐位不变。
    let (files_before, dirs_before) = snapshot(&root);
    assert!(
        files_before.keys().any(|p| p.contains("story/state/")),
        "warmup 后状态四件应在树内"
    );
    sweep(&app, endpoints).await;

    let (files_after, dirs_after) = snapshot(&root);
    assert_eq!(
        files_before, files_after,
        "读链写盘现形：文件路径/长度/mtime 变化（steady-state 读端点不应落盘）"
    );
    assert_eq!(dirs_before, dirs_after, "读链建目录现形（staging/tmp 残留等）");
}
