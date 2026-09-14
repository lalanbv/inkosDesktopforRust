//! 运行时状态编排（runtime-state-store）。
//!
//! 移植自 `packages/core/src/state/runtime-state-store.ts`（164 行）。state 域 I/O 编排层的最顶层——
//! 对外暴露加载快照 / 应用增量产出投影 / 持久化快照的完整生命周期，消费 state 域全部已移植子模块：
//!
//! - `[`state_bootstrap`]`：从 markdown 引导结构化状态
//! - `[`reducer`]`：增量归约（`apply_runtime_state_delta`）
//! - `[`validator`]`：快照校验（`validate_runtime_state`）
//! - `[`hook_arbiter`]`：hook 操作裁决
//! - `[`projections`]`：markdown 投影渲染
//! - `[`store`]`：fs 抽象（`StateStore` trait）
//!
//! 全部方法消费 `&dyn StateStore`，可注入 [`InMemoryStateStore`](crate::state::store::InMemoryStateStore) 单测。

use crate::models::runtime_state::{ChapterSummaryRow, CurrentStateFact, 
    ChapterSummariesState, CurrentStateState, HooksState, RuntimeStateDelta, StateManifest,
};
use crate::utils::hook_arbiter::arbitrate_runtime_state_delta_hooks;
use crate::state::projections::{
    render_chapter_summaries_projection, render_current_state_projection, render_hooks_projection,
};
use crate::state::reducer::{apply_runtime_state_delta, RuntimeStateSnapshot};
use crate::state::state_bootstrap::bootstrap_structured_state_from_markdown;
use crate::state::store::{join_path, StateStore};
use crate::state::validator::{validate_runtime_state, RuntimeStateValidationIssue, ValidationInput};
use crate::utils::language::WritingLanguage;

/// 把业务约束错误消息包装为 `EngineError::Constraint`（顶层编排的校验/reducer 失败统一走此变体）。
fn constraint(msg: impl Into<String>) -> crate::EngineError {
    crate::EngineError::Constraint(msg.into())
}

/// 加载运行时状态快照：先 bootstrap（确保 4 个 JSON 存在且合法），再读入 + 校验。
///
/// 对齐 TS `loadRuntimeStateSnapshot`。校验失败（issues 非空）→ `EngineError::Constraint`。
pub async fn load_runtime_state_snapshot(
    store: &dyn StateStore,
    book_dir: &str,
) -> crate::Result<RuntimeStateSnapshot> {
    bootstrap_structured_state_from_markdown(store, book_dir, None).await?;
    let state_dir = join_path(&join_path(book_dir, "story"), "state");

    let manifest_raw = read_required_json(store, &join_path(&state_dir, "manifest.json")).await?;
    let current_state_raw =
        read_required_json(store, &join_path(&state_dir, "current_state.json")).await?;
    let hooks_raw = read_required_json(store, &join_path(&state_dir, "hooks.json")).await?;
    let chapter_summaries_raw =
        read_required_json(store, &join_path(&state_dir, "chapter_summaries.json")).await?;

    let issues = validate_runtime_state(&ValidationInput {
        manifest: manifest_raw.clone(),
        current_state: current_state_raw.clone(),
        hooks: hooks_raw.clone(),
        chapter_summaries: chapter_summaries_raw.clone(),
    });
    if !issues.is_empty() {
        return Err(constraint(format!(
            "Invalid persisted runtime state: {}",
            summarize_issues(&issues)
        )));
    }

    // 校验通过，反序列化必成功（validator 已确认）。
    let manifest: StateManifest = serde_json::from_value(manifest_raw)?;
    let current_state: CurrentStateState = serde_json::from_value(current_state_raw)?;
    let hooks: HooksState = serde_json::from_value(hooks_raw)?;
    let chapter_summaries: ChapterSummariesState = serde_json::from_value(chapter_summaries_raw)?;

    Ok(RuntimeStateSnapshot {
        manifest,
        current_state,
        hooks,
        chapter_summaries,
    })
}

