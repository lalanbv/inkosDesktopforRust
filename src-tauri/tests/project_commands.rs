//! 项目命令层集成测试
//!
//! `tauri::State` 没有公开构造函数，命令函数本身无法在测试里直接调用。
//! 因此这里测的是命令层背后的完整链路——`ProjectManager`、`ProjectIndex`
//! 与 `enrich_recents` 桥接——即命令体内除 `State` 解包外的全部逻辑，
//! 且全部走真实 SQLite（非内存 mock），覆盖单元测试用假索引覆盖不到的
//! 持久化行为。

use inkos_desktop::project::{enrich_recents, ProjectManager, ProjectType};
use inkos_desktop::projects::RecentProject;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// 建一个带真实 SQLite 的 manager，外加一个可放项目的目录。
fn setup() -> (TempDir, TempDir, ProjectManager) {
    let db_dir = TempDir::new().unwrap();
    let work_dir = TempDir::new().unwrap();
    let manager = ProjectManager::new(&db_dir.path().join("projects.db")).unwrap();
    (db_dir, work_dir, manager)
}

/// 在 base 下建一个 Node.js 项目，返回其路径。
fn make_nodejs(base: &Path, dir: &str, name: &str) -> std::path::PathBuf {
    let p = base.join(dir);
    fs::create_dir_all(&p).unwrap();
    fs::write(p.join("package.json"), format!(r#"{{"name": "{}"}}"#, name)).unwrap();
    p
}

/// add_project → list_projects → remove_project 的完整往返，跨真实数据库。
#[test]
fn add_list_remove_round_trip() {
    let (_db, work, manager) = setup();
    let path = make_nodejs(work.path(), "app", "my-app");

    let added = manager.add_project(&path).unwrap();
    assert_eq!(added.project_type, ProjectType::NodeJs);

    let listed = manager.list_projects().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, added.id);

    manager.remove_project(&added.id).unwrap();
    assert!(manager.list_projects().unwrap().is_empty());
}

/// 重复添加同一路径必须失败——UNIQUE(path) 约束在真实数据库上生效。
#[test]
fn duplicate_path_is_rejected_by_db() {
    let (_db, work, manager) = setup();
    let path = make_nodejs(work.path(), "app", "my-app");

    manager.add_project(&path).unwrap();
    assert!(manager.add_project(&path).is_err());
    assert_eq!(manager.list_projects().unwrap().len(), 1);
}

/// toggle_favorite 命令的核心：状态翻转后必须落盘（不只是改缓存）。
#[test]
fn toggle_favorite_persists_across_cache_clear() {
    let (db, work, manager) = setup();
    let db_path = db.path().join("projects.db");
    let path = make_nodejs(work.path(), "app", "my-app");
    let added = manager.add_project(&path).unwrap();

    manager.toggle_favorite(&added.id).unwrap();
    assert!(manager.get_project(&added.id).unwrap().unwrap().is_favorite);

    // 换一个 manager 重新打开同一数据库：验证写的是磁盘而非内存缓存
    drop(manager);
    let reopened = ProjectManager::new(&db_path).unwrap();
    assert!(reopened.get_project(&added.id).unwrap().unwrap().is_favorite);

    // 再翻一次回到未收藏
    reopened.toggle_favorite(&added.id).unwrap();
    assert!(!reopened.get_project(&added.id).unwrap().unwrap().is_favorite);
}

/// open_project 命令：last_opened_at 落盘，且进入 list_recent。
#[test]
fn open_project_records_recency() {
    let (_db, work, manager) = setup();
    let a = manager
        .add_project(&make_nodejs(work.path(), "a", "a"))
        .unwrap();
    let b = manager
        .add_project(&make_nodejs(work.path(), "b", "b"))
        .unwrap();

    // 未打开过 → 不在最近列表
    assert!(manager.list_recent(10).unwrap().is_empty());

    manager.open_project(&a.id).unwrap();
    let recent = manager.list_recent(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, a.id);
    assert!(recent[0].last_opened_at.is_some());

    // b 后打开 → 排在 a 前面
    manager.open_project(&b.id).unwrap();
    let recent = manager.list_recent(10).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].id, b.id);
}

/// scan_projects 命令：一次扫描收录多个项目，重复扫描不产生重复行。
#[tokio::test]
async fn scan_is_idempotent_over_real_db() {
    let (_db, work, manager) = setup();
    make_nodejs(work.path(), "one", "one");
    make_nodejs(work.path(), "two", "two");

    let first = manager.scan_and_add(work.path()).await.unwrap();
    assert_eq!(first.len(), 2);

    let second = manager.scan_and_add(work.path()).await.unwrap();
    assert!(second.is_empty(), "重复扫描不应新增");
    assert_eq!(manager.list_projects().unwrap().len(), 2);
}

/// 回归：同一秒内连续打开多个项目，顺序必须严格反映打开次序。
///
/// 曾经的实现用秒级 `last_opened_at` 排序，这些打开会全部并列，
/// `list_recent` 返回任意顺序。现在排序键是库内单调的 `open_seq`。
#[test]
fn recent_order_is_exact_within_the_same_second() {
    let (_db, work, manager) = setup();
    let ids: Vec<_> = ["a", "b", "c", "d"]
        .iter()
        .map(|n| {
            manager
                .add_project(&make_nodejs(work.path(), n, n))
                .unwrap()
                .id
        })
        .collect();

    // 无 sleep：四次打开几乎必然落在同一秒
    for id in &ids {
        manager.open_project(id).unwrap();
    }

    let recent: Vec<String> = manager
        .list_recent(10)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();

    let mut expected: Vec<String> = ids.clone();
    expected.reverse(); // 最后打开的在最前
    assert_eq!(recent, expected, "最近列表须严格按打开次序倒排");

    // 重新打开最早的那个 → 它必须跳到首位
    manager.open_project(&ids[0]).unwrap();
    assert_eq!(manager.list_recent(1).unwrap()[0].id, ids[0]);
}

