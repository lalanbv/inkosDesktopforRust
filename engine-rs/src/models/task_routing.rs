//! 按任务模型路由（G16/345 号，Phase B 批次三末项）。
//!
//! TS 真源：`packages/core/src/models/task-routing.ts`；共享向量：
//! `packages/core/src/__tests__/golden/task-routing-vectors.json`
//! （差分测试 `tests/golden_task_routing_diff.rs`，解析一致 duel）。
//!
//! 五类任务（writing/review/repair/detect/analysis）逐字段三层回退：
//! book.tasks[task] > book.defaults > project.tasks[task] > project.defaults
//! > fallback（现有全局解析产物）。迁移兼容：routing 缺省全 fallback。
//!
//! R25/397 号失败接管链：override 增可选 retryCount（1–5）与 backupModels
//! （≤3）；[`resolve_task_model_chain`] 产出去重封顶（总长 ≤4）的尝试链，
//! service 只随 primary。零配置 → [primary] + retryCount=1（现行为）。

use serde::Deserialize;
use serde::Serialize;

pub const TASK_MODEL_KINDS: [&str; 5] = ["writing", "review", "repair", "detect", "analysis"];

/// agent 名 → 任务类型（管线接线口径；detect 走外部检测服务不在映射内）。
pub fn agent_task(agent: &str) -> Option<TaskModelKind> {
    match agent {
        "writer" | "planner" | "architect" => Some(TaskModelKind::Writing),
        "continuity-auditor" | "consolidator" | "state-validator" => Some(TaskModelKind::Review),
        "reviser" => Some(TaskModelKind::Repair),
        "radar" | "chapter-analyzer" => Some(TaskModelKind::Analysis),
        _ => None,
    }
}

/// G16/346 号：agent 级便捷解析——路由给出 model 覆盖时返回新 model，否则 None。
pub fn resolve_agent_model(
    agent: &str,
    routing: Option<&TaskModelRouting>,
    fallback_model: &str,
) -> Option<String> {
    let task = agent_task(agent)?;
    let routing = routing?;
    let resolved = resolve_task_model(ResolveTaskModelParams {
        task,
        book_routing: None,
        project_routing: Some(routing),
        fallback_model: fallback_model.to_string(),
        fallback_service: None,
        fallback_temperature: None,
        fallback_max_tokens: None,
    });
    if resolved.model != fallback_model {
        Some(resolved.model)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskModelKind {
    Writing,
    Review,
    Repair,
    Detect,
    Analysis,
}

impl TaskModelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskModelKind::Writing => "writing",
            TaskModelKind::Review => "review",
            TaskModelKind::Repair => "repair",
            TaskModelKind::Detect => "detect",
            TaskModelKind::Analysis => "analysis",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskModelOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "maxTokens")]
    pub max_tokens: Option<u32>,
    /// R25：当前模型单次尝试内重试次数（1–5；缺省 1 = 不重试）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_count: Option<u32>,
    /// R25：备用模型按序接管（≤3；与主模型同端点，链条目仅以 model 标识）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_models: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskModelRouting {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<TaskModelOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<std::collections::BTreeMap<String, TaskModelOverride>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingFieldSourceEntry {
    pub field: &'static str,
    pub source: RoutingFieldSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RoutingFieldSource {
    Task,
    Defaults,
    Fallback,
}

