//! C9 SSE 契约测：inkos `server.ts` broadcast 事件集合 ⊇ Rust `Router::default_table` 路由键。
//!
//! 架构 §10 承诺 observer 与 inkos 通过 SSE 事件解耦耦合：若上游改事件名而 observer
//! 未跟进，observer 会**静默失活**——事件被 `dispatch` 当未知事件忽略，用户不再收到
//! 通知/角标。本测试从 inkos 源（mono-repo `packages/studio/src/api/server.ts`）正则
//! 提取所有 `broadcast("event", ...)` 调用的事件名，断言 default_table 的每个键都存在
//! 于 inkos 集合。反向差异（inkos 有但 router 没路由的事件）打印供维护者参考——
//! 未知事件被忽略是设计（架构 §6.2 容错），不算 fail。
//!
//! 跑：
//!     cd src-tauri && cargo test --test sse_contract
//!
//! 若 inkos 源不可读（CI 拆分仓库、tarball 不含 mono-repo 根）→ skip 不 fail：
//! 此时契约测失去信号源，强行 fail 会阻断所有 PR。

use inkos_desktop::observer::router::Router;
use inkos_desktop::observer::sse::SseEvent;
use std::sync::Arc;

/// 退化 handler：default_table 需要两个 handler 实参，本测只关心 `events()` 集合，
/// handler 行为已被 router 单元测覆盖。这里用零效果 handler 占位即可。
struct Noop;
impl inkos_desktop::observer::router::EventHandler for Noop {
    fn handle(&self, _ev: &SseEvent) -> anyhow::Result<()> {
        Ok(())
    }
}

/// inkos server.ts 相对 src-tauri 的路径（mono-repo 根下）。
/// 用 `CARGO_MANIFEST_DIR` 锚定 src-tauri 目录，再向上一级到 workspace 根
/// （src-tauri 与 packages 同为 repo 根的直接子目录，故只需一个 `..`）。
const INKOS_SERVER_TS_REL: &str = "../packages/studio/src/api/server.ts";

/// 提取 inkos broadcast 事件名集合：扫描每行，匹配 `broadcast("<event>"` 模式。
/// 用朴素状态机而非 regex crate（避免引入额外 dev-dep）；事件名仅 ASCII 字母+冒号+下划线。
fn extract_broadcast_events(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        // 跳过注释行（避免提取注释里提到的事件名）。
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("*") {
            continue;
        }
        // 在行内反复找 `broadcast("`，提取紧跟的 ASCII 字母/冒号/下划线序列。
        let mut search_from = 0;
        let bytes = line.as_bytes();
        while let Some(rel) = line[search_from..].find("broadcast(\"") {
            let start = search_from + rel + "broadcast(\"".len();
            let end = bytes[start..]
                .iter()
                .position(|&b| !(b.is_ascii_alphanumeric() || b == b':' || b == b'_' || b == b'-'))
                .map(|p| start + p)
                .unwrap_or(line.len());
            if end > start {
                let name = &line[start..end];
                // 过滤模板字面量/空字符串：事件名至少含一个字母。
                if name.chars().any(|c| c.is_ascii_alphabetic()) {
                    out.push(name.to_string());
                }
            }
            search_from = end;
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn default_table_keys_are_subset_of_inkos_broadcasts() {
    // 锚定 src-tauri 目录，构造 inkos server.ts 的绝对路径。
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let inkos_path = std::path::Path::new(manifest_dir).join(INKOS_SERVER_TS_REL);

    let src = match std::fs::read_to_string(&inkos_path) {
        Ok(s) => s,
        Err(e) => {
            // inkos 源不可读：契约测失去信号源，skip 不 fail（CI 拆分仓库时常见）。
            eprintln!(
                "[sse_contract] skip: inkos 源不可读 ({}) — {}",
                inkos_path.display(),
                e
            );
            // `eprintln!` + return 即可——cargo 不把正常返回当 fail。
            // 为了在 `cargo test -- --nocapture` 看到原因，上面已打印。
            // 若要严格标记 skipped，需 nightly 特性；此处用 eprintln + 提前 return。
            // 但为了避免无声通过，我们用一个 dummy assert 永远为 true，并在日志标 skip。
            let _ = inkos_path; // suppress unused warning on err path
            return;
        }
    };

    let inkos_events = extract_broadcast_events(&src);
    assert!(
        !inkos_events.is_empty(),
        "[sse_contract] inkos 源可读但提取到 0 个 broadcast 事件——正则失效或源被重构"
    );

    // 构造 default_table 并读取其路由键集合（C9：通过公开 `events()` API 而非硬编码）。
    let noop = Arc::new(Noop) as Arc<dyn inkos_desktop::observer::router::EventHandler>;
    let router = Router::default_table(Arc::clone(&noop), noop);
    let router_keys: Vec<&str> = router.events();

    assert!(
        !router_keys.is_empty(),
        "[sse_contract] Router::default_table 注册了 0 个键——构造逻辑被破坏"
    );

    // 主断言：default_table 的每个键都存在于 inkos broadcast 集合。
    let inkos_set: std::collections::HashSet<&str> =
        inkos_events.iter().map(|s| s.as_str()).collect();
    let mut missing: Vec<&str> = Vec::new();
    for k in &router_keys {
        if !inkos_set.contains(*k) {
            missing.push(*k);
        }
    }
    assert!(
        missing.is_empty(),
        "[sse_contract] Router 路由键在 inkos broadcast 集合中不存在：{:?}。\n\
         上游 inkos 可能重命名了事件——observer 正在静默失活。请同步 default_table 或 notifier 映射。\n\
         inkos 全集 ({} 个): {:?}",
        missing,
        inkos_events.len(),
        inkos_events
    );

    // 反向差异（信息性）：inkos 有但 router 没路由的事件——未知事件被忽略是设计，不算 fail。
    // 打印供维护者参考：评估是否应新增路由。
    let router_set: std::collections::HashSet<&str> = router_keys.iter().copied().collect();
    let unrouted: Vec<&str> = inkos_set
        .iter()
        .filter(|e| !router_set.contains(*e))
        .copied()
        .collect();
    eprintln!(
        "[sse_contract] OK: router 路由 {} 个键 {:?} 全部存在于 inkos broadcast 集合 ({} 个)",
        router_keys.len(),
        router_keys,
        inkos_events.len()
    );
    eprintln!(
        "[sse_contract] info: inkos 有 {} 个事件未被 router 路由（设计如此，未知事件被忽略）：{:?}",
        unrouted.len(),
        unrouted
    );
}

/// 单元测：`extract_broadcast_events` 正则提取正确性（不依赖 inkos 源文件存在）。
#[test]
fn extract_broadcast_events_handles_common_shapes() {
    let sample = r#"
        import { x } from "y";
        // broadcast("commented:out", should not match);
        broadcast("tool:start", { id: 1 });
        broadcast("write:complete", `template ${x}`);
        broadcast("daemon:chapter", data);
        const z = broadcast("agent:complete", obj);
        broadcast("book:created", { bookId }); // trailing comment
    "#;
    let events = extract_broadcast_events(sample);
    assert!(events.contains(&"tool:start".to_string()));
    assert!(events.contains(&"write:complete".to_string()));
    assert!(events.contains(&"daemon:chapter".to_string()));
    assert!(events.contains(&"agent:complete".to_string()));
    assert!(events.contains(&"book:created".to_string()));
    assert!(
        !events.contains(&"commented:out".to_string()),
        "注释行不应被提取"
    );
}