/// 应用 delta 到快照，产出归约后快照 + 三份 markdown 投影。
///
/// 对齐 TS `buildRuntimeStateArtifacts`。hook 操作先经 [`arbitrate_runtime_state_delta_hooks`] 裁决，
/// 再 [`apply_runtime_state_delta`] 归约；投影用 resolved delta 的章节标注 stale/blocked hooks。
pub async fn build_runtime_state_artifacts(
    store: &dyn StateStore,
    book_dir: &str,
    delta: &RuntimeStateDelta,
    language: WritingLanguage,
    allow_reapply: Option<bool>,
    allow_new_hooks: Option<bool>,
) -> crate::Result<RuntimeStateArtifacts> {
    let snapshot = load_runtime_state_snapshot(store, book_dir).await?;
    let (resolved_delta, _decisions) =
        arbitrate_runtime_state_delta_hooks(&snapshot.hooks.hooks, delta, allow_new_hooks);
    let next =
        apply_runtime_state_delta(&snapshot, &resolved_delta, allow_reapply).map_err(|e| {
            constraint(format!("reducer: {e}"))
        })?;

    let resolved_chapter = resolved_delta.chapter;
    Ok(RuntimeStateArtifacts {
        snapshot: next.clone(),
        resolved_delta,
        current_state_markdown: render_current_state_projection(&next.current_state, language),
        hooks_markdown: render_hooks_projection(&next.hooks, language, Some(resolved_chapter)),
        chapter_summaries_markdown: render_chapter_summaries_projection(&next.chapter_summaries, language),
    })
}


/// 加载章前快照（TS `loadRuntimeStateSnapshotAtChapter`）：`story/snapshots/{N}`
/// 的 state 四 JSON 优先；markdown 重建兜底（manifest.lastAppliedChapter = N，
/// migrationWarnings 注明重建来源）。
pub async fn load_runtime_state_snapshot_at_chapter(
    store: &dyn StateStore,
    book_dir: &str,
    chapter: u32,
    language: WritingLanguage,
) -> crate::Result<RuntimeStateSnapshot> {
    let snapshot_dir = join_path(&join_path(book_dir, "story"), &format!("snapshots/{}", chapter));
    let state_dir = join_path(&snapshot_dir, "state");

    let try_json = |name: &str| {
        let path = join_path(&state_dir, name);
        async move {
            match store.read_to_string(&path).await {
                Ok(Some(raw)) => serde_json::from_str::<serde_json::Value>(&raw).ok(),
                _ => None,
            }
        }
    };
    let (manifest_raw, current_raw, hooks_raw, summaries_raw) = tokio::join!(
        try_json("manifest.json"),
        try_json("current_state.json"),
        try_json("hooks.json"),
        try_json("chapter_summaries.json"),
    );
    if let (Some(manifest_raw), Some(current_raw), Some(hooks_raw), Some(summaries_raw)) =
        (manifest_raw, current_raw, hooks_raw, summaries_raw)
    {
        let issues = validate_runtime_state(&ValidationInput {
            manifest: manifest_raw.clone(),
            current_state: current_raw.clone(),
            hooks: hooks_raw.clone(),
            chapter_summaries: summaries_raw.clone(),
        });
        if !issues.is_empty() {
            return Err(constraint(format!(
                "Invalid runtime snapshot at chapter {}: {}",
                chapter,
                summarize_issues(&issues)
            )));
        }
        return Ok(RuntimeStateSnapshot {
            manifest: serde_json::from_value(manifest_raw)?,
            current_state: serde_json::from_value(current_raw)?,
            hooks: serde_json::from_value(hooks_raw)?,
            chapter_summaries: serde_json::from_value(summaries_raw)?,
        });
    }

    // markdown 重建（TS 同款：current_state.md / pending_hooks.md 必读，
    // chapter_summaries.md 缺省为空）。
    let read_md = |name: &str| {
        let path = join_path(&snapshot_dir, name);
        async move { store.read_to_string(&path).await }
    };
    let (current_md, hooks_md, summaries_md) = tokio::join!(
        read_md("current_state.md"),
        read_md("pending_hooks.md"),
        read_md("chapter_summaries.md"),
    );
    let current_md = current_md?.unwrap_or_default();
    let hooks_md = hooks_md?.unwrap_or_default();
    let summaries_md = summaries_md?.unwrap_or_default();
    let lang_str = if language == WritingLanguage::En { "en" } else { "zh" };
    let facts = crate::utils::story_markdown::parse_current_state_facts(&current_md, chapter as i64)
        .into_iter()
        .map(|f| CurrentStateFact {
            subject: f.subject,
            predicate: f.predicate,
            object: f.object,
            valid_from_chapter: f.valid_from_chapter.max(0) as u32,
            valid_until_chapter: f.valid_until_chapter.map(|v| v.max(0) as u32),
            source_chapter: f.source_chapter.max(0) as u32,
        })
        .collect();
    let rows = crate::utils::story_markdown::parse_chapter_summaries_markdown(&summaries_md)
        .into_iter()
        .map(|row| ChapterSummaryRow {
            chapter: row.chapter.max(0) as u32,
            title: row.title,
            characters: row.characters,
            events: row.events,
            state_changes: row.state_changes,
            hook_activity: row.hook_activity,
            mood: row.mood,
            chapter_type: row.chapter_type,
            conflict_level: row.conflict_level.map(|v| v.max(0) as u32),
            reveal_level: row.reveal_level.map(|v| v.max(0) as u32),
        })
        .collect();
    Ok(RuntimeStateSnapshot {
        manifest: StateManifest {
            schema_version: 2,
            language: lang_str.to_string(),
            last_applied_chapter: chapter,
            projection_version: 1,
            migration_warnings: vec![format!(
                "runtime snapshot {} reconstructed from markdown",
                chapter
            )],
        },
        current_state: CurrentStateState { chapter, facts },
        hooks: HooksState {
            hooks: crate::utils::story_markdown::parse_pending_hooks_markdown(&hooks_md),
        },
        chapter_summaries: ChapterSummariesState { rows },
    })
}

