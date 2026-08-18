//! 预测工件渲染（render.ts）——确定性 markdown 渲染器。
//!
//! 两份文档纯从 forecast.json 派生，重渲染无需再调 LLM，测试可无时钟。

use super::schema::{ForecastBranch, ForecastRiskKind, NarrativeForecast};

/// TS `renderForecastComparisonMarkdown` 逐字。
pub fn render_forecast_comparison_markdown(forecast: &NarrativeForecast) -> String {
    let zh = forecast.language == "zh";
    let header = if zh {
        vec![
            format!("# 叙事推演对比：{}", forecast.divergence),
            String::new(),
            format!("- 推演 ID：{}", forecast.forecast_id),
            format!("- 书籍：{}", forecast.book_id),
            format!("- 基准章节：第 {} 章", forecast.base_chapter),
            format!("- 推演跨度：约 {} 章", forecast.horizon),
            format!("- 生成时间：{}", forecast.created_at),
            String::new(),
            "> 本文件是非正史规划材料，不会改动正文或权威状态。".to_string(),
        ]
    } else {
        vec![
            format!("# Narrative forecast comparison: {}", forecast.divergence),
            String::new(),
            format!("- Forecast id: {}", forecast.forecast_id),
            format!("- Book: {}", forecast.book_id),
            format!("- Base chapter: {}", forecast.base_chapter),
            format!("- Horizon: ~{} chapters", forecast.horizon),
            format!("- Created at: {}", forecast.created_at),
            String::new(),
            "> Non-canonical planning material. Nothing here modifies prose or authoritative state.".to_string(),
        ]
    };

    let table_header = if zh {
        vec![
            "| 分支 | 标题 | 意图匹配 | 风险数 | 前提 |".to_string(),
            "| --- | --- | --- | --- | --- |".to_string(),
        ]
    } else {
        vec![
            "| Branch | Title | Intent fit | Risks | Premise |".to_string(),
            "| --- | --- | --- | --- | --- |".to_string(),
        ]
    };
    let table_rows: Vec<String> = forecast
        .branches
        .iter()
        .map(|branch| {
            format!(
                "| {} | {} | {} | {} | {} |",
                branch.branch_id,
                escape_cell(&branch.title),
                branch.intent_alignment.score,
                branch.risks.len(),
                escape_cell(&branch.premise)
            )
        })
        .collect();

    let sections: Vec<String> = forecast
        .branches
        .iter()
        .map(|branch| render_branch_section(branch, zh, 2, true))
        .collect();

    let mut lines = header;
    lines.push(String::new());
    lines.extend(table_header);
    lines.extend(table_rows);
    lines.push(String::new());
    lines.push(sections.join("\n\n"));
    lines.join("\n")
}

/// TS `renderSelectedBranchPlanMarkdown` 逐字。
pub fn render_selected_branch_plan_markdown(
    forecast: &NarrativeForecast,
    branch: &ForecastBranch,
    selected_at: &str,
    stale: bool,
) -> String {
    let zh = forecast.language == "zh";

    let stale_warning = if stale {
        if zh {
            "> ⚠️ 该推演已过期：正史章节或状态在推演生成后发生了变化。以下计划基于旧上下文，采用前请重新核对，必要时重新生成推演。"
        } else {
            "> ⚠️ This forecast is stale: canonical chapters or state changed after it was generated. The plan below is based on outdated context — re-check before applying, and regenerate if needed."
        }
        .to_string()
    } else {
        String::new()
    };

    let header = if zh {
        vec![
            format!("# 已选分支计划：{}", branch.title),
            String::new(),
            format!("- 推演 ID：{}", forecast.forecast_id),
            format!("- 分支：{}", branch.branch_id),
            format!("- 分歧点：{}", forecast.divergence),
            format!("- 基准章节：第 {} 章", forecast.base_chapter),
            format!("- 选择时间：{selected_at}"),
        ]
    } else {
        vec![
            format!("# Selected branch plan: {}", branch.title),
            String::new(),
            format!("- Forecast id: {}", forecast.forecast_id),
            format!("- Branch: {}", branch.branch_id),
            format!("- Divergence: {}", forecast.divergence),
            format!("- Base chapter: {}", forecast.base_chapter),
            format!("- Selected at: {selected_at}"),
        ]
    };

    let footer = if zh {
        "> 本计划不修改正史。要把它应用到大纲、章节意图或权威状态，需要另行确认的操作（v1 不自动执行）。"
    } else {
        "> This plan does not modify canon. Applying it to the outline, chapter intents, or authoritative state is a separate, explicitly confirmed operation (not automated in v1)."
    };

    let mut lines = header;
    if !stale_warning.is_empty() {
        lines.push(String::new());
        lines.push(stale_warning);
    }
    lines.push(String::new());
    lines.push(render_branch_section(branch, zh, 2, false));
    lines.push(String::new());
    lines.push(footer.to_string());
    lines.join("\n")
}

