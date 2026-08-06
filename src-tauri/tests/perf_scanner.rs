//! 性能回归测试——量化热路径，防止性能退化（商业级性能保护）
//!
//! 不用 criterion（避免重编译依赖），用 std::time + 断言性能上限。
//! 上限留足余量（CI 机器波动、debug 构建），定位"数量级回归"而非微优化。

use inkos_desktop::project::ProjectScanner;
use std::fs;
use std::time::Instant;
use tempfile::TempDir;

/// 在临时目录下造 N 个 Node.js 项目（package.json）
fn seed_projects(root: &std::path::Path, n: usize) {
    for i in 0..n {
        let dir = root.join(format!("proj-{i}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), format!(r#"{{"name":"proj-{i}"}}"#)).unwrap();
    }
}

#[tokio::test]
async fn scan_50_projects_completes_fast() {
    // 设计目标（Phase4 设计文档）：<1s / 1000 目录。50 项目应远快。
    // debug 构建比 release 慢，留余量到 800ms（数量级保护，非微基准）。
    let temp = TempDir::new().unwrap();
    seed_projects(temp.path(), 50);

    let scanner = ProjectScanner::new();
    let start = Instant::now();
    let projects = scanner.scan(temp.path()).await.unwrap();
    let elapsed = start.elapsed();

    assert_eq!(projects.len(), 50, "应发现全部 50 个项目");
    assert!(
        elapsed.as_millis() < 800,
        "扫描 50 项目应 <800ms（debug 余量），实际 {:?}",
        elapsed
    );
}

#[tokio::test]
async fn scan_skips_excluded_dirs_no_perf_hit() {
    // node_modules 等排除目录不应拖慢扫描（排除在 walkdir filter_entry 阶段，
    // 不进入子树）。造一个含大 node_modules 的项目，验证扫描仍快。
    let temp = TempDir::new().unwrap();
    let proj = temp.path().join("app");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("package.json"), r#"{"name":"app"}"#).unwrap();

    // 造 200 个 node_modules 子文件（应被跳过，不计入扫描）
    let nm = proj.join("node_modules");
    fs::create_dir_all(&nm).unwrap();
    for i in 0..200 {
        fs::write(nm.join(format!("dep-{i}.json")), "{}").unwrap();
    }

    let scanner = ProjectScanner::new();
    let start = Instant::now();
    let projects = scanner.scan(temp.path()).await.unwrap();
    let elapsed = start.elapsed();

    // 只发现 1 个项目（node_modules 内的 .json 不算项目根）
    assert_eq!(projects.len(), 1, "node_modules 应被排除");
    assert!(
        elapsed.as_millis() < 500,
        "排除目录不应拖慢扫描，实际 {:?}",
        elapsed
    );
}