impl RoutingFieldSource {
    pub fn as_str(self) -> &'static str {
        match self {
            RoutingFieldSource::Task => "task",
            RoutingFieldSource::Defaults => "defaults",
            RoutingFieldSource::Fallback => "fallback",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTaskModel {
    pub task: &'static str,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "maxTokens")]
    pub max_tokens: Option<u32>,
    pub sources: Vec<RoutingFieldSourceEntry>,
}


/// 逐字段合并：project.defaults → project.tasks[t] → book.defaults → book.tasks[t]，
/// 后者覆盖前者；同时产出每字段来源。
fn compose_task_override(
    book: Option<&TaskModelRouting>,
    project: Option<&TaskModelRouting>,
    task: TaskModelKind,
) -> (TaskModelOverride, Vec<RoutingFieldSourceEntry>) {
    const FIELDS: [&str; 4] = ["model", "service", "temperature", "maxTokens"];
    let layers: [Option<&TaskModelOverride>; 4] = [
        project.and_then(|r| r.defaults.as_ref()),
        project
            .and_then(|r| r.tasks.as_ref())
            .and_then(|tasks| tasks.get(task.as_str())),
        book.and_then(|r| r.defaults.as_ref()),
        book.and_then(|r| r.tasks.as_ref())
            .and_then(|tasks| tasks.get(task.as_str())),
    ];
    let field_present = |layer: Option<&TaskModelOverride>, field: &str| -> bool {
        let Some(o) = layer else { return false };
        match field {
            "model" => o.model.is_some(),
            "service" => o.service.is_some(),
            "temperature" => o.temperature.is_some(),
            "maxTokens" => o.max_tokens.is_some(),
            _ => false,
        }
    };
    let sources: Vec<RoutingFieldSourceEntry> = FIELDS
        .iter()
        .map(|field| {
            let source = if field_present(layers[3], field) {
                RoutingFieldSource::Task
            } else if field_present(layers[2], field) {
                RoutingFieldSource::Defaults
            } else if field_present(layers[1], field) {
                RoutingFieldSource::Task
            } else if field_present(layers[0], field) {
                RoutingFieldSource::Defaults
            } else {
                RoutingFieldSource::Fallback
            };
            RoutingFieldSourceEntry { field, source }
        })
        .collect();

    let mut merged = TaskModelOverride::default();
    for layer in layers.iter().flatten() {
        if layer.model.is_some() {
            merged.model = layer.model.clone();
        }
        if layer.service.is_some() {
            merged.service = layer.service.clone();
        }
        if let Some(t) = layer.temperature.filter(|t| t.is_finite()) {
            merged.temperature = Some(t);
        }
        if let Some(t) = layer.max_tokens {
            merged.max_tokens = Some(t);
        }
        if let Some(t) = layer.retry_count.filter(|t| *t >= 1) {
            merged.retry_count = Some(t);
        }
        if let Some(models) = layer.backup_models.as_ref() {
            let models: Vec<String> =
                models.iter().filter(|m| !m.is_empty()).cloned().collect();
            if !models.is_empty() {
                merged.backup_models = Some(models);
            }
        }
    }
    (merged, sources)
}

/// [`resolve_task_model`] 参数。
pub struct ResolveTaskModelParams<'a> {
    pub task: TaskModelKind,
    pub book_routing: Option<&'a TaskModelRouting>,
    pub project_routing: Option<&'a TaskModelRouting>,
    pub fallback_model: String,
    pub fallback_service: Option<String>,
    pub fallback_temperature: Option<f64>,
    pub fallback_max_tokens: Option<u32>,
}

/// 任务模型解析（逐字段三层回退）。迁移兼容：routing 缺省全 fallback。
pub fn resolve_task_model(params: ResolveTaskModelParams<'_>) -> ResolvedTaskModel {
    let (override_value, sources) = compose_task_override(
        params.book_routing,
        params.project_routing,
        params.task,
    );
    let model = override_value
        .model
        .clone()
        .unwrap_or_else(|| params.fallback_model.clone());
    let service = override_value
        .service
        .clone()
        .or_else(|| params.fallback_service.clone());
    let temperature = override_value.temperature.or(params.fallback_temperature);
    let max_tokens = override_value.max_tokens.or(params.fallback_max_tokens);
    ResolvedTaskModel {
        task: params.task.as_str(),
        model,
        service,
        temperature,
        max_tokens,
        sources,
    }
}

/// 尝试链总长封顶：1 个 primary + 至多 3 个备用（与 backupModels 上限一致）。
pub const TASK_MODEL_CHAIN_MAX_ATTEMPTS: u32 = 4;
/// retryCount 缺省：零配置 = 单次尝试（现行为）。
pub const TASK_MODEL_CHAIN_DEFAULT_RETRY_COUNT: u32 = 1;

/// 链条目（R25）：primary 含生效 service；备用仅 model（同端点运行）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskModelAttempt {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

/// [`resolve_task_model_chain`] 产物（R25）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTaskModelChain {
    pub task: &'static str,
    pub attempts: Vec<TaskModelAttempt>,
    #[serde(rename = "retryCount")]
    pub retry_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "maxTokens")]
    pub max_tokens: Option<u32>,
}

