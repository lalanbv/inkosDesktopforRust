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

/// baseUrl → 账本 serviceKey：host（**不含端口**），不可解析/无 host 时回退
/// 原值（TS `serviceHostOf` 镜像，golden `serviceHosts` 向量锁定）。
///
/// 1666 号：写点（chat 流水）与读点（budget-status 条目匹配）共用本函数=
/// serviceKey 口径单源。此前读点对不可解析的 baseUrl 取空串（`unwrap_or_default`），
/// 写点取原值——读写不同口径。
pub fn service_host_of(base_url: &str) -> String {
    url::Url::parse(base_url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .unwrap_or_else(|| base_url.to_string())
}

fn usage_ledger_path(root: &str) -> PathBuf {
    std::path::Path::new(root)
        .join(".inkos")
        .join("usage-log.jsonl")
}

/// 账本文本 → 行（坏行跳过：serde 要求六字段齐全且类型正确——TS
/// `isUsageLogRow` 同款形状守卫。缺字段的「像行的对象」若被收下，投影求和会
/// 出 NaN）。
fn parse_rows(text: &str) -> Vec<UsageLogRow> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// 读账本（TS `loadUsageLedger` 镜像）：无文件 = 空；坏行跳过（jsonl 宽容读）；
/// **读不出来（权限/IO）= Err**。1666 号：此前任何读错误都返回空——预算面据此
/// 报「0 用量」，确认门静默失效；追加面则用「只有新行」的内容覆盖整本历史。
pub async fn load_usage_ledger(root: &str) -> std::io::Result<Vec<UsageLogRow>> {
    let path = usage_ledger_path(root);
    let text = match tokio::fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    Ok(parse_rows(&text))
}

/// 追加账本行（读旧+拼+原子写；best-effort 失败只记日志）。
///
/// 1666 号：经 [`crate::utils::jsonl_ledger::append_jsonl`]——同一路径的追加
/// 进程内串行（并发追加此前丢行：32 路实测只剩 2 行）；读不出来即中止
/// （既有账本保留）。
pub async fn append_usage_rows(root: &str, rows: &[UsageLogRow]) {
    if rows.is_empty() {
        return;
    }
    let new_lines: Vec<String> = rows
        .iter()
        .map(|row| serde_json::to_string(row).unwrap_or_default())
        .collect();
    let appended = crate::utils::jsonl_ledger::append_jsonl(
        &usage_ledger_path(root),
        |text| {
            parse_rows(text)
                .iter()
                .map(|row| serde_json::to_string(row).unwrap_or_default())
                .collect()
        },
        new_lines,
    )
    .await;
    if let Err(error) = appended {
        eprintln!("[usage-ledger] append aborted, existing ledger kept (ignored): {error}");
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
///
/// 1666 号：cap=0 按「无预算」处理（TS 同款 `> 0` 守卫）。此前 `x / 0.0`：
/// 用量 > 0 时得 inf → 恒 over（每次发送都弹确认门），ratio 序列化成 null
/// （serde 把非有限 f64 写 null，破坏 `ratio: number` 契约）。
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
                .filter(|cap| *cap > 0)
                .map(|cap| usage.daily as f64 / cap as f64);
            let month_ratio = budget
                .monthly_token_budget
                .filter(|cap| *cap > 0)
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

    /// R63/1691 号（变异 spike 补断言）：预算档位的算术与边界——ratio 两位舍
    /// 入、双 cap 取 worst、=1.0 与 0.8 恰界、0 cap 过滤、host 不含端口。
    #[test]
    fn budget_status_precision_worst_cap_and_boundaries() {
        let budget = ServiceBudget {
            daily_token_budget: Some(1000),
            monthly_token_budget: None,
            confirm_at_budget: false,
        };
        // ratio 两位舍入：850/1000 = 0.85（除法变异 ×/% 当场红）。
        let usage = ServiceUsage {
            daily: 850,
            monthly: 0,
            total: 850,
        };
        let status = budget_status("a", &usage, Some(&budget));
        assert_eq!(status.ratio, 0.85);
        // =1.0 恰 Over（>= 变异 > 即红）。
        let exact = ServiceUsage {
            daily: 1000,
            monthly: 0,
            total: 1000,
        };
        let status = budget_status("a", &exact, Some(&budget));
        assert_eq!(status.level, BudgetLevel::Over);
        assert_eq!(status.ratio, 1.0);
        // 0.8 恰 Warn（>= 变异 > 即红：799/1000=Ok）。
        let warn_edge = ServiceUsage {
            daily: 800,
            monthly: 0,
            total: 800,
        };
        let status = budget_status("a", &warn_edge, Some(&budget));
        assert_eq!(status.level, BudgetLevel::Warn);
        let ok_edge = ServiceUsage {
            daily: 799,
            monthly: 0,
            total: 799,
        };
        let status = budget_status("a", &ok_edge, Some(&budget));
        assert_eq!(status.level, BudgetLevel::Ok);
        // 双 cap 取 worst（月 1.2 > 日 0.85 → Over+ratio=1.2；乘法/换位变异即红）。
        let dual = ServiceUsage {
            daily: 850,
            monthly: 1200,
            total: 2050,
        };
        let dual_budget = ServiceBudget {
            daily_token_budget: Some(1000),
            monthly_token_budget: Some(1000),
            confirm_at_budget: true,
        };
        let status = budget_status("a", &dual, Some(&dual_budget));
        assert_eq!(status.level, BudgetLevel::Over);
        assert_eq!(status.ratio, 1.2);
        assert!(status.confirm_required);
        // 0 cap 过滤（除零防护：cap=0 不产比率 → Ok/ratio 0；日/月各自过各自的
        // filter——R63 变异 spike 抓出月 cap 侧缺测）。
        let zero_daily = ServiceBudget {
            daily_token_budget: Some(0),
            monthly_token_budget: None,
            confirm_at_budget: true,
        };
        let status = budget_status("a", &usage, Some(&zero_daily));
        assert_eq!(status.level, BudgetLevel::Ok);
        assert_eq!(status.ratio, 0.0);
        assert!(!status.confirm_required);
        let zero_month = ServiceBudget {
            daily_token_budget: None,
            monthly_token_budget: Some(0),
            confirm_at_budget: false,
        };
        // 月用量非零：cap=0 若被放行（> → >= 变异）比率=1200/0=inf → Over 反例红
        // （monthly=0 时 0/0=NaN 会被 f64::max 忽略成等价变异）。
        let heavy_month = ServiceUsage {
            daily: 0,
            monthly: 1200,
            total: 1200,
        };
        let status = budget_status("a", &heavy_month, Some(&zero_month));
        assert_eq!(status.level, BudgetLevel::Ok);
        assert_eq!(status.ratio, 0.0);
    }

    /// R63/1691 号（变异 spike 补断言）：host 不含端口、不可解析回退原值；
    /// default_confirm 恒 true（缺省即需确认——翻 false 立即红）。
    #[test]
    fn service_host_of_strips_port_and_default_confirm_is_true() {
        assert_eq!(
            service_host_of("http://api.example.com:8080/v1"),
            "api.example.com"
        );
        assert_eq!(service_host_of("https://a.b/c"), "a.b");
        assert_eq!(service_host_of("not a url"), "not a url");
        assert!(default_confirm());
    }

    /// R63/1691 号（变异 spike 补断言）：读不出来（权限/IO）= Err 的 1666 语义
    /// ——NotFound 是唯一静默缺失（首启）；路径是目录（Is a directory IO 错）
    /// 必须报错（NotFound 守卫变异翻 true 时此测红）。
    #[tokio::test]
    async fn load_usage_ledger_errors_on_io_failure_not_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        // 账本路径上是目录 → read_to_string 报 Is-a-directory（非 NotFound）。
        let ledger_dir = dir.path().join(".inkos").join("usage-log.jsonl");
        tokio::fs::create_dir_all(&ledger_dir).await.unwrap();
        assert!(load_usage_ledger(&root).await.is_err());
    }

    #[tokio::test]
    async fn ledger_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        assert!(load_usage_ledger(&root).await.unwrap().is_empty());
        append_usage_rows(&root, &[row("2026-11-23T00:00:00.000Z", "a", 42)]).await;
        let rows = load_usage_ledger(&root).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_tokens, 42);
    }

    /// 1666 号：并发追加不得丢行（append-only）。「读旧+拼+原子写」的读改写
    /// 若交错，各任务都读到 N 行、各写 N+1 行。chat 多会话并行即触发。
    #[tokio::test]
    async fn concurrent_appends_keep_every_row() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        append_usage_rows(&root, &[row("2026-11-23T00:00:00.000Z", "old", 1)]).await;
        let mut handles = Vec::new();
        for index in 0..32u64 {
            let root = root.clone();
            handles.push(tokio::spawn(async move {
                append_usage_rows(
                    &root,
                    &[UsageLogRow {
                        ts: "2026-11-24T00:00:00.000Z".to_string(),
                        service_key: format!("svc-{index}"),
                        model: "m".to_string(),
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    }],
                )
                .await;
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
        let rows = load_usage_ledger(&root).await.unwrap();
        assert_eq!(rows.len(), 33, "并发追加丢行：只剩 {} 行", rows.len());
        assert_eq!(rows[0].service_key, "old");
    }
}
