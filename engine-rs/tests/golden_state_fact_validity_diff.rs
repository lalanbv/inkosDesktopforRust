//! R65 第一步：快照事实有效期语义共享 golden 差分（双腿同一事实源）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/state-fact-validity.json`。
//! - `activeAt`：`CurrentStateFact::is_active_at` 边界表（与 TS `isFactActiveAt`
//!   同语义：`validUntil === null || validUntil >= chapter`）。
//! - `supersede`：Rust 侧以 `CurrentStatePatch` 驱动 `apply_runtime_state_delta`，
//!   TS 侧（`state-fact-validity.test.ts`）以 `factOps.upsert` 驱动
//!   `applyRuntimeStateDelta`——两驱动同语义（活跃同谓词旧事实置失效于
//!   chapter-1、精确同值活跃事实整跳过、已失效历史行不重触、异槽位不牵连）。
//!   expectedFacts 双侧各自按 [predicate, object] 码点序规范化后逐字段比对
//!   （各腿落盘顺序归各自 reducer 排序管，本差分不钉顺序）。

use inkos_engine::models::runtime_state::{
    ChapterSummariesState, CurrentStateFact, CurrentStatePatch, CurrentStateState, HooksState,
    RuntimeStateDelta, StateManifest,
};
use inkos_engine::state::reducer::{apply_runtime_state_delta, RuntimeStateSnapshot};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/state-fact-validity.json");

fn fact_from_json(v: &Value) -> CurrentStateFact {
    CurrentStateFact {
        subject: v["subject"].as_str().expect("subject").into(),
        predicate: v["predicate"].as_str().expect("predicate").into(),
        object: v["object"].as_str().expect("object").into(),
        valid_from_chapter: v["validFromChapter"].as_u64().expect("validFrom") as u32,
        valid_until_chapter: v["validUntilChapter"].as_u64().map(|n| n as u32),
        source_chapter: v["sourceChapter"].as_u64().expect("source") as u32,
    }
}

fn canonical_sorted(items: Vec<Value>) -> Vec<Value> {
    let key = |v: &Value| {
        (
            v["predicate"].as_str().unwrap_or("").to_string(),
            v["object"].as_str().unwrap_or("").to_string(),
        )
    };
    let mut sorted = items;
    sorted.sort_by_key(key);
    sorted
}

#[test]
fn is_active_at_matches_shared_boundary_table() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["activeAt"].as_array().expect("activeAt array") {
        let name = vector["name"].as_str().unwrap();
        let fact = CurrentStateFact {
            subject: "protagonist".into(),
            predicate: "当前位置".into(),
            object: "值".into(),
            valid_from_chapter: 0,
            valid_until_chapter: vector["validUntil"].as_u64().map(|n| n as u32),
            source_chapter: 0,
        };
        let chapter = vector["queryChapter"].as_u64().unwrap() as u32;
        assert_eq!(
            fact.is_active_at(chapter),
            vector["active"].as_bool().unwrap(),
            "activeAt vector '{name}' drifted"
        );
    }
}

#[test]
fn supersede_matches_shared_semantics_table() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for case in vectors["supersede"].as_array().expect("supersede array") {
        let name = case["name"].as_str().unwrap();
        let chapter = case["chapter"].as_u64().unwrap() as u32;
        let snapshot = RuntimeStateSnapshot {
            manifest: StateManifest {
                schema_version: 2,
                language: case["language"].as_str().unwrap().into(),
                last_applied_chapter: 0,
                projection_version: 1,
                migration_warnings: vec![],
            },
            current_state: CurrentStateState {
                chapter: chapter.saturating_sub(1),
                facts: case["inputFacts"]
                    .as_array()
                    .expect("inputFacts")
                    .iter()
                    .map(fact_from_json)
                    .collect(),
            },
            hooks: HooksState { hooks: vec![] },
            chapter_summaries: ChapterSummariesState { rows: vec![] },
        };
        let patch = match case["rustPatch"]["field"].as_str().unwrap() {
            "currentLocation" => CurrentStatePatch {
                current_location: Some(case["rustPatch"]["value"].as_str().unwrap().into()),
                ..Default::default()
            },
            other => panic!(
                "golden case '{name}': 未接线的 patch 字段 '{other}'（扩展向量时同登记 Rust 驱动）"
            ),
        };
        let delta = RuntimeStateDelta {
            chapter,
            current_state_patch: Some(patch),
            ..Default::default()
        };
        let next = apply_runtime_state_delta(&snapshot, &delta, None)
            .unwrap_or_else(|e| panic!("golden case '{name}': delta 被拒：{e:?}"));
        let actual = serde_json::to_value(&next.current_state.facts).unwrap();
        let expected = case["expectedFacts"].clone();
        assert_eq!(
            canonical_sorted(actual.as_array().expect("facts array").clone()),
            canonical_sorted(expected.as_array().expect("expectedFacts").clone()),
            "supersede case '{name}' drifted"
        );
    }
}
