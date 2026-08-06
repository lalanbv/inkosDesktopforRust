//! scanner 精细性能 bench（criterion）——量化项目扫描吞吐
//!
//! 运行：cargo bench --bench scanner
//! 产出：target/criterion/report/index.html（统计：均值/P95/方差 + 回归对比）
//!
//! 与 tests/perf_scanner.rs 互补：perf_scanner 是断言式回归门，
//! criterion 是统计分析工具（CI 性能监控用）。

use criterion::{criterion_group, criterion_main, Criterion};
use inkos_desktop::project::ProjectScanner;
use std::fs;
use tempfile::TempDir;

fn seed_projects(root: &std::path::Path, n: usize) {
    for i in 0..n {
        let dir = root.join(format!("proj-{i}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), format!(r#"{{"name":"proj-{i}"}}"#)).unwrap();
    }
}

fn bench_scan(c: &mut Criterion) {
    let temp = TempDir::new().unwrap();
    seed_projects(temp.path(), 50);

    let mut group = c.benchmark_group("scanner");
    group.sample_size(20); // 扫描有 I/O，样本数降到 20 加速（默认 100）
    group.bench_function("scan_50_projects", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
            let scanner = ProjectScanner::new();
            let _ = scanner.scan(temp.path()).await.unwrap();
        });
    });
    group.finish();
}

criterion_group!(benches, bench_scan);
criterion_main!(benches);