/// 排序序号不被无关的 update 清掉（update 不写 open_seq）。
#[test]
fn unrelated_update_preserves_recent_order() {
    let (_db, work, manager) = setup();
    let a = manager
        .add_project(&make_nodejs(work.path(), "a", "a"))
        .unwrap();
    let b = manager
        .add_project(&make_nodejs(work.path(), "b", "b"))
        .unwrap();

    manager.open_project(&a.id).unwrap();
    manager.open_project(&b.id).unwrap();

    // 改 a 的元数据（收藏），不应影响它在最近列表中的位置
    manager.toggle_favorite(&a.id).unwrap();

    let recent: Vec<String> = manager
        .list_recent(10)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(recent, vec![b.id, a.id]);
}

/// cmd_get_launch_state 的增强链路：projects.json 的最近项 + 索引元数据。
#[test]
fn launch_state_enrichment_over_real_index() {
    let (_db, work, manager) = setup();
    let indexed = make_nodejs(work.path(), "indexed", "indexed-app");
    let added = manager.add_project(&indexed).unwrap();
    manager.toggle_favorite(&added.id).unwrap();

    // 一条在索引里，一条不在（模拟老用户历史 recents）
    let recents = vec![
        RecentProject {
            path: indexed.to_string_lossy().into_owned(),
            name: "indexed-app".into(),
        },
        RecentProject {
            path: "/gone/from/index".into(),
            name: "stale".into(),
        },
    ];

    let enriched = enrich_recents(&recents, |p| {
        manager.get_by_path(Path::new(p)).ok().flatten()
    });

    assert_eq!(enriched.len(), 2);
    // 命中：带类型 + ID + 收藏
    assert_eq!(enriched[0].project_type.as_deref(), Some("nodejs"));
    assert_eq!(enriched[0].project_id.as_deref(), Some(added.id.as_str()));
    assert!(enriched[0].is_favorite);
    // 未命中：退化，但仍可展示与点击
    assert_eq!(enriched[1].name, "stale");
    assert_eq!(enriched[1].project_type, None);
    assert_eq!(enriched[1].project_id, None);
    assert!(!enriched[1].is_favorite);
}

/// cmd_choose_project 的同步链路：ensure_indexed 让手选的项目进入索引，
/// 于是下次启动 picker 就能显示它的类型徽章。
#[test]
fn ensure_indexed_backfills_chosen_project() {
    let (_db, work, manager) = setup();
    let path = make_nodejs(work.path(), "chosen", "chosen-app");

    // 选择前索引里没有
    assert!(manager.get_by_path(&path).unwrap().is_none());

    let meta = manager.ensure_indexed(&path).unwrap();
    assert_eq!(meta.project_type, ProjectType::NodeJs);

    // 再次选择同一项目：ID 稳定，不产生重复行
    let again = manager.ensure_indexed(&path).unwrap();
    assert_eq!(again.id, meta.id);
    assert_eq!(manager.list_projects().unwrap().len(), 1);

    // 此时 picker 的最近列表已能拿到增强数据
    let recents = vec![RecentProject {
        path: path.to_string_lossy().into_owned(),
        name: "chosen-app".into(),
    }];
    let enriched = enrich_recents(&recents, |p| {
        manager.get_by_path(Path::new(p)).ok().flatten()
    });
    assert_eq!(enriched[0].project_type.as_deref(), Some("nodejs"));
}

/// search_projects 命令：按名称模糊匹配，走真实 SQL LIKE。
#[test]
fn search_matches_by_name_substring() {
    let (_db, work, manager) = setup();
    manager
        .add_project(&make_nodejs(work.path(), "one", "alpha-service"))
        .unwrap();
    manager
        .add_project(&make_nodejs(work.path(), "two", "beta-service"))
        .unwrap();
    manager
        .add_project(&make_nodejs(work.path(), "three", "gamma-tool"))
        .unwrap();

    assert_eq!(manager.search_projects("service").unwrap().len(), 2);
    assert_eq!(manager.search_projects("alpha").unwrap().len(), 1);
    assert!(manager.search_projects("nonexistent").unwrap().is_empty());
}

/// list_favorites 命令：只返回收藏项。
#[test]
fn list_favorites_filters_correctly() {
    let (_db, work, manager) = setup();
    let a = manager
        .add_project(&make_nodejs(work.path(), "a", "a"))
        .unwrap();
    manager
        .add_project(&make_nodejs(work.path(), "b", "b"))
        .unwrap();

    assert!(manager.list_favorites().unwrap().is_empty());

    manager.toggle_favorite(&a.id).unwrap();
    let favs = manager.list_favorites().unwrap();
    assert_eq!(favs.len(), 1);
    assert_eq!(favs[0].id, a.id);
}

/// 删除项目后其健康记录随之消失（ON DELETE CASCADE + PRAGMA foreign_keys）。
#[test]
fn removing_project_cascades_health_record() {
    let (_db, work, manager) = setup();
    let path = make_nodejs(work.path(), "app", "app");
    let added = manager.add_project(&path).unwrap();

    // 同一路径可以重新添加（说明行确实被删掉了，而非残留导致 UNIQUE 冲突）
    manager.remove_project(&added.id).unwrap();
    let re_added = manager.add_project(&path).unwrap();
    assert_ne!(re_added.id, added.id);
    assert_eq!(manager.list_projects().unwrap().len(), 1);
}
