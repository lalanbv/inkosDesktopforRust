//! narrative forecast 聊天三件套（90 号）——create / get / select。
//!
//! 移植自 `packages/core/src/agent/forecast-tools.ts`：三件工具严格操作
//! `story/runtime/narrative-forecasts/`——不改正史正文/状态/控制文档。
//! v1 create 在聊天轮内同步执行。注册面：仅 book/book-create 会话
//! （TS edit 过滤器剔除 forecast 三件）。

use serde_json::{json, Value};

use crate::forecast::schema::{ForecastBranch, NarrativeForecast};
use crate::interaction::import_chapters_tool::resolve_tool_book_id;
use crate::interaction::project_tools::{error_result, ToolResult};
use crate::llm::agent_router::RoutedAgent;
use crate::server::books_routes::BooksRuntime;

/// 工具依赖：runtime + 活动书。
pub struct ForecastDeps<'a> {
    pub runtime: &'a BooksRuntime,
    pub active_book_id: &'a str,
}

fn text_result(text: impl Into<String>, details: Option<Value>) -> ToolResult {
    ToolResult { text: text.into(), details, is_error: false }
}

fn field_str<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn describe_branch(branch: &ForecastBranch) -> String {
    format!(
        "{} \"{}\" — intent fit {}/100, {} risk(s), premise: {}",
        branch.branch_id,
        branch.title,
        branch.intent_alignment.score,
        branch.risks.len(),
        branch.premise
    )
}

fn describe_forecast(forecast: &NarrativeForecast, stale: bool) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Forecast {} (book {}) — status: {}.",
            forecast.forecast_id,
            forecast.book_id,
            if stale { "stale" } else { status_str(forecast) }
        ),
        format!("Divergence: {}", forecast.divergence),
        format!(
            "Base chapter: {}, horizon: ~{} chapters.",
            forecast.base_chapter, forecast.horizon
        ),
    ];
    if stale {
        lines.push(
            "WARNING: canonical chapters or state changed after this forecast was generated; regenerate before relying on it."
                .to_string(),
        );
    }
    lines.push("Branches:".to_string());
    lines.extend(forecast.branches.iter().map(describe_branch));
    lines
}

fn status_str(forecast: &NarrativeForecast) -> &'static str {
    match forecast.status {
        crate::forecast::schema::ForecastStatus::Active => "active",
        crate::forecast::schema::ForecastStatus::Stale => "stale",
    }
}

async fn forecast_agent(runtime: &BooksRuntime) -> RoutedAgent {
    RoutedAgent { router: runtime.effective_router().await, agent: "forecast" }
}

/// `create_narrative_forecast`：正史上下文 + 分歧点 → 2-5 隔离候选分支。
pub async fn tool_create_narrative_forecast(deps: &ForecastDeps<'_>, args: &Value) -> ToolResult {
    let book_id = match resolve_tool_book_id(
        "create_narrative_forecast",
        field_str(args, "bookId"),
        Some(deps.active_book_id),
    ) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let Some(divergence) = args.get("divergence").and_then(Value::as_str) else {
        return error_result(
            "divergence is required: describe the decision point the forecast should branch on."
                .to_string(),
        );
    };
    let branch_count = parse_bounded_u32(args, "branchCount");
    let horizon = parse_bounded_u32(args, "horizon");
    let agent = forecast_agent(deps.runtime).await;
    match crate::forecast::runner::create_narrative_forecast(
        &agent,
        &crate::forecast::runner::CreateNarrativeForecastOptions {
            project_root: deps.runtime.state.project_root(),
            book_id: &book_id,
            divergence,
            branch_count,
            horizon,
        },
    )
    .await
    {
        Ok(result) => {
            let forecast = &result.forecast;
            let mut lines = vec![format!(
                "Narrative forecast {} created with {} isolated branches.",
                forecast.forecast_id,
                forecast.branches.len()
            )];
            lines.extend(forecast.branches.iter().map(describe_branch));
            lines.push(format!("Comparison: {}", result.comparison_path));
            lines.push(format!("Forecast data: {}", result.forecast_json_path));
            lines.push(
                "These branches are non-canonical planning material. Use select_narrative_branch with the forecastId and a branchId to write selected-branch-plan.md, or get_narrative_forecast to re-check staleness later."
                    .to_string(),
            );
            let details = json!({
                "kind": "narrative_forecast_created",
                "forecastId": forecast.forecast_id,
                "forecast": forecast,
                "forecastJsonPath": result.forecast_json_path,
                "comparisonPath": result.comparison_path,
            });
            text_result(lines.join("\n"), Some(details))
        }
        Err(message) => error_result(message),
    }
}

/// `get_narrative_forecast`：读取 + 重检过期（stale 标记持久化）。
pub async fn tool_get_narrative_forecast(deps: &ForecastDeps<'_>, args: &Value) -> ToolResult {
    let book_id = match resolve_tool_book_id(
        "get_narrative_forecast",
        field_str(args, "bookId"),
        Some(deps.active_book_id),
    ) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let Some(forecast_id) = field_str(args, "forecastId") else {
        return error_result("get_narrative_forecast requires a forecastId.".to_string());
    };
    match crate::forecast::runner::get_narrative_forecast(
        deps.runtime.state.project_root(),
        &book_id,
        forecast_id,
    )
    .await
    {
        Ok(result) => {
            let mut lines = describe_forecast(&result.forecast, result.stale);
            lines.push(format!("Comparison: {}", result.comparison_path));
            let details = json!({
                "kind": "narrative_forecast",
                "stale": result.stale,
                "forecast": result.forecast,
            });
            text_result(lines.join("\n"), Some(details))
        }
        Err(message) => error_result(message),
    }
}

