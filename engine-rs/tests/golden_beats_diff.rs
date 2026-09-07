//! 212 号：节拍沉淀纯函数域共享 golden 差分——与 core
//! `__tests__/golden-beats.test.ts` 读同一份 `beats-vectors.json` 逐例断言
//! （双端「逐字对齐」（191 号声明）由差分守门；向量即契约，任一侧改动
//! 漂移即红）。

use std::collections::HashSet;

const VECTORS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../packages/core/src/__tests__/golden/beats-vectors.json"
));

#[test]
fn beats_parse_matches_shared_golden_vectors() {
    let vectors: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
    for tc in vectors["parse"].as_array().unwrap() {
        let name = tc["name"].as_str().unwrap();
        let text = tc["text"].as_str().unwrap();
        let valid_ids: HashSet<String> = tc["validIds"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let result = inkos_engine::agents::timeline_settler::parse_beats_json(text, &valid_ids);
        if let Some(err_contains) = tc["errorContains"].as_str() {
            let err = result.expect_err(&format!("{name}: 应报错"));
            assert!(err.contains(err_contains), "{name}: 错误「{err}」应含「{err_contains}」");
            continue;
        }
        let beats = result.unwrap_or_else(|e| panic!("{name}: 不应报错：{e}"));
        // PlotlineBeat → JSON 归一（None 字段缺席）后与期望比对。
        let got: Vec<serde_json::Value> = beats
            .iter()
            .map(|b| {
                serde_json::json!({
                    "plotlineId": b.plotline_id,
                    "title": b.title,
                    "note": b.note,
                })
            })
            .collect();
        // None（Rust）/undefined（TS）统一归一为缺席键后比对。
        let expected = normalize_nulls(&tc["expected"]);
        assert_eq!(normalize_nulls(&serde_json::json!(got)), expected, "{name}: parse 结果漂移");
    }
}

#[test]
fn beats_merge_matches_shared_golden_vectors() {
    let vectors: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
    for tc in vectors["merge"].as_array().unwrap() {
        let name = tc["name"].as_str().unwrap();
        let mut timeline: inkos_engine::models::timeline::Timeline =
            serde_json::from_value(tc["timeline"].clone()).unwrap();
        let old_updated_at = timeline.updated_at.clone();
        let chapter = tc["chapter"].as_u64().unwrap() as u32;
        let beats: Vec<inkos_engine::models::timeline::PlotlineBeat> = tc["beats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| inkos_engine::models::timeline::PlotlineBeat {
                plotline_id: b["plotlineId"].as_str().unwrap().to_string(),
                title: b["title"].as_str().map(str::to_string),
                note: b["note"].as_str().map(str::to_string),
            })
            .collect();
        let applied =
            inkos_engine::models::timeline::merge_chapter_beats(&mut timeline, chapter, &beats);
        assert_eq!(
            applied, 
            tc["expectedApplied"].as_u64().unwrap() as usize,
            "{name}: applied 计数漂移"
        );
        // updatedAt 非确定：断言刷新语义。
        if let Some(expected_at) = tc["expectedUpdatedAt"].as_str() {
            assert_eq!(timeline.updated_at, expected_at, "{name}: 未落格不应刷新 updatedAt");
        } else if applied > 0 {
            assert_ne!(timeline.updated_at, old_updated_at, "{name}: 落格后应刷新 updatedAt");
        }
        let plotlines_json = serde_json::to_value(&timeline.plotlines).unwrap();
        assert_eq!(
            normalize_nulls(&plotlines_json),
            normalize_nulls(&tc["expectedPlotlines"]),
            "{name}: merge 结果漂移"
        );
    }
}

/// 期望侧（TS undefined → JSON 缺席）与实际侧（Rust None → serde skip 缺席）
/// 统一：无 null 值即可直接比对；此函数防御性地把 null 剥成缺席键。
fn normalize_nulls(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let out: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), normalize_nulls(v)))
                .collect();
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(normalize_nulls).collect())
        }
        other => other.clone(),
    }
}
