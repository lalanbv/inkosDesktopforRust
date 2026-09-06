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

criterion_group!(benches, bench_sensitive_words, bench_sse_broadcast);
criterion_main!(benches);
