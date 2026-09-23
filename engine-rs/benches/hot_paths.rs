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

criterion_group!(
    benches,
    bench_sensitive_words,
    bench_sse_broadcast,
    bench_write_next_hot_paths,
    bench_tool_registry
);
criterion_main!(benches);