/// 从指定快照归约产出工件（TS `buildRuntimeStateArtifactsFromSnapshot`）。
pub async fn build_runtime_state_artifacts_from_snapshot(
    snapshot: &RuntimeStateSnapshot,
    delta: &RuntimeStateDelta,
    language: WritingLanguage,
    allow_reapply: Option<bool>,
    allow_new_hooks: Option<bool>,
) -> crate::Result<RuntimeStateArtifacts> {
    let (resolved_delta, _decisions) =
        arbitrate_runtime_state_delta_hooks(&snapshot.hooks.hooks, delta, allow_new_hooks);
    let next = apply_runtime_state_delta(snapshot, &resolved_delta, allow_reapply)
        .map_err(|e| constraint(format!("reducer: {e}")))?;
    let resolved_chapter = resolved_delta.chapter;
    Ok(RuntimeStateArtifacts {
        snapshot: next.clone(),
        resolved_delta,
        current_state_markdown: render_current_state_projection(&next.current_state, language),
        hooks_markdown: render_hooks_projection(&next.hooks, language, Some(resolved_chapter)),
        chapter_summaries_markdown: render_chapter_summaries_projection(&next.chapter_summaries, language),
    })
}

/// 持久化快照到 `{book_dir}/story/state/` 的 4 个 JSON 文件。
///
/// 对齐 TS `saveRuntimeStateSnapshot`。目录若不存在则创建。
pub async fn save_runtime_state_snapshot(
    store: &dyn StateStore,
    book_dir: &str,
    snapshot: &RuntimeStateSnapshot,
) -> crate::Result<()> {
    let state_dir = join_path(&join_path(book_dir, "story"), "state");
    store.mkdir_p(&state_dir).await?;
    let manifest = serde_json::to_string_pretty(&snapshot.manifest)?;
    let current_state = serde_json::to_string_pretty(&snapshot.current_state)?;
    let hooks = serde_json::to_string_pretty(&snapshot.hooks)?;
    let chapter_summaries = serde_json::to_string_pretty(&snapshot.chapter_summaries)?;
    store.write_string(&join_path(&state_dir, "manifest.json"), &manifest).await?;
    store
        .write_string(&join_path(&state_dir, "current_state.json"), &current_state)
        .await?;
    store.write_string(&join_path(&state_dir, "hooks.json"), &hooks).await?;
    store
        .write_string(&join_path(&state_dir, "chapter_summaries.json"), &chapter_summaries)
        .await?;
    Ok(())
}

/// 增量应用 + 持久化的产物。对齐 TS `RuntimeStateArtifacts`。
#[derive(Debug, Clone)]
pub struct RuntimeStateArtifacts {
    pub snapshot: RuntimeStateSnapshot,
    pub resolved_delta: RuntimeStateDelta,
    pub current_state_markdown: String,
    pub hooks_markdown: String,
    pub chapter_summaries_markdown: String,
}

