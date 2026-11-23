//! R55/1660 号：用量账本与预算投影 golden 差分（TS 生产向量 → Rust 镜像
//! 对拍）——投影（日/月窗口+多服务隔离）/预算级别/确认门（软关+无配置
//! 负路径）/月窗口更紧。

use inkos_engine::utils::usage_ledger::{
    budget_status, usage_projection, ServiceBudget, UsageLogRow,
};
use std::collections::HashMap;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/usage-budget-vectors.json");

#[derive(serde::Deserialize)]
struct VectorFile {
    rows: Vec<UsageLogRow>,
    #[serde(rename = "dayKeys")]
    day_keys: Vec<String>,
    #[serde(rename = "todayKey")]
    today_key: String,
    projection: Vec<ProjectionEntry>,
    statuses: Vec<StatusVector>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectionEntry {
    service_key: String,
    daily: u64,
    monthly: u64,
    total: u64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusVector {
    name: String,
    usage: UsageInput,
    budget: Option<BudgetInput>,
    status: ExpectedStatus,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageInput {
    daily: u64,
    monthly: u64,
    total: u64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BudgetInput {
    daily_token_budget: Option<u64>,
    monthly_token_budget: Option<u64>,
    #[serde(default = "default_true")]
    confirm_at_budget: bool,
}

fn default_true() -> bool {
    true
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedStatus {
    service_key: String,
    daily: u64,
    monthly: u64,
    ratio: f64,
    level: String,
    confirm_required: bool,
}

#[test]
fn usage_budget_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(file.statuses.len() >= 5, "golden 非空防空转");

    // 投影。
    let day_by_ts: HashMap<&str, &str> = file
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| (row.ts.as_str(), file.day_keys[index].as_str()))
        .collect();
    let projection = usage_projection(
        &file.rows,
        |row| {
            day_by_ts
                .get(row.ts.as_str())
                .copied()
                .unwrap_or_default()
                .to_string()
        },
        &file.today_key,
    );
    assert_eq!(projection.len(), file.projection.len(), "服务数漂移");
    for expected in &file.projection {
        let actual = projection
            .get(&expected.service_key)
            .unwrap_or_else(|| panic!("服务 {} 缺失", expected.service_key));
        assert_eq!(
            (actual.daily, actual.monthly, actual.total),
            (expected.daily, expected.monthly, expected.total),
            "{} 投影漂移",
            expected.service_key
        );
    }

    // 预算判定。
    for vector in &file.statuses {
        let budget = vector.budget.as_ref().map(|input| ServiceBudget {
            daily_token_budget: input.daily_token_budget,
            monthly_token_budget: input.monthly_token_budget,
            confirm_at_budget: input.confirm_at_budget,
        });
        let usage = inkos_engine::utils::usage_ledger::ServiceUsage {
            daily: vector.usage.daily,
            monthly: vector.usage.monthly,
            total: vector.usage.total,
        };
        let status = budget_status(&vector.name, &usage, budget.as_ref());
        // 1663 号：断言宽度补齐到 BudgetStatus 契约全六键——此前
        // daily/monthly 反序列化后从不比较（clippy 死字段），serviceKey 未核。
        assert_eq!(
            (status.service_key.as_str(), status.daily, status.monthly),
            (
                vector.status.service_key.as_str(),
                vector.status.daily,
                vector.status.monthly
            ),
            "{} 服务键/窗口用量漂移",
            vector.name
        );
        let level = match status.level {
            inkos_engine::utils::usage_ledger::BudgetLevel::Ok => "ok",
            inkos_engine::utils::usage_ledger::BudgetLevel::Warn => "warn",
            inkos_engine::utils::usage_ledger::BudgetLevel::Over => "over",
        };
        assert_eq!(level, vector.status.level, "{} 级别漂移", vector.name);
        assert_eq!(
            status.ratio, vector.status.ratio,
            "{} 比例漂移",
            vector.name
        );
        assert_eq!(
            status.confirm_required, vector.status.confirm_required,
            "{} 确认门漂移",
            vector.name
        );
    }
}
