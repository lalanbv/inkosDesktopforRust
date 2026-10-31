//! 引擎热路径基准（177 号 W-D4）。
//!
//! 覆盖两条真实热路径（选型依据：原方案的 plugin_execute 在本仓不存在，
//! 按实际调用频度重选）：
//! - `sensitive_words/scan_chapter`：`analyze_sensitive_words` 全词表扫描——
//!   审计/全周期审计每章必经；自定义词表为 studio 配置面。
//! - `sse_broadcast/dispatch_*`：`BroadcastHub::broadcast` 同步分发——每个
//!   写面事件（write:start/progress/complete…）一跳，订阅者数为真实变量。
//!
//! 运行：`cargo bench --bench hot_paths`；门禁聚合见
//! `scripts/bench-gate.mjs`（`pnpm bench:gate`）。

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use inkos_engine::agents::sensitive_words::analyze_sensitive_words;
use inkos_engine::utils::language::WritingLanguage;
use inkos_engine::server::sse::BroadcastHub;
use serde_json::json;
use std::sync::Arc;

/// 构造 ~3000 中文字的章节正文：6 段 × 500 字，穿插少量命中词。
fn chapter_content() -> String {
    let paragraph = "林动盘膝而坐，经脉中的灵气如溪流般缓缓运转，丹田处的元气旋涡愈发凝实。\
                     窗外暮色四合，远山如黛，演武场上隐约传来同门的喝声。\
                     他想起白日里长老的一番话，心中警兆顿生——宗门暗流涌动，怕是要变天了。\
                     罢了，兵来将挡水来土掩，且先将这周天运转圆满再说。".repeat(6);
    format!("第41章 风起于青萍之末\n\n{paragraph}\n\n他提到了文革和台独的旧事。", )
}

/// 50 个自定义敏感词（studio 配置面典型规模；2 个真实命中）。
fn custom_words() -> Vec<String> {
    let mut words: Vec<String> = (1..=48).map(|i| format!("自定义黑话{i}")).collect();
    words.push("旧事".to_string());
    words.push("变天".to_string());
    words
}

fn bench_sensitive_words(c: &mut Criterion) {
    let mut group = c.benchmark_group("sensitive_words");
    group.throughput(Throughput::Bytes(chapter_content().len() as u64));
    group.sample_size(20);

    let content = chapter_content();
    let custom = custom_words();
    group.bench_function("scan_chapter", |b| {
        b.iter(|| analyze_sensitive_words(&content, Some(&custom), WritingLanguage::Zh))
    });

    // 无自定义词表的最小形态（纯内置三组）。
    group.bench_function("scan_chapter_builtin_only", |b| {
        b.iter(|| analyze_sensitive_words(&content, None, WritingLanguage::Zh))
    });
    group.finish();
}

fn bench_sse_broadcast(c: &mut Criterion) {
    // broadcast() 是同步 send；订阅者用后台 runtime drain（真实 SSE 消费形态）。
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .build()
        .expect("bench runtime");
    let _guard = rt.enter();

    let mut group = c.benchmark_group("sse_broadcast");
    group.sample_size(30);
    let payload = json!({
        "bookId": "demo-book",
        "chapterNumber": 41,
        "stage": "draft",
        "message": "撰写章节草稿",
        "progress": 0.42
    });

    for subscribers in [1usize, 8, 32] {
        let hub = Arc::new(BroadcastHub::new());
        let mut drainers = Vec::new();
        for _ in 0..subscribers {
            let mut rx = hub.subscribe();
            drainers.push(tokio::spawn(async move {
                while rx.recv().await.is_ok() {}
            }));
        }
        group.bench_with_input(
            BenchmarkId::new("dispatch", subscribers),
            &subscribers,
            |b, _| {
                b.iter(|| hub.broadcast("write:progress", &payload));
            },
        );
        for handle in drainers {
            handle.abort();
        }
    }
    group.finish();
}