/// 叙事记忆的种子（summaries + hooks，转 memory-db 的持久化类型）。
/// 对齐 TS `NarrativeMemorySeed`，供 memory-db 批量写入。
#[derive(Debug, Clone, Default)]
pub struct NarrativeMemorySeed {
    pub summaries: Vec<crate::state::memory_db::StoredSummary>,
    pub hooks: Vec<crate::state::memory_db::StoredHook>,
}

/// 从快照提取 narrative memory seed（summaries + hooks 转为 memory-db 类型）。
/// 对齐 TS `loadNarrativeMemorySeed`（读快照后映射字段）。
pub async fn load_narrative_memory_seed(
    store: &dyn StateStore,
    book_dir: &str,
) -> crate::Result<NarrativeMemorySeed> {
    use crate::state::memory_db::{StoredHook, StoredSummary};
    let snapshot = load_runtime_state_snapshot(store, book_dir).await?;
    let summaries = snapshot
        .chapter_summaries
        .rows
        .into_iter()
        .map(|r| StoredSummary {
            chapter: r.chapter as i64,
            title: r.title,
            characters: r.characters,
            events: r.events,
            state_changes: r.state_changes,
            hook_activity: r.hook_activity,
            mood: r.mood,
            chapter_type: r.chapter_type,
            conflict_level: None,
            reveal_level: None,
        })
        .collect();
    let hooks = snapshot
        .hooks
        .hooks
        .into_iter()
        .map(|h| StoredHook {
            hook_id: h.hook_id,
            start_chapter: h.start_chapter as i64,
            r#type: h.hook_type,
            // status 变体均为单词（Progressing→"progressing"…），Debug+lowercase 安全。
            status: format!("{:?}", h.status).to_lowercase(),
            last_advanced_chapter: h.last_advanced_chapter as i64,
            expected_payoff: h.expected_payoff,
            // R26/409 号：payoff_timing 存在多词变体（NearTerm→"nearterm" 错值），
            // 必须走 canonical 映射（"near-term"/"mid-arc"/"slow-burn"）。
            payoff_timing: h
                .payoff_timing
                .map(crate::utils::hook_lifecycle::hook_payoff_timing_canonical)
                .unwrap_or_default()
                .to_string(),
            notes: h.notes,
        })
        .collect();
    Ok(NarrativeMemorySeed { summaries, hooks })
}

async fn read_required_json(store: &dyn StateStore, path: &str) -> crate::Result<serde_json::Value> {
    match store.read_to_string(path).await? {
        Some(raw) => Ok(serde_json::from_str(&raw)?),
        None => Err(constraint(format!("{path} missing after bootstrap"))),
    }
}

