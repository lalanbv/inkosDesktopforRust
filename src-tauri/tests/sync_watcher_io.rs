//! spawn_writeback 端到端 IO 测（依赖 notify timing，全部 `#[ignore]`）。
//!
//! 从 `tests/sync_unit.rs` 拆出（C6 修复：sync_unit.rs 文件 < 800 行）。
//! 这些测依赖真实 notify watcher，CI / headless 环境不跑。手动验证：
//!
//! ```bash
//! cd src-tauri && cargo test --test sync_watcher_io -- --ignored
//! ```
//!
//! ## 988 号修复：固定 sleep → 探针自同步
//!
//! 旧版用固定 `WATCHER_REGISTER_MS`（200ms）猜「watcher 注册完成」再写目标文件。
//! 实测注册时延环境相关且波动大（本机同日实测：500ms 不够、2000ms 够、另轮
//! ~1.7s——FSEventStream 首建 + 测试进程冷启动负载敏感），写入落在注册完成前
//! 即被静默丢弃：`watcher_spa_write_writebacks_to_keychain` 必红、
//! `watcher_anti_loop_programmatic_write_does_not_writeback` **空真通过**（事件
//! 丢失时断言恒成立，真回归测不出）。M1a 教训复演：被忽略的测试路径不是零风险，
//! 是零观测。
//!
//! 修复：**探针自同步**。先写探针键并轮询 store 直到看见（`probe_watcher_ready`，
//! 未观测到则换值重写重试——注册完成前的探针写会被静默丢弃，重写自愈），观测到即
//! 证明 watcher 活体；此后的写不再可能丢事件，断言改轮询等待（`wait_until`），
//! 彻底消除猜 sleep。探针返回实际观测值，后续步骤全部以它为基线动态取新值。
//! 防回环测在「跳过周期已跑、删除传播写未发」的中途点断言，使反回环回归可观测
//! （删除传播会把中途泄漏的 A 键在末写时删掉，事后断言不可见）。

use inkos_desktop::secrets::store::{MockStore, SecretStore};
use inkos_desktop::secrets::sync::{spawn_writeback, DEBOUNCE_MS, POLL_TIMEOUT_MS};

use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// 轮询步长。
const POLL_STEP_MS: u64 = 25;

/// 单轮观测窗口（覆盖 debounce + 处理 + FSEvents 抖动）。
const ROUND_MS: u64 = 2 * DEBOUNCE_MS + 500;

/// 探针重写轮数上限（每轮 `ROUND_MS`；注册再慢也够，超出即判链路断裂）。
const PROBE_ROUNDS: usize = 6;

/// 防回环跳过周期的等待窗：事件送达 + debounce 静默满 + 处理完成。
const SKIP_WINDOW_MS: u64 = 2 * DEBOUNCE_MS;

/// 探针键：不与真实 service 命名空间重叠（MockStore 测试内隔离，仅防可读性混淆）。
const PROBE_KEY: &str = "__watch_probe__";

type Store = Arc<dyn SecretStore>;

/// 轮询等待 `pred` 成立，返回成立时的 store 快照；超时返回 None（调用方带现场 panic）。
fn wait_until(
    store: &Store,
    pred: impl Fn(&HashMap<String, String>) -> bool,
) -> Option<HashMap<String, String>> {
    let deadline = Instant::now() + Duration::from_millis(ROUND_MS);
    while Instant::now() < deadline {
        let all = store.read_all().unwrap();
        if pred(&all) {
            return Some(all);
        }
        thread::sleep(Duration::from_millis(POLL_STEP_MS));
    }
    None
}

/// 探针自同步：写探针键并轮询 store，未观测到则换值重写重试。
///
/// 观测到探针 ⇒ watcher 已注册且事件→debounce→回写链路活体 ⇒ 此后的写不再可能
/// 被静默丢弃（注册窗口竞态只存在于 watch 生效前）。返回实际观测到的探针值，
/// 调用方据此取后续互异值（硬编码字面量会在「重试轮数 > 1」时假红——988 号首轮
/// 实测教训）。
fn probe_watcher_ready(store: &Store, path: &Arc<std::path::PathBuf>) -> String {
    for round in 0..PROBE_ROUNDS {
        let val = format!("p{round}");
        write_secrets_content(path, &[(PROBE_KEY, &val)]);
        if let Some(all) =
            wait_until(store, |m| m.get(PROBE_KEY).map(String::as_str) == Some(val.as_str()))
        {
            assert_eq!(
                all.get(PROBE_KEY).unwrap(),
                &val,
                "探针观测竞态：wait_until 返回的快照与预测值不符"
            );
            return val;
        }
    }
    panic!(
        "watcher 探针 {PROBE_ROUNDS} 轮未观测到回写——watcher 未注册（看 stderr 的 \
         [secrets-writeback] 日志）或事件链路断裂"
    );
}