/// 196 号：write-next 每章必经的非 LLM 纯 CPU 路径基准补强。
///
/// - `title_dedup/resolve_*`：标题去重每章两次调用（writer 输出 + 持久化
///   装配），长书既有标题 200 个为规模锚点；
/// - `paragraph_scan/detect_shape`：落盘后段落形态检查，每章一次；
/// - `chapter_index/parse`：章节索引反序列化——长书 index 数百条，每章
///   load_chapter_index 至少两次。
fn bench_write_next_hot_paths(c: &mut Criterion) {
    // 标题去重：200 个既有标题；无重复（走 collapse 扫描主路径）与
    // 重复命中（触发重生成候选循环）两形态。
    let titles: Vec<String> = (1..=200)
        .map(|i| format!("第{i}章 风起于青萍之末的{i}种写法"))
        .collect();
    let content = chapter_content();
    let mut group = c.benchmark_group("title_dedup");
    group.sample_size(30);
    group.bench_function("resolve_no_duplicate_200", |b| {
        b.iter(|| {
            inkos_engine::agents::post_write_validator::resolve_duplicate_title(
                "第201章 崭新的标题",
                &titles,
                WritingLanguage::Zh,
                Some(&content),
            )
        })
    });
    group.bench_function("resolve_duplicate_hit_200", |b| {
        b.iter(|| {
            inkos_engine::agents::post_write_validator::resolve_duplicate_title(
                "第5章 风起于青萍之末的5种写法",
                &titles,
                WritingLanguage::Zh,
                Some(&content),
            )
        })
    });
    group.finish();

    // 段落形态扫描（正文同 sensitive_words 的 ~3000 中文章节）。
    let mut group = c.benchmark_group("paragraph_scan");
    group.throughput(Throughput::Bytes(content.len() as u64));
    group.sample_size(30);
    group.bench_function("detect_shape_3000zh", |b| {
        b.iter(|| {
            inkos_engine::agents::post_write_validator::detect_paragraph_shape_warnings(
                &content,
                WritingLanguage::Zh,
            )
        })
    });
    group.finish();

    // 章节索引反序列化（200 条 camelCase ChapterMeta）。
    let index_json = {
        use inkos_engine::models::chapter::{ChapterMeta, ChapterStatus};
        let metas: Vec<ChapterMeta> = (1..=200)
            .map(|i| ChapterMeta {
                number: i,
                title: format!("第{i}章 标题{i}"),
                status: ChapterStatus::Approved,
                word_count: 3000,
                created_at: "2026-09-07T00:00:00.000Z".to_string(),
                updated_at: "2026-09-07T00:00:00.000Z".to_string(),
                audit_issues: Vec::new(),
                length_warnings: Vec::new(),
                review_note: None,
                detection_score: None,
                detection_provider: None,
                detected_at: None,
                length_telemetry: None,
                token_usage: None,
            })
            .collect();
        serde_json::to_string(&metas).expect("serialize index")
    };
    let mut group = c.benchmark_group("chapter_index");
    group.throughput(Throughput::Bytes(index_json.len() as u64));
    group.sample_size(30);
    group.bench_function("parse_200", |b| {
        b.iter(|| {
            let metas: Vec<inkos_engine::models::chapter::ChapterMeta> =
                serde_json::from_str(&index_json).expect("parse index");
            metas
        })
    });
    group.finish();
}

/// 工具注册表分发基准（R38b/544 号施工图 §2.4）：lookup（find：全表 34 项
/// 名称线性扫 + available 门控）+ schema 投影（装配面每请求一次）。
/// 执行体本身为族内既有函数（I/O 主导），不在此计量。
fn bench_tool_registry(c: &mut Criterion) {
    let mut group = c.benchmark_group("tool_registry");
    group.sample_size(30);

    let registry = inkos_engine::interaction::registry::ToolRegistry::global();
    let root = std::path::Path::new(".");
    let ctx = inkos_engine::interaction::registry::ToolCtx::root_only(root);

    // 全表尾部查找（最坏情形：末位命中）+ 未注册名查找（全表扫空）。
    group.bench_function("lookup_last_hit", |b| {
        b.iter(|| registry.find("retrieve_material", &ctx))
    });
    group.bench_function("lookup_miss", |b| b.iter(|| registry.find("nope", &ctx)));
    group.bench_function("schemas_book_session_layer", |b| {
        b.iter(|| registry.schemas(inkos_engine::interaction::registry::ToolScope::BookSession))
    });
    group.finish();
}