fn risk_kind(kind: ForecastRiskKind) -> &'static str {
    match kind {
        ForecastRiskKind::Continuity => "continuity",
        ForecastRiskKind::Causality => "causality",
        ForecastRiskKind::Character => "character",
    }
}

fn render_branch_section(branch: &ForecastBranch, zh: bool, heading_level: usize, include_branch_id: bool) -> String {
    let level = "#".repeat(heading_level);
    let sub = format!("{level}#");
    let heading = if include_branch_id {
        format!("{level} {}：{}", branch.branch_id, branch.title)
    } else {
        format!("{level} {}", branch.title)
    };

    let (premise, beats, decisions, changes, characters, relationships, world, hooks, risks, uncertainties, alignment, none) = if zh {
        (
            "前提与假设",
            "未来章节节拍",
            "人物决策",
            "预计变化",
            "人物",
            "关系",
            "世界",
            "伏笔",
            "一致性风险",
            "不确定性",
            "作者意图匹配度",
            "（无）",
        )
    } else {
        (
            "Premise and assumptions",
            "Future chapter beats",
            "Character decisions",
            "Projected changes",
            "Characters",
            "Relationships",
            "World",
            "Hooks",
            "Consistency risks",
            "Uncertainties",
            "Author intent alignment",
            "(none)",
        )
    };

    let list = |items: &[String]| -> String {
        if items.is_empty() {
            none.to_string()
        } else {
            items.iter().map(|item| format!("- {item}")).collect::<Vec<_>>().join("\n")
        }
    };
    let chapter_prefix = |n: u32| {
        if zh {
            format!("第 {n} 章")
        } else {
            format!("Chapter {n}")
        }
    };
    let join_or_none = |items: &[String]| -> String {
        if items.is_empty() {
            none.to_string()
        } else {
            items.join("；")
        }
    };

    let beat_lines: Vec<String> = branch
        .beats
        .iter()
        .map(|beat| format!("{}：{}", chapter_prefix(beat.chapter), beat.summary))
        .collect();
    let decision_lines: Vec<String> = branch
        .character_decisions
        .iter()
        .map(|decision| format!("{}：{}", decision.character, decision.decision))
        .collect();
    let risk_lines: Vec<String> = branch
        .risks
        .iter()
        .map(|risk| format!("[{}] {}", risk_kind(risk.kind), risk.description))
        .collect();

    [
        heading,
        String::new(),
        format!("{sub} {premise}"),
        String::new(),
        branch.premise.clone(),
        String::new(),
        format!("{sub} {beats}"),
        String::new(),
        list(&beat_lines),
        String::new(),
        format!("{sub} {decisions}"),
        String::new(),
        list(&decision_lines),
        String::new(),
        format!("{sub} {changes}"),
        String::new(),
        format!("- {characters}：{}", join_or_none(&branch.projected_changes.characters)),
        format!("- {relationships}：{}", join_or_none(&branch.projected_changes.relationships)),
        format!("- {world}：{}", join_or_none(&branch.projected_changes.world)),
        format!("- {hooks}：{}", join_or_none(&branch.projected_changes.hooks)),
        String::new(),
        format!("{sub} {risks}"),
        String::new(),
        list(&risk_lines),
        String::new(),
        format!("{sub} {uncertainties}"),
        String::new(),
        list(&branch.uncertainties),
        String::new(),
        format!("{sub} {alignment}"),
        String::new(),
        format!("{}/100 — {}", branch.intent_alignment.score, branch.intent_alignment.rationale),
    ]
    .join("\n")
}

fn escape_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}
