//! 345 号：按任务模型路由共享 golden 差分（G16，批次三末项）——解析一致 duel。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/task-routing-vectors.json`；
//! core 侧 `src/__tests__/golden-task-routing.test.ts` 断言同文件。
//! 三组差分：解析决策表（逐字段回退 + 来源标注）、迁移兼容、契约形状。

use inkos_engine::models::task_routing::{
    resolve_task_model, task_routing_contract, ResolveTaskModelParams, RoutingFieldSource,
    TaskModelKind, TaskModelRouting,
};
use serde::Deserialize;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/task-routing-vectors.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskRoutingInput {
    task: TaskModelKind,
    #[serde(default)]
    book_routing: Option<TaskModelRouting>,
    #[serde(default)]
    project_routing: Option<TaskModelRouting>,
}

fn fallback_of(vectors: &Value) -> (String, String, f64, u32) {
    let f = &vectors["fallback"];
    (
        f["model"].as_str().unwrap().to_string(),
        f["service"].as_str().unwrap().to_string(),
        f["temperature"].as_f64().unwrap(),
        f["maxTokens"].as_u64().unwrap() as u32,
    )
}

#[test]
fn decision_table_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let (model, service, temperature, max_tokens) = fallback_of(&vectors);
    for vector in vectors["resolve"].as_array().expect("resolve array") {
        let input: TaskRoutingInput = serde_json::from_value(vector["input"].clone()).unwrap();
        let got = resolve_task_model(ResolveTaskModelParams {
            task: input.task,
            book_routing: input.book_routing.as_ref(),
            project_routing: input.project_routing.as_ref(),
            fallback_model: model.clone(),
            fallback_service: Some(service.clone()),
            fallback_temperature: Some(temperature),
            fallback_max_tokens: Some(max_tokens),
        });
        let got_value = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_value,
            vector["expected"],
            "resolve vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn migration_compat_absent_routing_resolves_to_fallback() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let (model, service, temperature, max_tokens) = fallback_of(&vectors);
    // 五类任务全部回退 fallback（迁移兼容口径）。
    let kinds = [
        TaskModelKind::Writing,
        TaskModelKind::Review,
        TaskModelKind::Repair,
        TaskModelKind::Detect,
        TaskModelKind::Analysis,
    ];
    for kind in kinds {
        let got = resolve_task_model(ResolveTaskModelParams {
            task: kind,
            book_routing: None,
            project_routing: None,
            fallback_model: model.clone(),
            fallback_service: Some(service.clone()),
            fallback_temperature: Some(temperature),
            fallback_max_tokens: Some(max_tokens),
        });
        assert_eq!(got.model, model);
        assert!(
            got.sources.iter().all(|entry| entry.source == RoutingFieldSource::Fallback),
            "all fields expected fallback"
        );
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(task_routing_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}