/// 627 号：SSE 逐帧解析基准——streaming_client 每 chunk 必经的最热纯 CPU
/// 路径（每 token 数次），此前 12 基准未覆盖。流形态 = 120 个中文 content
/// delta 帧 + finish_reason/usage 同帧 + [DONE]；两个形态：
/// - `parse_full_stream`：整流一次解析（parser.push 一次到位）；
/// - `push_chunked_64b`：64 字节分片喂入（真实网络 chunk 边界不定，跨块
///   缓冲 + drain 路径；分片按字符边界切防劈开 UTF-8）。
fn bench_sse_parse(c: &mut Criterion) {
    let stream = {
        let mut s = String::new();
        for i in 0..120 {
            s.push_str(&format!(
                "data: {{\"id\":\"chatcmpl-{i}\",\"choices\":[{{\"delta\":{{\"content\":\"剑气纵横三千里，一剑霜寒十四州。\"}}}}]}}\n\n"
            ));
        }
        s.push_str(
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1024,\"completion_tokens\":2048,\"total_tokens\":3072}}\n\n",
        );
        s.push_str("data: [DONE]\n\n");
        s
    };
    let chunks: Vec<&str> = {
        let bytes = stream.as_bytes();
        let mut v = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let mut j = (i + 64).min(bytes.len());
            while j < bytes.len() && !stream.is_char_boundary(j) {
                j += 1;
            }
            v.push(&stream[i..j]);
            i = j;
        }
        v
    };
    let mut group = c.benchmark_group("sse_parse");
    group.throughput(Throughput::Bytes(stream.len() as u64));
    group.sample_size(30);
    group.bench_function("parse_full_stream", |b| {
        b.iter(|| {
            std::hint::black_box(inkos_engine::llm::sse_parser::parse_sse_stream(
                std::hint::black_box(&stream),
            ))
        })
    });
    group.bench_function("push_chunked_64b", |b| {
        b.iter(|| {
            let mut parser = inkos_engine::llm::sse_parser::SseStreamParser::new();
            for chunk in &chunks {
                std::hint::black_box(parser.push(chunk));
            }
            std::hint::black_box(parser.finish());
        })
    });
    group.finish();
}

const TIMINGS: [&str; 5] = ["immediate", "near-term", "mid-arc", "slow-burn", "endgame"];