/// 任务模型**尝试链**解析（R25）：primary 走 [`resolve_task_model`] 同款逐字段
/// 回退；override 的 backupModels 按序追加——与链中已有 model 精确重复者剔除，
/// 总长封顶 [`TASK_MODEL_CHAIN_MAX_ATTEMPTS`]。零配置 → [primary] + 1。
pub fn resolve_task_model_chain(params: ResolveTaskModelParams<'_>) -> ResolvedTaskModelChain {
    let resolved = resolve_task_model(ResolveTaskModelParams {
        task: params.task,
        book_routing: params.book_routing,
        project_routing: params.project_routing,
        fallback_model: params.fallback_model.clone(),
        fallback_service: params.fallback_service.clone(),
        fallback_temperature: params.fallback_temperature,
        fallback_max_tokens: params.fallback_max_tokens,
    });
    let mut attempts = vec![TaskModelAttempt {
        model: resolved.model.clone(),
        service: resolved.service.clone(),
    }];
    let mut seen: Vec<&str> = vec![resolved.model.as_str()];
    let (override_value, _) = compose_task_override(params.book_routing, params.project_routing, params.task);
    for backup in override_value.backup_models.iter().flatten() {
        if attempts.len() as u32 >= TASK_MODEL_CHAIN_MAX_ATTEMPTS {
            break;
        }
        if seen.contains(&backup.as_str()) {
            continue;
        }
        seen.push(backup.as_str());
        attempts.push(TaskModelAttempt {
            model: backup.clone(),
            service: None,
        });
    }
    ResolvedTaskModelChain {
        task: params.task.as_str(),
        attempts,
        retry_count: override_value
            .retry_count
            .unwrap_or(TASK_MODEL_CHAIN_DEFAULT_RETRY_COUNT),
        temperature: resolved.temperature,
        max_tokens: resolved.max_tokens,
    }
}

/// 机器可读契约（双端 golden 锁形状）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRoutingContract {
    pub kinds: Vec<&'static str>,
    pub fields: Vec<&'static str>,
    pub levels: Vec<&'static str>,
    #[serde(rename = "migrationCompat")]
    pub migration_compat: &'static str,
    pub chain: TaskChainContract,
}

/// R25：接管链契约面。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskChainContract {
    pub chain_fields: Vec<&'static str>,
    pub max_attempts: u32,
    pub max_backup_models: u32,
    pub retry_count_range: Vec<u32>,
    pub default_retry_count: u32,
    pub dedupe: &'static str,
}

pub fn task_routing_contract() -> TaskRoutingContract {
    TaskRoutingContract {
        kinds: TASK_MODEL_KINDS.to_vec(),
        fields: vec!["model", "service", "temperature", "maxTokens"],
        levels: vec![
            "book-task",
            "book-defaults",
            "project-task",
            "project-defaults",
            "fallback",
        ],
        migration_compat: "absent-routing-resolves-to-fallback",
        chain: TaskChainContract {
            chain_fields: vec!["retryCount", "backupModels"],
            max_attempts: TASK_MODEL_CHAIN_MAX_ATTEMPTS,
            max_backup_models: 3,
            retry_count_range: vec![1, 5],
            default_retry_count: TASK_MODEL_CHAIN_DEFAULT_RETRY_COUNT,
            dedupe: "exact-match-against-existing-attempts",
        },
    }
}