/// 写 `secrets.json`（测试内直写；文件事件触发 watcher）。
fn write_secrets_content(path: &std::path::Path, entries: &[(&str, &str)]) {
    let mut body = String::from(r#"{"services":{"#);
    for (i, (k, v)) in entries.iter().enumerate() {
        if i > 0 {
            body.push(',');
        }
        // 值是受控测试字面量（p<N>、p<N>_prog 等，无转义需求）
        body.push_str(&format!(r#""{k}":{{"apiKey":"{v}"}}"#));
    }
    body.push_str("}}");
    fs::write(path, body).unwrap();
}

#[test]
#[ignore = "依赖 notify timing；跑：cargo test --test sync_watcher_io -- --ignored watcher_anti_loop_programmatic"]
fn watcher_anti_loop_programmatic_write_does_not_writeback() {
    let dir = tempfile::tempdir().unwrap();
    let path = Arc::new(dir.path().join("secrets.json"));
    write_secrets_content(&path, &[]);

    let store: Store = Arc::new(MockStore::new());
    // 探针阶段需要回写活体 → syncing 先 false；程序化写阶段再置 true。
    let syncing = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(AtomicBool::new(false));

    let handle = spawn_writeback(
        Arc::clone(&store),
        Arc::clone(&path),
        Arc::clone(&syncing),
        Arc::clone(&shutdown),
    );

    // ① 探针自同步：证明 watcher 活体，拿到基线值 v0（store == {probe: v0}）。
    let v0 = probe_watcher_ready(&store, &path);
    let v_prog = format!("{v0}_prog"); // 程序化写携带的探针值（防回环正确则不得出现）
    let v_final = format!("{v0}_final"); // 末写携带的探针值（轮询锚点）

    // ② 程序化写（syncing=true 防回环）：json 同时改探针值 v0→v_prog 并新增 A 键。
    //    若防回环失效，该周期会把 v_prog 与 A 一起 upsert——中途断言可观测。
    syncing.store(true, Ordering::SeqCst);
    write_secrets_content(&path, &[(PROBE_KEY, &v_prog), ("A", "va")]);
    // 等跳过周期跑完（事件送达 + debounce 满窗 + 处理；探针已证活体 ⇒ 周期必跑）
    thread::sleep(Duration::from_millis(SKIP_WINDOW_MS + 300));

    // ③ 中途断言（在删除传播写之前——事后 A 会被末写的删除传播清掉，不可观测）：
    //    防回环正确 ⇒ store 仍是探针 v0、无 A；防回环失效 ⇒ 探针已 v_prog / 出现 A。
    let mid = store.read_all().unwrap();
    assert_eq!(
        mid.get(PROBE_KEY).map(String::as_str),
        Some(v0.as_str()),
        "syncing=true 的程序化写不得更新 store（防回环失效信号一）；got {mid:?}"
    );
    assert!(
        !mid.contains_key("A"),
        "syncing=true 的程序化写不得把 A 写回 store（防回环失效信号二）；got {mid:?}"
    );

    // ④ 末写（syncing=false）：探针 v0→v_final 且移除 A。轮询到 v_final 即证明
    //    本周期已处理，中途断言的观测点成立且此后无再写，结论稳定。
    syncing.store(false, Ordering::SeqCst);
    write_secrets_content(&path, &[(PROBE_KEY, &v_final)]);
    let final_all = wait_until(&store, |m| {
        m.get(PROBE_KEY).map(String::as_str) == Some(v_final.as_str())
    })
    .expect("末写探针未回写——事件链路在中途断言后断裂");

    // ⑤ 收尾断言：store 恰为 {probe: v_final}（A 从未写入；删除传播无残留可清）。
    let mut expected = HashMap::new();
    expected.insert(PROBE_KEY.to_string(), v_final.clone());
    assert_eq!(final_all, expected, "防回环终态不符");

    shutdown.store(true, Ordering::SeqCst);
    handle.join().expect("watcher thread must not panic");
}

#[test]
#[ignore = "依赖 notify timing；跑：cargo test --test sync_watcher_io -- --ignored watcher_spa_write_writebacks"]
fn watcher_spa_write_writebacks_to_keychain() {
    let dir = tempfile::tempdir().unwrap();
    let path = Arc::new(dir.path().join("secrets.json"));
    write_secrets_content(&path, &[]);

    let store: Store = Arc::new(MockStore::new());
    let syncing = Arc::new(AtomicBool::new(false)); // SPA 改
    let shutdown = Arc::new(AtomicBool::new(false));

    let handle = spawn_writeback(
        Arc::clone(&store),
        Arc::clone(&path),
        Arc::clone(&syncing),
        Arc::clone(&shutdown),
    );

    // 探针自同步：证明 watcher 活体（store 现含 {__watch_probe__: v0}）
    let _v0 = probe_watcher_ready(&store, &path);

    // SPA 写：只含 openai 键（探针键被移除 → 顺带演练删除传播：store 中的探针键应被删）
    write_secrets_content(&path, &[("openai", "sk-new")]);

    // 轮询断言替代固定 settle：upsert 生效且探针键被删除传播清掉
    let all = wait_until(&store, |m| {
        m.get("openai").map(String::as_str) == Some("sk-new") && !m.contains_key(PROBE_KEY)
    })
    .expect("SPA 写回写未观测到（openai 未 upsert 或探针删除传播未跑）");

    let mut expected = HashMap::new();
    expected.insert("openai".to_string(), "sk-new".to_string());
    assert_eq!(all, expected, "SPA 回写终态不符");

    shutdown.store(true, Ordering::SeqCst);
    handle.join().expect("watcher thread must not panic");
}

/// shutdown 标记让 watcher 线程在 ≤ `POLL_TIMEOUT_MS * 3` 内干净退出。
///
/// 先探针证明主循环已在 recv（988 号：watcher 初始化耗时波动大且被计入 join，
/// 不先对齐就把初始化时延误算进「循环响应性」）。置位后的退出延迟含尾随
/// flush-on-shutdown 一次（break 后仍执行 process_writeback 排掉悬置回写）；
/// debounce 窗内等待已按 poll 切片，检查点密度与主循环一致——切片修复前置位
/// 落在窗中要白等满剩余窗（实测尾延迟 492ms/1.16s），界 300ms 不可守。
#[test]
#[ignore = "依赖 notify timing；跑：cargo test --test sync_watcher_io -- --ignored watcher_shutdown_exits_cleanly"]
fn watcher_shutdown_flag_exits_thread_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let path = Arc::new(dir.path().join("secrets.json"));
    write_secrets_content(&path, &[]);

    let store: Store = Arc::new(MockStore::new());
    let syncing = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(AtomicBool::new(false));

    let handle = spawn_writeback(
        Arc::clone(&store),
        Arc::clone(&path),
        Arc::clone(&syncing),
        Arc::clone(&shutdown),
    );

    // 主循环已在 recv（探针周期走完 ⇒ shutdown 检查点可达）
    let _v0 = probe_watcher_ready(&store, &path);

    shutdown.store(true, Ordering::SeqCst);
    let started = Instant::now();
    handle.join().expect("watcher thread must not panic");
    let elapsed = started.elapsed();
    // 名义界 = 3×POLL（切片检查点 + 一次 flush）；×10 吸收满载调度尾——sweep
    // 语境（lib 重套件刚跑完 + 并行走查负载）实测过一次 620ms（988 号），结构面
    // 无白等路径（窗内等待已按 poll 切片），超出 1s 才可能是真挂。
    let bound = POLL_TIMEOUT_MS * 10;
    assert!(
        elapsed <= Duration::from_millis(bound),
        "shutdown 后线程应在 ≤ {bound}ms 内退出（含尾随 flush-on-shutdown），实际 {elapsed:?}"
    );
}