fn bench_story_markdown_render(c: &mut Criterion) {
    // 719 号 0GC：台账渲染热路径（每章 settle 调用）——100 条钩子的
    // render_hook_snapshot，全字段形态（timing/depends_on/core/half_life/promoted/kind）。
    let hooks: Vec<inkos_engine::models::runtime_state::HookRecord> = (1..=100)
        .map(|i| {
            serde_json::from_value(json!({
                "hookId": format!("H{i:02}"),
                "startChapter": i,
                "type": if i % 3 == 0 { "main" } else { "sub" },
                "status": if i % 4 == 0 { "progressing" } else { "open" },
                "lastAdvancedChapter": i % 20,
                "expectedPayoff": format!("第{}0章", i % 9 + 1),
                "payoffTiming": TIMINGS[i % 5],
                "dependsOn": if i % 5 == 0 { vec![format!("H{:02}", i - 1)] } else { vec![] },
                "paysOffInArc": if i % 2 == 0 { "一卷" } else { "" },
                "coreHook": i % 7 == 0,
                "halfLifeChapters": if i % 6 == 0 { 5 } else { 0 },
                "promoted": i % 2 == 0,
                "notes": format!("第{i}条伏笔种子——长线叙事的承重桩，需要持续追踪其推进节奏与兑现时机。"),
                "kind": if i % 3 == 0 { "promise" } else { "suspense" },
            }))
            .expect("hook seed 反序列化失败")
        })
        .collect();
    // 720 号 0GC 第三选点：settle 投影族全貌排查（每章 settle 落盘三投影）。
    let proj_hooks_val = serde_json::json!({ "hooks": hooks });
    let proj_hooks: inkos_engine::models::runtime_state::HooksState =
        serde_json::from_value(proj_hooks_val).expect("hooks state seed");
    let summary_rows: Vec<inkos_engine::models::runtime_state::ChapterSummaryRow> = (1..=100)
        .map(|i| serde_json::from_value(json!({
            "chapter": i,
            "title": format!("第{i}章 风起"),
            "characters": "林动，苏檀",
            "events": format!("第{i}场冲突与推进"),
            "stateChanges": "修为精进",
            "hookActivity": format!("H{:02} 推进", i % 20),
            "mood": "紧张",
            "chapterType": "冲突",
        })).expect("summary row seed"))
        .collect();
    let summaries_state = inkos_engine::models::runtime_state::ChapterSummariesState {
        rows: summary_rows,
    };
    let facts: Vec<inkos_engine::models::runtime_state::CurrentStateFact> = (1..=100)
        .map(|i| serde_json::from_value(json!({
            "subject": format!("实体{i}"),
            "predicate": "持有",
            "object": format!("信物{i}"),
            "validFromChapter": i,
            "validUntilChapter": null,
            "sourceChapter": i,
        })).expect("fact seed"))
        .collect();
    let current_state = inkos_engine::models::runtime_state::CurrentStateState {
        chapter: 100,
        facts,
    };

    let mut group = c.benchmark_group("story_markdown_render");
    group.sample_size(30);
    group.bench_function("render_hook_snapshot_100", |b| {
        b.iter(|| {
            inkos_engine::utils::story_markdown::render_hook_snapshot(
                &hooks,
                WritingLanguage::Zh,
            )
        })
    });
    // 719 号分段定位：settle 结算链全貌（resolve→normalize→infer 推导与渲染
    // 分离计时——正则预编译改动的收益归因）。
    group.bench_function("resolve_timing_x100", |b| {
        b.iter(|| {
            for hook in std::iter::once(&hooks[0]).cycle().take(100) {
                std::hint::black_box(inkos_engine::utils::hook_lifecycle::resolve_hook_payoff_timing(
                    hook.payoff_timing.as_ref().map(|t| match t {
                        inkos_engine::models::runtime_state::HookPayoffTiming::Immediate => "immediate",
                        inkos_engine::models::runtime_state::HookPayoffTiming::NearTerm => "near-term",
                        inkos_engine::models::runtime_state::HookPayoffTiming::MidArc => "mid-arc",
                        inkos_engine::models::runtime_state::HookPayoffTiming::SlowBurn => "slow-burn",
                        inkos_engine::models::runtime_state::HookPayoffTiming::Endgame => "endgame",
                    }),
                    Some(&hook.expected_payoff),
                    Some(&hook.notes),
                ));
            }
        })
    });
    group.bench_function("projections_hooks_100", |b| {
        b.iter(|| {
            inkos_engine::state::projections::render_hooks_projection(
                &proj_hooks,
                WritingLanguage::Zh,
                Some(100),
            )
        })
    });
    group.bench_function("projections_summaries_100", |b| {
        b.iter(|| {
            inkos_engine::state::projections::render_chapter_summaries_projection(
                &summaries_state,
                WritingLanguage::Zh,
            )
        })
    });
    group.bench_function("projections_current_state_100", |b| {
        b.iter(|| {
            inkos_engine::state::projections::render_current_state_projection(
                &current_state,
                WritingLanguage::Zh,
            )
        })
    });
    group.finish();
}

fn bench_export_artifact(c: &mut Criterion) {
    // 724 号 0GC 第四选点：导出链基线（build_export_artifact epub=零依赖
    // stored-zip，12 章 × 3000 字中文正文；回归闸资产——无热点预期）。
    let rt = tokio::runtime::Runtime::new().expect("tokio rt");
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    // 718 号双栈布局 Rust 侧形态：StateManager.book_dir = root/books/{id}
    // （TS 正典为 works/{id}/source——bench 按 Rust 引擎读取布局播种）。
    let source_dir = root.join("books").join("b1");
    std::fs::create_dir_all(source_dir.join("chapters")).expect("mkdir");
    std::fs::write(
        source_dir.join("book.json"),
        r#"{"id":"b1","title":"镜花缘","platform":"qidian","genre":"xuanhuan","status":"active","targetChapters":12,"chapterWordCount":3000,"language":"zh","createdAt":"2026-10-16T00:00:00.000Z","updatedAt":"2026-10-16T00:00:00.000Z"}"#,
    )
    .expect("book.json");
    let paragraph = "林动握紧手中的祖符玉佩，坊市的喧嚣在身后渐渐远去，山道上的风带着初秋的凉意。他回望这座生活了十六年的小城，目光里没有留恋，只有对前路的笃定。";
    let body: String = (0..46).map(|_| paragraph).collect::<Vec<_>>().join("\n\n");
    let mut index_rows = Vec::new();
    for i in 1..=12 {
        let title = format!("第{i}章 风起于青萍之末");
        std::fs::write(
            source_dir.join("chapters").join(format!("{:04}_{}.md", i, title)),
            format!("# {}\n\n{}\n", title, body),
        )
        .expect("chapter file");
        index_rows.push(serde_json::json!({
            "number": i, "title": title, "wordCount": body.replace('\n', "").chars().count(),
            "createdAt": "2026-10-16T00:00:00.000Z", "updatedAt": "2026-10-16T00:00:00.000Z",
            "observations": [], "provenance": "generated",
        }));
    }
    std::fs::write(
        source_dir.join("chapters").join("index.json"),
        serde_json::to_string(&index_rows).expect("index"),
    )
    .expect("index.json");
    let state = inkos_engine::state::manager::StateManager::new(root);

    let mut group = c.benchmark_group("export_artifact");
    group.sample_size(30);
    group.bench_function("build_epub_12x3000", |b| {
        b.iter(|| {
            rt.block_on(async {
                std::hint::black_box(
                    inkos_engine::interaction::export_artifact::build_export_artifact(
                        &state,
                        "b1",
                        inkos_engine::interaction::export_artifact::ExportFormat::parse(Some("epub")),
                        false,
                        None,
                    )
                    .await
                    .expect("export"),
                )
            })
        })
    });
    group.finish();
}

