//! R55/1660 号：per-service 用量账本与预算投影（TS 镜像 =
//! packages/core/src/utils/usage-budget.ts，golden usage-budget-vectors.json
//! 锁定）。
//!
//! - 账本 = `{root}/.inkos/usage-log.jsonl` append-only（行 `{ts, serviceKey,
//!   model, promptTokens, completionTokens, totalTokens}`——**usage 精确值**
//!   （1647 隐私红线：非 TokenMeter 估算）；ts=UTC ISO，dayKey 由消费端
//!   计算后入参纯函数（R51 惯例）。
//! - 预算 = services 配置可选键 `dailyTokenBudget`/`monthlyTokenBudget`
//!   （软顶，无计费/硬断语义）；级别：ok <80%、warn ≥80%、over ≥100%。
//! - 写点：管线 persist（token_usage 精确值）经 PersistenceHooks
//!   `append_usage_log`（1654 同款默认 no-op 模式）；chat 流水写点挂下轮
//!   （task-local root 通道专项）。

use std::collections::HashMap;
use std::path::PathBuf;

/// 账本行（serde camelCase；TS `UsageLogRow` 镜像）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLogRow {
    pub ts: String,
    pub service_key: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// 单服务预算软顶（services 配置可选键；None=不限）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceBudget {
    pub daily_token_budget: Option<u64>,
    pub monthly_token_budget: Option<u64>,
    /// 100% 发送前确认门（可关闭=仅告警；默认开）。
    #[serde(default = "default_confirm")]
    pub confirm_at_budget: bool,
}

fn default_confirm() -> bool {
    true
}

fn usage_ledger_path(root: &str) -> PathBuf {
    std::path::Path::new(root)
        .join(".inkos")
        .join("usage-log.jsonl")
}

