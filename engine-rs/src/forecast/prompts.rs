//! 预测提示词（prompts.ts）——双语三构建器逐字。

pub type ForecastLanguage = &'static str; // "zh" | "en"

pub fn build_forecast_system_prompt(language: ForecastLanguage) -> String {
    if language == "en" {
        [
            "You are the narrative forecast assistant for a long-form novel.",
            "Task: starting from the canonical context and the author's divergence point, project several mutually isolated, non-canonical candidate futures for the author to compare.",
            "Rules:",
            "- Branches are mutually exclusive: each assumes a different resolution of the divergence point and must not reference or depend on sibling branches.",
            "- Branches are planning material, not prose: beats describe what happens, not scene-level detail.",
            "- Respect canon: every projection must stay consistent with established facts, character locks, and world rules; any necessary conflict must be listed under risks.",
            "- Output exactly one JSON object. No explanations, no markdown headings, no code fences.",
        ]
        .join("\n")
    } else {
        [
            "你是长篇小说的叙事推演助手。",
            "任务：从正史上下文和作者给出的分歧点出发，推演多个相互隔离的非正史候选未来分支，供作者并排比较。",
            "规则：",
            "- 分支之间互斥：每个分支对分歧点做出不同走向的假设，不得引用或依赖其他分支。",
            "- 分支是规划材料，不是正文：节拍只写“发生了什么”，不写场景级细节。",
            "- 尊重正史：所有推演必须与既有事实、人设锁和世界规则一致；确需冲突时必须写进 risks。",
            "- 只输出一个 JSON 对象，不要输出解释、markdown 标题或代码围栏。",
        ]
        .join("\n")
    }
}

pub struct ForecastPromptInput<'a> {
    pub context_markdown: &'a str,
    pub divergence: &'a str,
    pub branch_count: usize,
    pub horizon: u32,
    pub base_chapter: u32,
}

pub fn build_forecast_user_prompt(input: &ForecastPromptInput<'_>, language: ForecastLanguage) -> String {
    let first_chapter = input.base_chapter + 1;
    if language == "en" {
        [
            input.context_markdown.to_string(),
            String::new(),
            "## Divergence point".to_string(),
            String::new(),
            input.divergence.to_string(),
            String::new(),
            "## Output requirements".to_string(),
            String::new(),
            format!(
                "Produce exactly {} candidate branches. Each branch covers roughly {} future chapters starting at chapter {}.",
                input.branch_count, input.horizon, first_chapter
            ),
            "Return JSON with exactly this shape (field names must match):".to_string(),
            forecast_json_shape(first_chapter, "en"),
        ]
        .join("\n")
    } else {
        [
            input.context_markdown.to_string(),
            String::new(),
            "## 分歧点".to_string(),
            String::new(),
            input.divergence.to_string(),
            String::new(),
            "## 输出要求".to_string(),
            String::new(),
            format!(
                "生成恰好 {} 个候选分支。每个分支覆盖从第 {} 章开始、约 {} 章的未来走向。",
                input.branch_count, first_chapter, input.horizon
            ),
            "输出 JSON，结构如下（字段名必须完全一致）：".to_string(),
            forecast_json_shape(first_chapter, "zh"),
        ]
        .join("\n")
    }
}

pub fn build_forecast_repair_prompt(validation_error: &str, language: ForecastLanguage) -> String {
    if language == "en" {
        [
            format!("Your previous output failed validation: {validation_error}"),
            "Re-output the complete JSON object only, fixing the problem above. No explanations, no code fences.".to_string(),
        ]
        .join("\n")
    } else {
        [
            format!("你上一次的输出未通过校验：{validation_error}"),
            "请修正上述问题后重新输出完整 JSON 对象，只输出 JSON，不要解释，不要代码围栏。".to_string(),
        ]
        .join("\n")
    }
}

fn forecast_json_shape(first_chapter: u32, language: ForecastLanguage) -> String {
    let beats_line = if language == "en" {
        format!(
            "      \"beats\": [{{ \"chapter\": integer chapter number starting at {first_chapter}, \"summary\": \"what happens in that chapter\" }}],"
        )
    } else {
        format!(
            "      \"beats\": [{{ \"chapter\": 从 {first_chapter} 开始的整数章号, \"summary\": \"该章发生什么\" }}],"
        )
    };
    let lines: Vec<String> = if language == "en" {
        [
            "{",
            "  \"branches\": [",
            "    {",
            "      \"title\": \"short branch title\",",
            "      \"premise\": \"the assumption this branch makes about the divergence point\",",
            "      \"characterDecisions\": [{ \"character\": \"name\", \"decision\": \"the key decision this character makes\" }],",
            "      \"projectedChanges\": {",
            "        \"characters\": [\"projected character state changes\"],",
            "        \"relationships\": [\"projected relationship changes\"],",
            "        \"world\": [\"projected world/faction changes\"],",
            "        \"hooks\": [\"which hooks advance, fire, or break\"]",
            "      },",
            "      \"risks\": [{ \"kind\": \"continuity|causality|character\", \"description\": \"consistency risk\" }],",
            "      \"uncertainties\": [\"open uncertainties\"],",
            "      \"intentAlignment\": { \"score\": integer 0-100, \"rationale\": \"how well this matches the author intent and current focus\" }",
            "    }",
            "  ]",
            "}",
        ]
        .iter()
        .map(|line| line.to_string())
        .collect()
    } else {
        [
            "{",
            "  \"branches\": [",
            "    {",
            "      \"title\": \"分支短标题\",",
            "      \"premise\": \"该分支对分歧点做出的前提与假设\",",
            "      \"characterDecisions\": [{ \"character\": \"人物名\", \"decision\": \"该人物做出的关键决策\" }],",
            "      \"projectedChanges\": {",
            "        \"characters\": [\"人物状态预计变化\"],",
            "        \"relationships\": [\"关系预计变化\"],",
            "        \"world\": [\"世界/势力预计变化\"],",
            "        \"hooks\": [\"哪些伏笔被推进、引爆或破坏\"]",
            "      },",
            "      \"risks\": [{ \"kind\": \"continuity|causality|character\", \"description\": \"一致性风险\" }],",
            "      \"uncertainties\": [\"不确定因素\"],",
            "      \"intentAlignment\": { \"score\": 0到100的整数, \"rationale\": \"与作者意图和当前聚焦的匹配说明\" }",
            "    }",
            "  ]",
            "}",
        ]
        .iter()
        .map(|line| line.to_string())
        .collect()
    };
    // beats 行插在 premise 之后（与 TS 行序一致）。
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    for line in lines {
        out.push(line);
        if out.len() == 5 {
            out.push(beats_line.clone());
        }
    }
    out.join("\n")
}