/// `select_narrative_branch`：选择分支，只写 selected-branch-plan.md。
pub async fn tool_select_narrative_branch(deps: &ForecastDeps<'_>, args: &Value) -> ToolResult {
    let book_id = match resolve_tool_book_id(
        "select_narrative_branch",
        field_str(args, "bookId"),
        Some(deps.active_book_id),
    ) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    let Some(forecast_id) = field_str(args, "forecastId") else {
        return error_result("select_narrative_branch requires a forecastId.".to_string());
    };
    let Some(branch_id) = field_str(args, "branchId") else {
        return error_result("select_narrative_branch requires a branchId.".to_string());
    };
    match crate::forecast::runner::select_narrative_branch(
        deps.runtime.state.project_root(),
        &book_id,
        forecast_id,
        branch_id,
    )
    .await
    {
        Ok(result) => {
            let mut lines = vec![format!(
                "Selected {} \"{}\" from forecast {}.",
                result.branch.branch_id, result.branch.title, result.forecast.forecast_id
            )];
            if result.stale {
                lines.push(
                    "WARNING: this forecast is stale — canon changed after it was generated. The plan includes a stale warning; verify before applying."
                        .to_string(),
                );
            }
            lines.push(format!("Plan written: {}", result.plan_path));
            lines.push(
                "Canonical files were not modified. Applying this plan to the outline or chapter intents requires explicit user confirmation."
                    .to_string(),
            );
            let details = json!({
                "kind": "narrative_branch_selected",
                "stale": result.stale,
                "planPath": result.plan_path,
                "branchId": result.branch.branch_id,
            });
            text_result(lines.join("\n"), Some(details))
        }
        Err(message) => error_result(message),
    }
}

fn parse_bounded_u32(args: &Value, name: &str) -> Option<u32> {
    args.get(name)
        .and_then(Value::as_f64)
        .filter(|v| v.fract() == 0.0)
        .map(|v| v as u32)
}

/// 三件分发（未知名返回 None 交回退执行器）。
pub async fn execute_forecast_tool(deps: &ForecastDeps<'_>, name: &str, args: &Value) -> Option<ToolResult> {
    match name {
        "create_narrative_forecast" => Some(tool_create_narrative_forecast(deps, args).await),
        "get_narrative_forecast" => Some(tool_get_narrative_forecast(deps, args).await),
        "select_narrative_branch" => Some(tool_select_narrative_branch(deps, args).await),
        _ => None,
    }
}

/// 三件 schema（ForecastCreate/Get/SelectParams 逐字）。
pub fn forecast_tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "create_narrative_forecast",
                "description": "Create a non-canonical narrative forecast for the current long-form book: reads the canonical context (structured state, author intent, current focus, outline, hooks, recent summaries, characters) and projects 2-5 mutually isolated candidate futures from a divergence point. Writes forecast.json and comparison.md under story/runtime/narrative-forecasts/<forecastId>/ and never modifies canonical chapters or state.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "bookId": { "type": "string", "description": "All forecast tools: book id to forecast. Defaults to the active book; must match it when both are present." },
                        "divergence": { "type": "string", "description": "Required divergence point: the open decision or fork the author wants to compare, e.g. 主角接受还是拒绝对手的合作提议. Include the competing options when known." },
                        "branchCount": { "type": "number", "description": "create_narrative_forecast only: number of mutually isolated candidate branches, integer 2-5. Default 3." },
                        "horizon": { "type": "number", "description": "create_narrative_forecast only: how many future chapters each branch should cover, integer 1-10. Default 5." },
                    },
                    "required": ["divergence"],
                },
            },
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_narrative_forecast",
                "description": "Read an existing narrative forecast, re-check it against the current canonical context, and mark it stale when canonical chapters, structured state, or control documents changed after it was generated. Read-only apart from persisting the stale marker inside the forecast's own directory.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "bookId": { "type": "string", "description": "All forecast tools: book id. Defaults to the active book; must match it when both are present." },
                        "forecastId": { "type": "string", "description": "get_narrative_forecast only: forecast id returned by create_narrative_forecast, e.g. fc-20260715-080910." },
                    },
                    "required": ["forecastId"],
                },
            },
        }),
        json!({
            "type": "function",
            "function": {
                "name": "select_narrative_branch",
                "description": "Select one branch of a narrative forecast. Writes only selected-branch-plan.md inside the forecast directory — it does NOT apply the plan to the outline, chapter intents, or canonical state; that is a separate, user-confirmed operation.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "bookId": { "type": "string", "description": "All forecast tools: book id. Defaults to the active book; must match it when both are present." },
                        "forecastId": { "type": "string", "description": "select_narrative_branch only: forecast id containing the branch to select." },
                        "branchId": { "type": "string", "description": "select_narrative_branch only: branch id to select, e.g. branch-2. Must exist in the forecast." },
                    },
                    "required": ["forecastId", "branchId"],
                },
            },
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_shape() {
        let schemas = forecast_tool_schemas();
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec!["create_narrative_forecast", "get_narrative_forecast", "select_narrative_branch"]
        );
        assert_eq!(
            schemas[0]["function"]["parameters"]["required"],
            json!(["divergence"])
        );
        assert_eq!(
            schemas[2]["function"]["parameters"]["required"],
            json!(["forecastId", "branchId"])
        );
    }
}