/// 读账本（无文件=空；坏行跳过——jsonl 宽容读）。
pub async fn load_usage_ledger(root: &str) -> Vec<UsageLogRow> {
    match tokio::fs::read_to_string(usage_ledger_path(root)).await {
        Ok(text) => text
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    serde_json::from_str(trimmed).ok()
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 追加账本行（读旧+拼+原子写；best-effort 失败日志吞）。
pub async fn append_usage_rows(root: &str, rows: &[UsageLogRow]) {
    if rows.is_empty() {
        return;
    }
    let mut lines: Vec<String> = load_usage_ledger(root)
        .await
        .iter()
        .map(|row| serde_json::to_string(row).unwrap_or_default())
        .collect();
    for row in rows {
        lines.push(serde_json::to_string(row).unwrap_or_default());
    }
    let path = usage_ledger_path(root);
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let text = lines.join("\n") + "\n";
    if let Err(error) = crate::utils::atomic_file_set::write_file_atomic(&path, &text).await {
        eprintln!("[usage-ledger] append failed (ignored): {error}");
    }
}

/// 服务用量投影（TS `usageProjection` 镜像）：dayKeyOf 入参映射 + 今日
/// dayKey → per-service {daily, monthly, total}（月窗口=今日同月）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceUsage {
    pub daily: u64,
    pub monthly: u64,
    pub total: u64,
}

pub fn usage_projection(
    rows: &[UsageLogRow],
    day_key_of: impl Fn(&UsageLogRow) -> String,
    today_key: &str,
) -> HashMap<String, ServiceUsage> {
    let today_month = today_key.get(..7).unwrap_or_default().to_string();
    let mut out: HashMap<String, ServiceUsage> = HashMap::new();
    for row in rows {
        let day_key = day_key_of(row);
        let entry = out.entry(row.service_key.clone()).or_insert(ServiceUsage {
            daily: 0,
            monthly: 0,
            total: 0,
        });
        entry.total += row.total_tokens;
        if day_key == today_key {
            entry.daily += row.total_tokens;
        }
        if day_key.get(..7).unwrap_or_default() == today_month {
            entry.monthly += row.total_tokens;
        }
    }
    out
}

/// 预算级别（ok <80%、warn ≥80%、over ≥100%；无预算=ok 不确认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BudgetLevel {
    Ok,
    Warn,
    Over,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetStatus {
    pub service_key: String,
    pub daily: u64,
    pub monthly: u64,
    /// 日/月中更紧的一档比例（0-1，无预算=0）。
    pub ratio: f64,
    pub level: BudgetLevel,
    /// 确认门（level=over 且 confirm_at_budget；无配置不发确认=负路径）。
    pub confirm_required: bool,
}

/// 预算判定（TS `budgetStatus` 镜像）：无预算键=ok/不确认（v8「确认门
/// 负路径（无配置不发确认）」）。
pub fn budget_status(
    service_key: &str,
    usage: &ServiceUsage,
    budget: Option<&ServiceBudget>,
) -> BudgetStatus {
    let (ratio, level) = match budget {
        None => (0.0, BudgetLevel::Ok),
        Some(budget) => {
            let day_ratio = budget
                .daily_token_budget
                .map(|cap| usage.daily as f64 / cap as f64);
            let month_ratio = budget
                .monthly_token_budget
                .map(|cap| usage.monthly as f64 / cap as f64);
            let worst = day_ratio
                .iter()
                .chain(month_ratio.iter())
                .cloned()
                .fold(0.0f64, f64::max);
            let level = if worst >= 1.0 {
                BudgetLevel::Over
            } else if worst >= 0.8 {
                BudgetLevel::Warn
            } else {
                BudgetLevel::Ok
            };
            (worst, level)
        }
    };
    BudgetStatus {
        service_key: service_key.to_string(),
        daily: usage.daily,
        monthly: usage.monthly,
        ratio: (ratio * 100.0).round() / 100.0,
        level,
        confirm_required: matches!(level, BudgetLevel::Over)
            && budget.map(|b| b.confirm_at_budget).unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ts: &str, service: &str, total: u64) -> UsageLogRow {
        UsageLogRow {
            ts: ts.to_string(),
            service_key: service.to_string(),
            model: "m".to_string(),
            prompt_tokens: total / 2,
            completion_tokens: total / 2,
            total_tokens: total,
        }
    }

    #[test]
    fn projection_day_month_window_and_isolation() {
        let rows = vec![
            row("2026-11-23T01:00:00.000Z", "a", 100),
            row("2026-11-23T05:00:00.000Z", "a", 50),
            row("2026-11-22T01:00:00.000Z", "a", 30), // 昨日：月内日外
            row("2026-10-30T01:00:00.000Z", "a", 10), // 月外
            row("2026-11-23T02:00:00.000Z", "b", 7),  // 服务隔离
        ];
        let by_ts: HashMap<&str, &str> = HashMap::new();
        let _ = by_ts;
        let projection = usage_projection(
            &rows,
            |r| r.ts.get(..10).unwrap_or("").to_string(),
            "2026-11-23",
        );
        let a = &projection["a"];
        assert_eq!(a.daily, 150);
        assert_eq!(a.monthly, 180);
        assert_eq!(a.total, 190);
        let b = &projection["b"];
        assert_eq!(b.daily, 7);
        assert_eq!(b.total, 7);
    }

    #[test]
    fn budget_levels_and_confirm_gate() {
        let usage = ServiceUsage {
            daily: 850,
            monthly: 0,
            total: 850,
        };
        // 日顶 1000：85% warn。
        let budget = ServiceBudget {
            daily_token_budget: Some(1000),
            monthly_token_budget: None,
            confirm_at_budget: true,
        };
        let status = budget_status("a", &usage, Some(&budget));
        assert_eq!(status.level, BudgetLevel::Warn);
        assert!(!status.confirm_required);
        // 1100/1000=over+确认。
        let over = ServiceUsage {
            daily: 1100,
            monthly: 0,
            total: 1100,
        };
        let status = budget_status("a", &over, Some(&budget));
        assert_eq!(status.level, BudgetLevel::Over);
        assert!(status.confirm_required);
        // 可关闭=仅告警（confirm_at_budget=false → over 不确认）。
        let soft = ServiceBudget {
            daily_token_budget: Some(1000),
            monthly_token_budget: None,
            confirm_at_budget: false,
        };
        let status = budget_status("a", &over, Some(&soft));
        assert_eq!(status.level, BudgetLevel::Over);
        assert!(!status.confirm_required);
        // 无配置不发确认（负路径）。
        let status = budget_status("a", &over, None);
        assert_eq!(status.level, BudgetLevel::Ok);
        assert!(!status.confirm_required);
    }

    #[tokio::test]
    async fn ledger_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        assert!(load_usage_ledger(&root).await.is_empty());
        append_usage_rows(&root, &[row("2026-11-23T00:00:00.000Z", "a", 42)]).await;
        let rows = load_usage_ledger(&root).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_tokens, 42);
    }
}