fn bench_backup_export(c: &mut Criterion) {
    // 725 号 0GC 第五选点：backup 导出链基线（staging 复制 + tar + gzip
    // level 6；12 章 × ~1600 字书稿 + inkos.json + .inkos 结构；回归闸资产）。
    let rt = tokio::runtime::Runtime::new().expect("tokio rt");
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    let book_dir = root.join("books").join("b1");
    std::fs::create_dir_all(book_dir.join("chapters")).expect("mkdir");
    std::fs::write(
        book_dir.join("book.json"),
        r#"{"id":"b1","title":"镜花缘","platform":"qidian","genre":"xuanhuan","status":"active","targetChapters":12,"chapterWordCount":3000,"language":"zh","createdAt":"2026-10-16T00:00:00.000Z","updatedAt":"2026-10-16T00:00:00.000Z"}"#,
    )
    .expect("book.json");
    let paragraph = "坊市的喧嚣在身后渐渐远去，山道上的风带着初秋的凉意，祖符玉佩在他掌心微微发烫。";
    let body: String = (0..24).map(|_| paragraph).collect::<Vec<_>>().join("\n\n");
    let mut index_rows = Vec::new();
    for i in 1..=12 {
        let title = format!("第{i}章 山道");
        std::fs::write(
            book_dir.join("chapters").join(format!("{:04}_{}.md", i, title)),
            format!("# {}\n\n{}\n", title, body),
        )
        .expect("chapter");
        index_rows.push(serde_json::json!({
            "number": i, "title": title, "wordCount": 1600,
            "createdAt": "2026-10-16T00:00:00.000Z", "updatedAt": "2026-10-16T00:00:00.000Z",
            "observations": [], "provenance": "generated",
        }));
    }
    std::fs::write(book_dir.join("chapters").join("index.json"), serde_json::to_string(&index_rows).expect("index")).expect("write");
    std::fs::write(
        root.join("inkos.json"),
        r#"{"name":"probe","version":"0.1.0","language":"zh"}"#,
    )
    .expect("inkos.json");
    std::fs::create_dir_all(root.join(".inkos")).expect("inkos dir");

    let mut group = c.benchmark_group("backup_export");
    group.sample_size(30);
    group.bench_function("run_export_12ch", |b| {
        b.iter(|| {
            rt.block_on(async {
                std::hint::black_box(
                    inkos_engine::server::backup_routes::run_export(&root, false, false)
                        .expect("backup export"),
                )
            })
        })
    });
    group.finish();
}

