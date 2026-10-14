//! C9 SSE 契约测：双引擎 broadcast 事件集合之并 ⊇ Rust `Router::default_table` 路由键。
//!
//! 架构 §10 承诺 observer 与引擎通过 SSE 事件解耦：若引擎改事件名而 observer
//! 未跟进，observer 会**静默失活**——事件被 `dispatch` 当未知事件忽略，用户不再收到
//! 通知/角标。688 号起桌面默认 Rust 引擎、Node 为回退，故契约源=**双引擎并集**：
//! TS `packages/studio/src/api/server.ts` 与 Rust `engine-rs/src/server/**/*.rs`
//! （两侧 `broadcast("event"` 字面形态一致，同一提取器通用）。任一引擎下 observer
//! 路由键都必须可被该引擎（或另一引擎）发出，否则静默失活。反向差异（引擎有但
//! router 没路由的事件）打印供维护者参考——未知事件被忽略是设计（架构 §6.2 容错）。
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
    // ① 行级剥注释：整行注释（// 或 * 块注释延续行）直接跳过；行内尾注释保留
    //（其位于匹配点之后，不影响提取，原始行为如此）。
    // ② 空白归一化：Rust 侧调用常跨行（`hub.broadcast(\n "event",`），折叠空白后
    // `broadcast("` 与 `broadcast( "` 两种形态统一扫描。
    let mut joined = String::with_capacity(src.len());
    for line in src.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("*") {
            continue;
        }
        joined.push_str(line);
        joined.push(' ');
    }
    let mut norm = String::with_capacity(joined.len());
    let mut prev_space = false;
    for ch in joined.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                norm.push(' ');
            }
            prev_space = true;
        } else {
            norm.push(ch);
            prev_space = false;
        }
    }
    let text = norm;

    let mut out = Vec::new();
    for needle in ["broadcast(\"", "broadcast( \""] {
        let mut search_from = 0;
        let bytes = text.as_bytes();
        while let Some(rel) = text[search_from..].find(needle) {
            let start = search_from + rel + needle.len();
            let end = bytes[start..]
                .iter()
                .position(|&b| !(b.is_ascii_alphanumeric() || b == b':' || b == b'_' || b == b'-'))
                .map(|p| start + p)
                .unwrap_or(text.len());
            if end > start {
                let name = &text[start..end];
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
    // 锚定 src-tauri 目录：TS 契约源 = studio server.ts；Rust 契约源 = engine-rs/src/server 递归。
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let base = std::path::Path::new(manifest_dir);
    let inkos_path = base.join(INKOS_SERVER_TS_REL);
    let rust_server_dir = base.join("../engine-rs/src/server");

    let mut sources: Vec<(std::path::PathBuf, String)> = Vec::new();
    if let Ok(s) = std::fs::read_to_string(&inkos_path) {
        sources.push((inkos_path.clone(), s));
    } else {
        eprintln!("[sse_contract] TS 源不可读（{}），仅以 Rust 引擎为契约源", inkos_path.display());
    }
    // Rust 侧递归收集（server 目录全量 .rs——路由分文件持有各域 broadcast 面）。
    fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    collect_rs(&p, out);
                } else if p.extension().map(|e| e == "rs").unwrap_or(false) {
                    out.push(p);
                }
            }
        }
    }
    let mut rs_files: Vec<std::path::PathBuf> = Vec::new();
    collect_rs(&rust_server_dir, &mut rs_files);
    rs_files.sort();
    for p in &rs_files {
        if let Ok(s) = std::fs::read_to_string(p) {
            sources.push((p.clone(), s));
        }
    }

    let mut inkos_events: Vec<String> = Vec::new();
    for (_, src) in &sources {
        inkos_events.extend(extract_broadcast_events(src));
    }
    inkos_events.sort();
    inkos_events.dedup();
    assert!(
        !inkos_events.is_empty(),
        "[sse_contract] 双引擎源可读但提取到 0 个 broadcast 事件——正则失效或源被重构"
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
