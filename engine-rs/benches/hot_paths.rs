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
use inkos_engine::server::sse::BroadcastHub;
use inkos_engine::utils::language::WritingLanguage;
use serde_json::json;
use std::sync::Arc;

/// 构造 ~3000 中文字的章节正文：6 段 × 500 字，穿插少量命中词。
fn chapter_content() -> String {
    let paragraph = "林动盘膝而坐，经脉中的灵气如溪流般缓缓运转，丹田处的元气旋涡愈发凝实。\
                     窗外暮色四合，远山如黛，演武场上隐约传来同门的喝声。\
                     他想起白日里长老的一番话，心中警兆顿生——宗门暗流涌动，怕是要变天了。\
                     罢了，兵来将挡水来土掩，且先将这周天运转圆满再说。"
        .repeat(6);
    format!("第41章 风起于青萍之末\n\n{paragraph}\n\n他提到了文革和台独的旧事。",)
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
        iter_budgeted(b, "sensitive_words/scan_chapter", || {
            analyze_sensitive_words(&content, Some(&custom), WritingLanguage::Zh)
        })
    });

    // 无自定义词表的最小形态（纯内置三组）。
    group.bench_function("scan_chapter_builtin_only", |b| {
        iter_budgeted(b, "sensitive_words/scan_chapter_builtin_only", || {
            analyze_sensitive_words(&content, None, WritingLanguage::Zh)
        })
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
    let dispatch = |hub: &BroadcastHub| hub.broadcast("write:progress", &payload);

    for subscribers in [1usize, 8, 32] {
        // 分配计量在**没有消费任务运行**时做：消费任务在自己的 worker 线程上唤醒
        // 也会分配，全局探针分不开。订阅者只保留接收端（broadcast 通道不因无人
        // 接收而阻塞），计量的就是分发本身。
        {
            let id = format!("sse_broadcast/dispatch/{subscribers}");
            let hub = BroadcastHub::new();
            let _receivers: Vec<_> = (0..subscribers).map(|_| hub.subscribe()).collect();
            assert_budget(&id, measure_min(|| dispatch(&hub)));
        }

        let hub = Arc::new(BroadcastHub::new());
        let mut drainers = Vec::new();
        for _ in 0..subscribers {
            let mut rx = hub.subscribe();
            drainers.push(tokio::spawn(
                async move { while rx.recv().await.is_ok() {} },
            ));
        }
        group.bench_with_input(
            BenchmarkId::new("dispatch", subscribers),
            &subscribers,
            |b, _| {
                b.iter(|| dispatch(&hub));
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
        iter_budgeted(b, "title_dedup/resolve_no_duplicate_200", || {
            inkos_engine::agents::post_write_validator::resolve_duplicate_title(
                "第201章 崭新的标题",
                &titles,
                WritingLanguage::Zh,
                Some(&content),
            )
        })
    });
    group.bench_function("resolve_duplicate_hit_200", |b| {
        iter_budgeted(b, "title_dedup/resolve_duplicate_hit_200", || {
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
        iter_budgeted(b, "paragraph_scan/detect_shape_3000zh", || {
            inkos_engine::agents::post_write_validator::detect_paragraph_shape_warnings(
                &content,
                WritingLanguage::Zh,
            )
        })
    });
    group.finish();

    // AI 痕迹扫描（写后审计链 merged_audit 每章必经；1586 号入闸——EN 路径
    // count_word 每词一次整文 to_lowercase 的分配面此前无观测）。
    let mut group = c.benchmark_group("ai_tells");
    let zh_body = content.clone();
    // EN 夹具：~450 词 / ~3KB，段落化+少量 hedge/transition 命中形态。
    let en_body = {
        let sentence = "The old man walked slowly through the market, perhaps looking \
                        for something he had lost years ago among the crowded stalls.";
        let mut body = String::new();
        for paragraph in 0..12 {
            body.push_str(&format!("Chapter {paragraph} begins here.\n\n"));
            for _ in 0..6 {
                body.push_str(sentence);
                body.push(' ');
                body.push_str("However, the crowd seemed indifferent to his search.\n");
            }
            body.push('\n');
        }
        body
    };
    group.throughput(Throughput::Bytes(zh_body.len() as u64));
    group.sample_size(30);
    group.bench_function("scan_3000zh", |b| {
        iter_budgeted(b, "ai_tells/scan_3000zh", || {
            inkos_engine::agents::ai_tells::analyze_ai_tells(&zh_body, WritingLanguage::Zh)
        })
    });
    group.throughput(Throughput::Bytes(en_body.len() as u64));
    group.bench_function("scan_en", |b| {
        iter_budgeted(b, "ai_tells/scan_en", || {
            inkos_engine::agents::ai_tells::analyze_ai_tells(&en_body, WritingLanguage::En)
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
                volume_id: None,
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
        iter_budgeted(b, "chapter_index/parse_200", || {
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
        iter_budgeted(b, "tool_registry/lookup_last_hit", || {
            registry.find("retrieve_material", &ctx)
        })
    });
    group.bench_function("lookup_miss", |b| {
        iter_budgeted(b, "tool_registry/lookup_miss", || {
            registry.find("nope", &ctx)
        })
    });
    group.bench_function("schemas_book_session_layer", |b| {
        iter_budgeted(b, "tool_registry/schemas_book_session_layer", || {
            registry.schemas(inkos_engine::interaction::registry::ToolScope::BookSession)
        })
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
        iter_budgeted(b, "sse_parse/parse_full_stream", || {
            std::hint::black_box(inkos_engine::llm::sse_parser::parse_sse_stream(
                std::hint::black_box(&stream),
            ))
        })
    });
    group.bench_function("push_chunked_64b", |b| {
        iter_budgeted(b, "sse_parse/push_chunked_64b", || {
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
        .map(|i| {
            serde_json::from_value(json!({
                "chapter": i,
                "title": format!("第{i}章 风起"),
                "characters": "林动，苏檀",
                "events": format!("第{i}场冲突与推进"),
                "stateChanges": "修为精进",
                "hookActivity": format!("H{:02} 推进", i % 20),
                "mood": "紧张",
                "chapterType": "冲突",
            }))
            .expect("summary row seed")
        })
        .collect();
    let summaries_state =
        inkos_engine::models::runtime_state::ChapterSummariesState { rows: summary_rows };
    let facts: Vec<inkos_engine::models::runtime_state::CurrentStateFact> = (1..=100)
        .map(|i| {
            serde_json::from_value(json!({
                "subject": format!("实体{i}"),
                "predicate": "持有",
                "object": format!("信物{i}"),
                "validFromChapter": i,
                "validUntilChapter": null,
                "sourceChapter": i,
            }))
            .expect("fact seed")
        })
        .collect();
    let current_state = inkos_engine::models::runtime_state::CurrentStateState {
        chapter: 100,
        facts,
    };

    let mut group = c.benchmark_group("story_markdown_render");
    group.sample_size(30);
    group.bench_function("render_hook_snapshot_100", |b| {
        iter_budgeted(b, "story_markdown_render/render_hook_snapshot_100", || {
            inkos_engine::utils::story_markdown::render_hook_snapshot(&hooks, WritingLanguage::Zh)
        })
    });
    // 719 号分段定位：settle 结算链全貌（resolve→normalize→infer 推导与渲染
    // 分离计时——正则预编译改动的收益归因）。
    group.bench_function("resolve_timing_x100", |b| {
        iter_budgeted(b, "story_markdown_render/resolve_timing_x100", || {
            for hook in std::iter::once(&hooks[0]).cycle().take(100) {
                std::hint::black_box(
                    inkos_engine::utils::hook_lifecycle::resolve_hook_payoff_timing(
                        hook.payoff_timing.as_ref().map(|t| match t {
                            inkos_engine::models::runtime_state::HookPayoffTiming::Immediate => {
                                "immediate"
                            }
                            inkos_engine::models::runtime_state::HookPayoffTiming::NearTerm => {
                                "near-term"
                            }
                            inkos_engine::models::runtime_state::HookPayoffTiming::MidArc => {
                                "mid-arc"
                            }
                            inkos_engine::models::runtime_state::HookPayoffTiming::SlowBurn => {
                                "slow-burn"
                            }
                            inkos_engine::models::runtime_state::HookPayoffTiming::Endgame => {
                                "endgame"
                            }
                        }),
                        Some(&hook.expected_payoff),
                        Some(&hook.notes),
                    ),
                );
            }
        })
    });
    group.bench_function("projections_hooks_100", |b| {
        iter_budgeted(b, "story_markdown_render/projections_hooks_100", || {
            inkos_engine::state::projections::render_hooks_projection(
                &proj_hooks,
                WritingLanguage::Zh,
                Some(100),
            )
        })
    });
    group.bench_function("projections_summaries_100", |b| {
        iter_budgeted(b, "story_markdown_render/projections_summaries_100", || {
            inkos_engine::state::projections::render_chapter_summaries_projection(
                &summaries_state,
                WritingLanguage::Zh,
            )
        })
    });
    group.bench_function("projections_current_state_100", |b| {
        iter_budgeted(
            b,
            "story_markdown_render/projections_current_state_100",
            || {
                inkos_engine::state::projections::render_current_state_projection(
                    &current_state,
                    WritingLanguage::Zh,
                )
            },
        )
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
            source_dir
                .join("chapters")
                .join(format!("{:04}_{}.md", i, title)),
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
        iter_budgeted(b, "export_artifact/build_epub_12x3000", || {
            rt.block_on(async {
                std::hint::black_box(
                    inkos_engine::interaction::export_artifact::build_export_artifact(
                        &state,
                        "b1",
                        inkos_engine::interaction::export_artifact::ExportFormat::parse(Some(
                            "epub",
                        )),
                        false,
                        None,
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
    let paragraph =
        "坊市的喧嚣在身后渐渐远去，山道上的风带着初秋的凉意，祖符玉佩在他掌心微微发烫。";
    let body: String = (0..24).map(|_| paragraph).collect::<Vec<_>>().join("\n\n");
    let mut index_rows = Vec::new();
    for i in 1..=12 {
        let title = format!("第{i}章 山道");
        std::fs::write(
            book_dir
                .join("chapters")
                .join(format!("{:04}_{}.md", i, title)),
            format!("# {}\n\n{}\n", title, body),
        )
        .expect("chapter");
        index_rows.push(serde_json::json!({
            "number": i, "title": title, "wordCount": 1600,
            "createdAt": "2026-10-16T00:00:00.000Z", "updatedAt": "2026-10-16T00:00:00.000Z",
            "observations": [], "provenance": "generated",
        }));
    }
    std::fs::write(
        book_dir.join("chapters").join("index.json"),
        serde_json::to_string(&index_rows).expect("index"),
    )
    .expect("write");
    std::fs::write(
        root.join("inkos.json"),
        r#"{"name":"probe","version":"0.1.0","language":"zh"}"#,
    )
    .expect("inkos.json");
    std::fs::create_dir_all(root.join(".inkos")).expect("inkos dir");

    let mut group = c.benchmark_group("backup_export");
    group.sample_size(30);
    group.bench_function("run_export_12ch", |b| {
        iter_budgeted(b, "backup_export/run_export_12ch", || {
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
            book_dir
                .join("chapters")
                .join(format!("{:04}_{}.md", i, title)),
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
    std::fs::write(
        book_dir.join("story").join("brief.md"),
        "# 简介\n\n试炼之路。\n",
    )
    .expect("truth");
    std::fs::write(
        book_dir
            .join("story")
            .join("outline")
            .join("story_frame.md"),
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
    let runner: inkos_engine::server::write_next_route::WriteNextRunner =
        Arc::new(|_state, _book, _target, _quality, _abort, _mode| {
            Box::pin(async { Err("read-chain bench 不触写链".to_string()) }) as WriteNextFuture
        });
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
        inkos_engine::server::AppState {
            version: "bench".into(),
        },
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
        let response = rt.block_on(app.clone().oneshot(request)).expect("oneshot");
        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "read-chain 自检失败：{name} {path}（fixture 与路由失配）"
        );
    }

    let mut group = c.benchmark_group("read_chain");
    for (name, path) in scenarios {
        let path = path.to_string();
        let id = format!("read_chain/{name}");
        group.bench_function(*name, |b| {
            iter_budgeted(b, &id, || {
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

fn bench_book_search(c: &mut Criterion) {
    // R47/1640 号：全局搜索 chapters 轴双路径（≥300 章大书锚点，验收
    // P95<200ms 以热路径 mean 为锚入 bench-gate 门禁；冷路径=首次检索
    // 全量分词，一次性质、不入阈值面）。
    // - `cold_300ch`：:memory: 全量 replace_scope + search（300 章全分词）
    // - `warm_300ch`：磁盘投影 db 已建，iter = 打开 + replace_scope
    //   （content_hash 命中 → 不分词、不落库）+ search——生产形态
    //   （.inkos/chapter-search.db）。分配预算锁在此：此前「全跳」只跳过落库，
    //   分词照做（单次 25 万次分配），与冷路径只差一次写库。
    let rt = tokio::runtime::Runtime::new().expect("tokio rt");
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    let book_dir = root.join("books").join("b1");
    std::fs::create_dir_all(book_dir.join("chapters")).expect("mkdir chapters");
    std::fs::create_dir_all(root.join(".inkos")).expect("mkdir .inkos");

    let paragraph = "林动握紧手中的祖符玉佩，坊市的喧嚣在身后渐渐远去，山道上的风带着初秋的凉意。他回望这座生活了十六年的小城，目光里没有留恋，只有对前路的笃定。";
    let body: String = (0..9).map(|_| paragraph).collect::<Vec<_>>().join("\n\n");
    let mut documents = Vec::with_capacity(300);
    let mut index_rows = Vec::with_capacity(300);
    for i in 1..=300u32 {
        let title = format!("第{i}章 风起于青萍之末");
        let content = format!("# {title}\n\n{body}\n");
        std::fs::write(
            book_dir.join("chapters").join(format!("{i:04}_风起.md")),
            &content,
        )
        .expect("chapter file");
        index_rows.push(json!({
            "number": i, "title": title, "wordCount": body.replace('\n', "").chars().count(),
            "createdAt": "2026-10-31T00:00:00.000Z", "updatedAt": "2026-10-31T00:00:00.000Z",
            "observations": [], "provenance": "generated",
        }));
        documents.push(inkos_engine::utils::local_search::SearchDocument {
            id: format!("chapter:{i}"),
            scope: "chapters:b1".to_string(),
            kind: "chapter".to_string(),
            source: format!("{i:04}_风起.md"),
            title: title.clone(),
            body: content,
            metadata: None,
        });
    }
    std::fs::write(
        book_dir.join("chapters").join("index.json"),
        serde_json::to_string(&index_rows).expect("index"),
    )
    .expect("index.json");

    let mut group = c.benchmark_group("book_search");
    group.sample_size(20);
    let scope = "chapters:b1".to_string();
    let docs = documents;
    group.bench_function("cold_300ch", |b| {
        iter_budgeted(b, "book_search/cold_300ch", || {
            let index = inkos_engine::utils::local_search::LocalSearchIndex::new(":memory:")
                .expect("memory index");
            index.replace_scope(&scope, &docs).expect("replace");
            let hits = index.search(
                "祖符玉佩",
                &inkos_engine::utils::local_search::SearchOptions {
                    scope: &scope,
                    kinds: &[],
                    limit: 20,
                },
            );
            index.close();
            hits
        })
    });

    // 热路径：预建磁盘投影（一次全量分词在 setup 内，不入采样）。
    let db_path = root.join(".inkos").join("chapter-search.db");
    {
        let index =
            inkos_engine::utils::local_search::LocalSearchIndex::new(&db_path.to_string_lossy())
                .expect("disk index");
        index.replace_scope(&scope, &docs).expect("warm replace");
        index.close();
    }
    let db_path_for_iter = db_path.clone();
    group.bench_function("warm_300ch", |b| {
        iter_budgeted(b, "book_search/warm_300ch", || {
            let index = inkos_engine::utils::local_search::LocalSearchIndex::new(
                &db_path_for_iter.to_string_lossy(),
            )
            .expect("disk index");
            index.replace_scope(&scope, &docs).expect("noop replace");
            let hits = index.search(
                "祖符玉佩",
                &inkos_engine::utils::local_search::SearchOptions {
                    scope: &scope,
                    kinds: &[],
                    limit: 20,
                },
            );
            index.close();
            hits
        })
    });
    let _ = rt;
    group.finish();
}

// ── 会话读面投影（1671 号）────────────────────────────────────────
//
// `GET /sessions/:id`、会话列表（逐会话 derive）与每轮聊天的缓存指纹都要走
// 「active 链过滤 → 已提交过滤 → 消息投影」。此前不在基准套件里——1666/1671
// 号逐项对齐 2.0 投影族时只有算法层面的论证，没有量化数字。

/// 分配计数探针（仅本组的分配预算锁用）：全局分配器外面包一层「**布防时才
/// 计数**」的探针——未布防时每次分配只多一次 Relaxed 原子读，对同文件其余
/// 基准的测量条件无可见影响。
mod alloc_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    static ARMED: AtomicBool = AtomicBool::new(false);
    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static BYTES: AtomicUsize = AtomicUsize::new(0);

    pub struct Probe;

    // SAFETY: 全部转发给系统分配器，只在布防期间累加计数；计数不参与分配决策。
    unsafe impl GlobalAlloc for Probe {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count(layout.size());
            System.alloc(layout)
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            System.dealloc(ptr, layout)
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            count(new_size);
            System.realloc(ptr, layout, new_size)
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            count(layout.size());
            System.alloc_zeroed(layout)
        }
    }

    fn count(bytes: usize) {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(bytes, Ordering::Relaxed);
        }
    }

    /// 运行 `body` 一次，返回其结果与期间的分配次数 / 字节数（单线程使用）。
    pub fn measure<R>(body: impl FnOnce() -> R) -> (R, usize, usize) {
        ALLOCS.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
        ARMED.store(true, Ordering::Relaxed);
        let result = body();
        ARMED.store(false, Ordering::Relaxed);
        (
            result,
            ALLOCS.load(Ordering::Relaxed),
            BYTES.load(Ordering::Relaxed),
        )
    }
}

#[global_allocator]
static ALLOCATOR: alloc_probe::Probe = alloc_probe::Probe;

// ── 分配预算锁（全部基准工作负载）────────────────────────────────────
//
// 1671 号只给会话读面一条路径上了分配预算锁；其余 32 条基准只有时间基线——那些
// 路径上的「高性能 / 0GC」此前没有数字。这里给每条基准的工作负载登记「单次调用
// 的分配次数预算」：`iter_budgeted` 先计量并断言预算，再交给 criterion 计时。
//
// - 计量取 5 次里的最小值：全局探针分不开别的线程的分配，I/O 面在 tokio 阻塞池
//   / 驱动线程上的零星分配用最小值滤掉（纯 CPU 面每次都相同；两遍独立计量 32 条
//   逐项相同）；
// - 预算 = 实测值 + 余量（纯 CPU 扫描面 10%，带文件 / 数据库 / 压缩的 I/O 面 25%）；
//   判「应当零分配」的路径预算就是 0（段落形态扫描、工具表查找 ×2、钩子兑现时机
//   推导），判「本就该分配」的路径锁的是回退不是清零；
// - 量化一上来就抓到一处：`book_search/warm_300ch`（磁盘投影已建的热路径）单次
//   25 万次分配 / 32MB，与冷路径只差一次落库——`replace_scope` 先对全部文档分词
//   再比内容哈希，而哈希并不依赖分词结果。改成「先比哈希、只对变化文档分词」后
//   1,679 次 / 213KB（Node 腿同形，两腿同改）；
// - 新增基准必须同时登记时间基线与这里的分配预算（没登记即 panic）。

/// 各基准工作负载的单次分配预算（次）。每行注释 = 实测值（两次独立计量逐项相同）
/// 与判定：零分配 / 纯 CPU（+10%）/ I/O（+25%）。
const ALLOC_BUDGETS: &[(&str, usize)] = &[
    ("sensitive_words/scan_chapter", 35), //                     32  纯 CPU：命中结果
    ("sensitive_words/scan_chapter_builtin_only", 19), //        18  纯 CPU
    ("sse_broadcast/dispatch/1", 2), //                             2  纯 CPU：载荷序列化 + 入队
    ("sse_broadcast/dispatch/8", 2), //                             2
    ("sse_broadcast/dispatch/32", 2), //                            2
    ("title_dedup/resolve_no_duplicate_200", 640), //             582  纯 CPU
    ("title_dedup/resolve_duplicate_hit_200", 1_388), //        1_262  纯 CPU
    ("paragraph_scan/detect_shape_3000zh", 0), //                   0  **应当零分配**
    ("ai_tells/scan_3000zh", 68),    //                               62  纯 CPU
    ("ai_tells/scan_en", 400),       //                                 364  纯 CPU：逐词小写
    ("chapter_index/parse_200", 667), //                          607  纯 CPU：反序列化
    ("tool_registry/lookup_last_hit", 0), //                        0  **应当零分配**
    ("tool_registry/lookup_miss", 0), //                            0  **应当零分配**
    ("tool_registry/schemas_book_session_layer", 158), //         144  纯 CPU：schema 投影
    ("sse_parse/parse_full_stream", 408), //                      371  纯 CPU：事件构造
    ("sse_parse/push_chunked_64b", 536), //                       488  纯 CPU：跨块缓冲
    ("story_markdown_render/render_hook_snapshot_100", 392), //   357  纯 CPU：渲染
    ("story_markdown_render/resolve_timing_x100", 0), //           0  **应当零分配**
    ("story_markdown_render/projections_hooks_100", 4_584), //  4_168  纯 CPU：投影渲染
    ("story_markdown_render/projections_summaries_100", 2_321), // 2_110
    ("story_markdown_render/projections_current_state_100", 1_412), // 1_284
    ("export_artifact/build_epub_12x3000", 5_088), //           4_071  I/O：zip + 文件
    ("backup_export/run_export_12ch", 1_162), //                   930  I/O：tar + gzip
    ("read_chain/list_books", 812),  //                             650  I/O：全量路由读链
    ("read_chain/book_detail", 1_311), //                        1_049
    ("read_chain/read_chapter", 188), //                          151
    ("read_chain/chapter_workspace", 472), //                     378
    ("read_chain/truth_list", 197),  //                            158
    ("read_chain/list_genres", 1_091), //                         873
    ("read_chain/analytics", 477),   //                             382
    // 冷路径：整本书分词建投影，本就该分配。
    ("book_search/cold_300ch", 316_708), //                  253_367  I/O + 分词
    // 热路径：内容未变 → 不再分词（此前 254_279 次 / 32MB，与冷路径只差一次落库；
    // 「先比哈希、只对变化文档分词」之后 1_679 次 / 213KB）。
    ("book_search/warm_300ch", 2_098), //                       1_679  I/O：哈希 + 检索
];

/// 工作负载跑 5 次取最小分配（次数，字节数）。
fn measure_min<R>(mut body: impl FnMut() -> R) -> (usize, usize) {
    let mut best = (usize::MAX, usize::MAX);
    for _ in 0..5 {
        let (_, allocs, bytes) = alloc_probe::measure(&mut body);
        if allocs < best.0 || (allocs == best.0 && bytes < best.1) {
            best = (allocs, bytes);
        }
    }
    best
}

fn assert_budget(id: &str, (allocs, bytes): (usize, usize)) {
    let (_, budget) = ALLOC_BUDGETS
        .iter()
        .find(|(name, _)| *name == id)
        .unwrap_or_else(|| {
            panic!(
                "基准 {id} 没有登记分配预算（ALLOC_BUDGETS）——新增基准须同时登记时间基线与分配预算"
            )
        });
    println!("ALLOC-BUDGET {id} allocs={allocs} bytes={bytes} budget={budget}");
    assert!(
        allocs <= *budget,
        "分配越预算：{id} 单次 {allocs} 次 > 预算 {budget}"
    );
}

/// `b.iter` 的带预算形态：先单次计量工作负载的分配并断言预算，再交给 criterion 计时。
fn iter_budgeted<R>(b: &mut criterion::Bencher, id: &str, mut body: impl FnMut() -> R) {
    assert_budget(id, measure_min(&mut body));
    b.iter(body);
}

/// transcript 行构建器：seq 递增、`parentSeq` 恒为上一个事件（分支后由调用方
/// 改指分支目标），与真实追加助手的戳记一致。
#[derive(Default)]
struct TranscriptBuilder {
    seq: u64,
    parent: Option<u64>,
    lines: Vec<String>,
}

impl TranscriptBuilder {
    fn push(&mut self, members: Vec<(&str, serde_json::Value)>) {
        self.seq += 1;
        let mut event = serde_json::Map::new();
        event.insert("version".into(), json!(1));
        event.insert("sessionId".into(), json!("bench-read-face"));
        event.insert("seq".into(), json!(self.seq));
        event.insert("timestamp".into(), json!(self.seq));
        event.insert("parentSeq".into(), json!(self.parent));
        for (key, value) in members {
            event.insert(key.into(), value);
        }
        // 真实 transcript 里每条消息事件都带 uuid——读边界对缺 uuid 的消息行是
        // 静默丢弃的（夹具自检实录：1133 行只读出 667）。
        if event.get("type").and_then(serde_json::Value::as_str) == Some("message") {
            let seq = self.seq;
            event
                .entry("uuid")
                .or_insert_with(|| json!(format!("u-{seq}")));
            event.entry("parentUuid").or_insert(serde_json::Value::Null);
        }
        self.parent = Some(self.seq);
        self.lines
            .push(serde_json::Value::Object(event).to_string());
    }
}

/// 200 轮混合回合的会话 transcript：三种形态轮转——纯文本 / 工具调用（产品
/// 引擎写入形态）/ 2.0 `finish_turn` 收尾协议（工具调用与结果仅模型可见、答复在
/// `display.completion.message`）。第 101 轮之后追加一条 `branch_moved` 回到第
/// 100 轮末尾——第 101 轮的事件留在文件里但不在 active 链上。
fn session_read_face_transcript() -> String {
    let mut transcript = TranscriptBuilder::default();
    let mut request_id;
    let mut commit_of_100: u64 = 0;
    // 回合一轮：`kind` = plain / tool / finish_turn。
    for turn in 1..=200u32 {
        let kind = match turn % 3 {
            0 => "finish_turn",
            1 => "plain",
            _ => "tool",
        };
        request_id = format!("req-{turn}");
        let input = format!("第 {turn} 轮提问");
        transcript.push(vec![
            ("type", json!("request_started")),
            ("requestId", json!(request_id)),
            ("input", json!(input)),
        ]);
        if kind == "finish_turn" {
            transcript.push(vec![
                ("type", json!("request_delivery")),
                ("requestId", json!(request_id)),
                (
                    "state",
                    json!({ "version": 1, "authorRequest": input, "declared": false, "steps": [], "receipts": [] }),
                ),
            ]);
        }
        transcript.push(vec![
            ("type", json!("message")),
            ("requestId", json!(request_id)),
            ("uuid", json!(format!("u-{}", transcript.seq))),
            ("parentUuid", serde_json::Value::Null),
            ("role", json!("user")),
            (
                "message",
                json!({ "role": "user", "content": if kind == "plain" { json!(input) } else { json!([{ "type": "text", "text": input }]) }, "timestamp": transcript.seq }),
            ),
        ]);
        let call_id = format!("call-{turn}");
        match kind {
            "plain" => {
                transcript.push(vec![
                    ("type", json!("message")),
                    ("role", json!("assistant")),
                    ("requestId", json!(request_id)),
                    (
                        "message",
                        json!({ "role": "assistant", "content": [{ "type": "text", "text": format!("第 {turn} 轮答复。") }] }),
                    ),
                ]);
            }
            "tool" => {
                transcript.push(vec![
                    ("type", json!("message")),
                    ("role", json!("assistant")),
                    ("requestId", json!(request_id)),
                    (
                        "message",
                        json!({ "role": "assistant", "content": [{ "type": "toolCall", "id": call_id, "name": "read", "arguments": { "path": "a.md" } }] }),
                    ),
                ]);
                transcript.push(vec![
                    ("type", json!("message")),
                    ("role", json!("toolResult")),
                    ("toolCallId", json!(call_id)),
                    ("requestId", json!(request_id)),
                    (
                        "message",
                        json!({ "role": "toolResult", "toolCallId": call_id, "content": [{ "type": "text", "text": "字".repeat(600) }], "isError": false, "timestamp": transcript.seq }),
                    ),
                ]);
                transcript.push(vec![
                    ("type", json!("message")),
                    ("role", json!("assistant")),
                    ("requestId", json!(request_id)),
                    (
                        "message",
                        json!({ "role": "assistant", "content": [{ "type": "text", "text": format!("第 {turn} 轮答复。") }] }),
                    ),
                ]);
            }
            _ => {
                transcript.push(vec![
                    ("type", json!("message")),
                    ("role", json!("assistant")),
                    ("visibility", json!("model")),
                    ("toolCallId", json!(call_id)),
                    ("requestId", json!(request_id)),
                    (
                        "message",
                        json!({ "role": "assistant", "content": [{ "type": "toolCall", "id": call_id, "name": "finish_turn", "arguments": { "status": "answered", "message": format!("第 {turn} 轮答复。") } }], "stopReason": "toolUse" }),
                    ),
                ]);
                transcript.push(vec![
                    ("type", json!("message")),
                    ("role", json!("toolResult")),
                    ("visibility", json!("model")),
                    ("toolCallId", json!(call_id)),
                    ("requestId", json!(request_id)),
                    (
                        "message",
                        json!({ "role": "toolResult", "toolCallId": call_id, "toolName": "finish_turn", "content": [{ "type": "text", "text": format!("第 {turn} 轮答复。") }], "isError": false }),
                    ),
                ]);
                transcript.push(vec![
                    ("type", json!("message")),
                    ("role", json!("assistant")),
                    ("requestId", json!(request_id)),
                    (
                        "display",
                        json!({ "completion": { "status": "answered", "message": format!("第 {turn} 轮答复。") } }),
                    ),
                    (
                        "message",
                        json!({ "role": "assistant", "content": [], "stopReason": "stop" }),
                    ),
                ]);
            }
        }
        transcript.push(vec![
            ("type", json!("request_committed")),
            ("requestId", json!(request_id)),
        ]);
        if turn == 100 {
            commit_of_100 = transcript.seq;
        }
        if turn == 101 {
            // 回到第 100 轮末尾：branch_moved 自身的父是移动前的 head（第 101 轮
            // 提交点），其后续写入以**分支目标**为父——与真实追加助手的戳记
            // 一致（分支元事件不入链）。
            transcript.push(vec![
                ("type", json!("branch_moved")),
                ("fromSeq", json!(transcript.seq)),
                ("toSeq", json!(commit_of_100)),
            ]);
            transcript.parent = Some(commit_of_100);
        }
    }
    format!("{}\n", transcript.lines.join("\n"))
}

/// 200 轮读面投影的分配预算（次）。同一输入的分配次数是确定的，实测 5,416
/// （1671 号清理前 21,553：整条 active 链深克隆 12,847 次、工具卡挂载时深克隆
/// 两份、调用配对表每条复制四个 String）。预算只留不到 2% 余量——每条消息多出
/// 一次字符串复制（+398）即越线；它锁的是「只为喂函数而复制」这类回退，不是
/// 输出构造本身（每条消息约 13 次：对象节点、键名、正文、工具卡）。
const SESSION_READ_FACE_ALLOC_BUDGET: usize = 5_500;

fn bench_session_read_face(c: &mut Criterion) {
    use inkos_engine::interaction::session_restore::{
        committed_message_events, message_events_to_interaction_messages,
    };
    use inkos_engine::interaction::session_transcript::{
        active_chain_events, read_transcript_events, transcript_path,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".inkos").join("sessions")).unwrap();
    std::fs::write(
        transcript_path(root, "bench-read-face"),
        session_read_face_transcript(),
    )
    .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let events = rt.block_on(read_transcript_events(root, "bench-read-face"));
    // 夹具自检：66 个「纯文本 4 + 工具 6 + 收尾协议 7」轮转 + 第 199 轮（纯文本
    // 4）+ 第 200 轮（工具 6）+ 一条 branch_moved。
    assert_eq!(events.len(), 17 * 66 + 4 + 6 + 1, "夹具事件数不符");
    let project = || {
        // 与 `derive_book_session_from_transcript` 的三步同形（active 链 → 已提交 → 投影）。
        let chain = active_chain_events(&events);
        let committed = committed_message_events(&chain, None);
        message_events_to_interaction_messages(&committed)
    };
    let (chain, chain_allocs, chain_bytes) = alloc_probe::measure(|| active_chain_events(&events));
    let (committed, committed_allocs, committed_bytes) =
        alloc_probe::measure(|| committed_message_events(&chain, None));
    let (_, project_allocs, project_bytes) =
        alloc_probe::measure(|| message_events_to_interaction_messages(&committed));
    let (messages, allocs, bytes) = alloc_probe::measure(project);
    // 第 101 轮在弃用分支上：读面 = 其余 199 轮 × (user + assistant)。
    assert_eq!(messages.len(), 199 * 2, "夹具读面形态不符");
    println!(
        "session_read_face: events={} messages={} allocs={} bytes={}",
        events.len(),
        messages.len(),
        allocs,
        bytes
    );
    println!(
        "session_read_face 分解: 链过滤 allocs={chain_allocs} bytes={chain_bytes} / 已提交过滤 allocs={committed_allocs} bytes={committed_bytes} / 消息投影 allocs={project_allocs} bytes={project_bytes}"
    );
    assert!(
        allocs <= SESSION_READ_FACE_ALLOC_BUDGET,
        "会话读面投影分配越预算：{allocs} > {SESSION_READ_FACE_ALLOC_BUDGET}（200 轮）"
    );

    let mut group = c.benchmark_group("session_read_face");
    group.throughput(Throughput::Elements(messages.len() as u64));
    group.bench_function(BenchmarkId::new("project", "200_turns"), |b| {
        b.iter(&project)
    });
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
    bench_read_chain,
    bench_book_search,
    bench_session_read_face
);
criterion_main!(benches);