fn summarize_issues(issues: &[RuntimeStateValidationIssue]) -> String {
    issues
        .iter()
        .map(|i| match &i.path {
            Some(p) => format!("{}@{p}", i.code),
            None => i.code.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::runtime_state::{HookOps, HookStatus};
    use crate::state::store::InMemoryStateStore;

    /// 构造一个最小合法 book（含章节产物 + 3 个 markdown），供 bootstrap 产出有效状态。
    fn setup_min_book(store: &InMemoryStateStore, book_dir: &str) {
        store.set(&format!("{book_dir}/book.json"), r#"{"language":"zh"}"#);
        store.set(&format!("{book_dir}/chapters/1_a.md"), "x");
        store.set(
            &format!("{book_dir}/story/chapter_summaries.md"),
            "| 1 | t1 | 角色 | 事件 | 变化 | 活动 | 心情 | 类型 |",
        );
        store.set(
            &format!("{book_dir}/story/pending_hooks.md"),
            "| h01 | 1 | mystery | open | 1 | Reveal x | mid-arc | n |",
        );
        store.set(
            &format!("{book_dir}/story/current_state.md"),
            "| 字段 | 值 |\n|---|---|\n| 当前章节 | 1 |\n| 当前位置 | 森林 |",
        );
    }

    #[tokio::test]
    async fn load_runtime_state_snapshot_bootstraps_then_loads() {
        let store = InMemoryStateStore::new();
        setup_min_book(&store, "book");
        let snap = load_runtime_state_snapshot(&store, "book").await.unwrap();
        assert_eq!(snap.manifest.language, "zh");
        assert_eq!(snap.manifest.last_applied_chapter, 1);
        assert_eq!(snap.chapter_summaries.rows.len(), 1);
        assert_eq!(snap.hooks.hooks.len(), 1);
        assert_eq!(snap.hooks.hooks[0].hook_id, "h01");
        assert_eq!(snap.current_state.facts.len(), 1);
    }

    #[tokio::test]
    async fn load_runtime_state_snapshot_self_heals_invalid_persisted_state() {
        // bootstrap 是幂等的：损坏的 hooks.json 会被从 markdown 重建（warning 标记），
        // 随后 load 成功——而非失败。这是设计语义（对齐 TS loadRuntimeStateSnapshot 先 bootstrap）。
        let store = InMemoryStateStore::new();
        setup_min_book(&store, "book");
        load_runtime_state_snapshot(&store, "book").await.unwrap();
        // 破坏 hooks.json。
        store.set("book/story/state/hooks.json", "not json");
        let snap = load_runtime_state_snapshot(&store, "book").await.unwrap();
        // bootstrap 自愈：从 pending_hooks.md 重建，hooks 仍为 1 条 h01。
        assert_eq!(snap.hooks.hooks.len(), 1);
        assert_eq!(snap.hooks.hooks[0].hook_id, "h01");
    }

    #[tokio::test]
    async fn build_runtime_state_artifacts_applies_delta_and_renders_projections() {
        let store = InMemoryStateStore::new();
        setup_min_book(&store, "book");
        let delta = RuntimeStateDelta {
            chapter: 2,
            hook_ops: HookOps::default(),
            new_hook_candidates: Vec::new(),
            chapter_summary: None,
            current_state_patch: None,
            subplot_ops: Vec::new(),
            emotional_arc_ops: Vec::new(),
            character_matrix_ops: Vec::new(),
            notes: Vec::new(),
        };
        let artifacts =
            build_runtime_state_artifacts(&store, "book", &delta, WritingLanguage::Zh, None, None)
                .await
                .unwrap();
        // 第 2 章，reducer 推进；投影非空。
        assert_eq!(artifacts.snapshot.manifest.last_applied_chapter, 2);
        assert!(!artifacts.current_state_markdown.is_empty());
        assert!(!artifacts.hooks_markdown.is_empty());
        assert!(!artifacts.chapter_summaries_markdown.is_empty());
    }

    #[tokio::test]
    async fn save_runtime_state_snapshot_writes_four_files() {
        let store = InMemoryStateStore::new();
        setup_min_book(&store, "book");
        let snap = load_runtime_state_snapshot(&store, "book").await.unwrap();
        // 修改快照并保存。
        let mut next = snap.clone();
        next.manifest.last_applied_chapter = 9;
        save_runtime_state_snapshot(&store, "book", &next).await.unwrap();
        // 直接读 manifest.json 验证写入（不经 bootstrap，因 bootstrap 会按 durable progress 重算）。
        let written = store
            .read_to_string("book/story/state/manifest.json")
            .await
            .unwrap()
            .unwrap();
        assert!(written.contains("\"lastAppliedChapter\": 9"));
        // 4 个文件均存在。
        for name in ["current_state.json", "hooks.json", "chapter_summaries.json"] {
            assert!(store
                .read_to_string(&format!("book/story/state/{name}"))
                .await
                .unwrap()
                .is_some());
        }
    }

    #[tokio::test]
    async fn load_narrative_memory_seed_maps_to_memory_db_types() {
        use crate::state::memory_db::{StoredHook, StoredSummary};
        let store = InMemoryStateStore::new();
        setup_min_book(&store, "book");
        let seed = load_narrative_memory_seed(&store, "book").await.unwrap();
        assert_eq!(seed.summaries.len(), 1);
        assert_eq!(seed.summaries[0], StoredSummary {
            chapter: 1,
            title: "t1".into(),
            characters: "角色".into(),
            events: "事件".into(),
            state_changes: "变化".into(),
            hook_activity: "活动".into(),
            mood: "心情".into(),
            chapter_type: "类型".into(),
            conflict_level: None,
            reveal_level: None,
        });
        assert_eq!(seed.hooks.len(), 1);
        assert_eq!(seed.hooks[0].hook_id, "h01");
        assert_eq!(seed.hooks[0].status, "open");
        assert_eq!(seed.hooks[0].payoff_timing, "mid-arc");
        let _ = StoredHook { hook_id: "x".into(), start_chapter: 0, r#type: "t".into(), status: "open".into(), last_advanced_chapter: 0, expected_payoff: String::new(), payoff_timing: String::new(), notes: String::new() };
        let _ = HookStatus::Open;
    }
}