/// 784 号：API 读链基线（757 号演进路线 #5「读链性能基线台账防 0GC 收益回吐」）。
///
/// 7 类热读端点走**生产形态全量路由**（`router_books`：axum 路由+提取器+
/// handler+响应序列化的端到端读链，非裸内核），tower `oneshot` 进程内打请求。
/// 场景=studio UI 实际加载频率：Dashboard 首屏（list）、写作页（detail）、
/// 单章（read_chapter）、编辑器聚合（workspace）、真相面板（truth）、
/// 流派面板（genres）、分析面板（analytics）。
/// 播种口径=既有 bench 同款（12 章 × ~1600 字 + index.json；双栈布局
/// books/ 正典形态）；流派画像用真仓 cozy.md（避免自造 frontmatter 解析
/// 失败伪象）。**首轮自检 200**（712 号教训：不许把 404/5xx 链静默测进基线）。
/// 写链 runner 为 no-op（读链不触 write-next；误触即 Err 现形）。
fn bench_read_chain(c: &mut Criterion) {
    use tower::util::ServiceExt;

    type WriteNextFuture = futures_util::future::BoxFuture<
        'static,
        Result<inkos_engine::pipeline::write_next::ChapterPipelineResult, String>,
    >;

    let rt = tokio::runtime::Runtime::new().expect("tokio rt");
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();

    // ── 播种（复用 export/backup bench 口径）──
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
        r#"{"id":"b1","title":"镜花缘","platform":"qidian","genre":"xuanhuan","status":"active","targetChapters":12,"chapterWordCount":1600,"language":"zh","createdAt":"2026-10-31T00:00:00.000Z","updatedAt":"2026-10-31T00:00:00.000Z"}"#,
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
        index_rows.push(json!({
            "number": i, "title": title, "wordCount": body.replace('\n', "").chars().count(),
            "createdAt": "2026-10-31T00:00:00.000Z", "updatedAt": "2026-10-31T00:00:00.000Z",
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

    // ── 生产形态 runtime 装配 ──
    let hub = Arc::new(BroadcastHub::new());
    let state = Arc::new(inkos_engine::state::manager::StateManager::new(
        root.display().to_string().as_str(),
    ));
    let agent_router = Arc::new(inkos_engine::llm::agent_router::AgentRouter::new(
        inkos_engine::llm::agent_router::LlmEndpointConfig {
            context_window_tokens: 128_000,
            base_url: "http://127.0.0.1:9".into(),
            api_key: "k".into(),
            model: "m".into(),
            max_tokens: 1024,
            extra_headers: Default::default(),
        },
        Default::default(),
    ));
    let runner: inkos_engine::server::write_next_route::WriteNextRunner = Arc::new(
        |_state, _book, _target, _quality, _abort, _mode| {
            Box::pin(async { Err("read-chain bench 不触写链".to_string()) }) as WriteNextFuture
        },
    );
    let books = inkos_engine::server::books_routes::BooksRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: agent_router.clone(),
        builtin_genres_dir: root.clone(),
        revision_gate: inkos_engine::pipeline::merged_audit::RevisionGate::default(),
    };
    let audit = inkos_engine::server::audit_route::AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: agent_router.clone(),
        builtin_genres_dir: root.clone(),
    };
    let write_next = inkos_engine::server::write_next_route::WriteNextRuntime {
        hub: hub.clone(),
        state: state.clone(),
        runner,
        project_root: root.clone(),
    };
    let app = inkos_engine::server::router_books(
        inkos_engine::server::AppState { version: "bench".into() },
        hub,
        write_next,
        audit,
        books,
    );

    let scenarios: &[(&str, &str)] = &[
        ("list_books", "/api/v1/books"),
        ("book_detail", "/api/v1/books/b1"),
        ("read_chapter", "/api/v1/books/b1/chapters/1"),
        ("chapter_workspace", "/api/v1/books/b1/chapters/1/workspace"),
        ("truth_list", "/api/v1/books/b1/truth"),
        ("list_genres", "/api/v1/genres"),
        ("analytics", "/api/v1/books/b1/analytics"),
    ];

    // 首轮自检：每场景必须 200——fixture 与路由失配时拒绝静默入基线。
    for (name, path) in scenarios {
        let request = axum::http::Request::builder()
            .uri(*path)
            .body(axum::body::Body::empty())
            .expect("request");
        let response = rt
            .block_on(app.clone().oneshot(request))
            .expect("oneshot");
        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "read-chain 自检失败：{name} {path}（fixture 与路由失配）"
        );
    }

    let mut group = c.benchmark_group("read_chain");
    for (name, path) in scenarios {
        let path = path.to_string();
        group.bench_function(*name, |b| {
            b.iter(|| {
                rt.block_on(async {
                    let request = axum::http::Request::builder()
                        .uri(path.as_str())
                        .body(axum::body::Body::empty())
                        .expect("request");
                    let response = app.clone().oneshot(request).await.expect("oneshot");
                    std::hint::black_box(response.status());
                })
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_sensitive_words,
    bench_sse_broadcast,
    bench_write_next_hot_paths,
    bench_tool_registry,
    bench_sse_parse,
    bench_story_markdown_render,
    bench_export_artifact,
    bench_backup_export,
    bench_read_chain
);
criterion_main!(benches);
