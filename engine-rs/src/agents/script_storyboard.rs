//! 剧本/分镜创作代理（agents/script-storyboard.ts，76 号）。
//!
//! 双代理（script-creation-writer 0.55 / storyboard-creation-writer 0.45）
//! + spec 渲染（zh/en 双语逐字）+ Markdown 提取面：
//! - `extract_markdown_section`（标题匹配：**加粗/杂符归一/前缀+分隔符容忍）
//! - `extract_storyboard_image_prompts`（Prompt: 行 + 表格 prompt 列双形态）
//! - `normalize_script_episode_end_labels`（"第N集"标题对齐"字幕：第N集完"）
//!
//! interactive-film 代理提示词与 spec 渲染随 77 号 runInteractiveFilmCreation。

use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::{LLMMessage, LLMRole};

#[derive(Debug, Clone, Default)]
pub struct ScriptCreationInput {
    pub title: String,
    pub source_kind: Option<String>,
    pub target_format: Option<String>,
    pub source_text: Option<String>,
    pub requirements: Option<String>,
    pub episode_count: Option<u32>,
    pub episode_duration: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StoryboardCreationInput {
    pub title: String,
    pub source_kind: Option<String>,
    pub source_text: Option<String>,
    pub requirements: Option<String>,
    pub visual_style: Option<String>,
    pub aspect_ratio: Option<String>,
    pub granularity: Option<String>,
    pub max_shots: Option<u32>,
    pub language: Option<String>,
}

fn is_en(language: Option<&str>) -> bool {
    language == Some("en")
}

// ── spec 渲染 ────────────────────────────────────────────────────

fn format_script_target(value: Option<&str>, language: Option<&str>) -> String {
    if is_en(language) {
        return match value {
            Some("vertical_short_drama") => "vertical short drama".to_string(),
            Some("screenplay") => "standard screenplay".to_string(),
            Some("audio_drama") => "audio drama".to_string(),
            Some("interactive_script") => "interactive script".to_string(),
            _ => "general script".to_string(),
        };
    }
    match value {
        Some("vertical_short_drama") => "竖屏短剧".to_string(),
        Some("screenplay") => "标准剧本".to_string(),
        Some("audio_drama") => "广播剧".to_string(),
        Some("interactive_script") => "互动剧本".to_string(),
        _ => "通用剧本".to_string(),
    }
}

/// `summarizeSourceForSpec`：空白折叠 + UTF-16 长度标注。
fn summarize_source_for_spec(source_text: Option<&str>, language: Option<&str>) -> String {
    let folded: String = source_text
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    if is_en(language) {
        if folded.is_empty() {
            return "No full source material provided.".to_string();
        }
        return format!(
            "Full source material provided, about {} characters; the full content will be read during generation.",
            folded.encode_utf16().count()
        );
    }
    if folded.is_empty() {
        return "未提供完整源素材。".to_string();
    }
    format!(
        "已提供完整源素材，约 {} 字符；生成时会读取完整内容。",
        folded.encode_utf16().count()
    )
}

pub fn render_script_spec(input: &ScriptCreationInput) -> String {
    let language = input.language.as_deref();
    if is_en(language) {
        let lines = vec![
            format!("# {} Script Creation Spec", input.title),
            String::new(),
            "## Goal".to_string(),
            format!("- Deliverable: {}", format_script_target(input.target_format.as_deref(), language)),
            input.episode_count
                .map(|count| format!("- Episode/segment count: {count}"))
                .unwrap_or_else(|| "- Episode/segment count: unspecified; judge from the source material and user requirements".to_string()),
            input.episode_duration.as_deref()
                .map(|duration| format!("- Per-episode/segment duration: {duration}"))
                .unwrap_or_else(|| "- Per-episode/segment duration: unspecified".to_string()),
            input.source_kind.as_deref()
                .map(|kind| format!("- Source material: {kind}"))
                .unwrap_or_else(|| "- Source material: user input / conversation brief".to_string()),
            String::new(),
            "## User Requirements".to_string(),
            input.requirements.as_deref().map(str::trim).filter(|r| !r.is_empty())
                .unwrap_or("Not separately specified; follow the instruction the user confirmed.")
                .to_string(),
            String::new(),
            "## Adaptation Boundaries".to_string(),
            "- Preserve the characters, relationships, conflicts, key events, and taboos the user explicitly specified.".to_string(),
            "- Never decide adaptation intensity (\"faithful adaptation / commercial punch-up / low-budget shoot\") on the user's behalf; execute only the spec the user has confirmed.".to_string(),
            "- If the source material is a novel, convert interiority into playable action, dialogue, evidence, objects, or on-screen consequences.".to_string(),
            "- If the target is a short drama, every episode needs visible conflict and an end-of-episode reason to keep watching.".to_string(),
            String::new(),
            "## Source Material Summary".to_string(),
            summarize_source_for_spec(input.source_text.as_deref(), language),
        ];
        lines.join("\n")
    } else {
        let lines = vec![
            format!("# {} 剧本创作规格", input.title),
            String::new(),
            "## 目标".to_string(),
            format!("- 交付类型：{}", format_script_target(input.target_format.as_deref(), language)),
            input.episode_count
                .map(|count| format!("- 集数/段落数：{count}"))
                .unwrap_or_else(|| "- 集数/段落数：未指定，按素材和用户要求判断".to_string()),
            input.episode_duration.as_deref()
                .map(|duration| format!("- 单集/单段时长：{duration}"))
                .unwrap_or_else(|| "- 单集/单段时长：未指定".to_string()),
            input.source_kind.as_deref()
                .map(|kind| format!("- 原素材：{kind}"))
                .unwrap_or_else(|| "- 原素材：用户输入/对话需求".to_string()),
            String::new(),
            "## 用户要求".to_string(),
            input.requirements.as_deref().map(str::trim).filter(|r| !r.is_empty())
                .unwrap_or("未单独指定；以用户确认时的 instruction 为准。")
                .to_string(),
            String::new(),
            "## 改编边界".to_string(),
            "- 优先保留用户明确指定的人物、关系、冲突、关键事件和禁忌。".to_string(),
            "- 不替用户擅自决定\u{201c}忠实改编 / 商业强化 / 低成本拍摄\u{201d}等强度；只执行用户已确认的规格。".to_string(),
            "- 如果原素材是小说，内心戏要转成可演的动作、对白、证据、物件或场面后果。".to_string(),
            "- 如果目标是短剧，每集必须有可见冲突和集尾继续看的理由。".to_string(),
            String::new(),
            "## 源素材摘要".to_string(),
            summarize_source_for_spec(input.source_text.as_deref(), language),
        ];
        lines.join("\n")
    }
}

pub fn render_storyboard_spec(input: &StoryboardCreationInput) -> String {
    let language = input.language.as_deref();
    if is_en(language) {
        [
            format!("# {} Storyboard Creation Spec", input.title).as_str(),
            "",
            "## Goal",
            &format!("- Shot granularity: {}", input.granularity.as_deref().map(str::trim).filter(|g| !g.is_empty()).unwrap_or("split by scene and key shots")),
            &format!("- Aspect ratio: {}", input.aspect_ratio.as_deref().map(str::trim).filter(|a| !a.is_empty()).unwrap_or("unspecified; default to what the user's material and target imply")),
            &format!("- Visual style: {}", input.visual_style.as_deref().map(str::trim).filter(|v| !v.is_empty()).unwrap_or("unspecified; judge from the user's material and target platform")),
            &input.max_shots.map(|shots| format!("- Shot cap: {shots}")).unwrap_or_else(|| "- Shot cap: unspecified".to_string()),
            &input.source_kind.clone().map(|kind| format!("- Source material: {kind}")).unwrap_or_else(|| "- Source material: user input / conversation brief".to_string()),
            "",
            "## User Requirements",
            input.requirements.as_deref().map(str::trim).filter(|r| !r.is_empty())
                .unwrap_or("Not separately specified; follow the instruction the user confirmed."),
            "",
            "## Storyboard Boundaries",
            "- A storyboard is a creative tool, not a locked-in shooting plan; the output must stay easy to discuss, extend, trim, and re-shoot.",
            "- Each shot carries only what the frame can show, an actor can play, and a camera can express.",
            "- Image prompts serve image generation: subject, action, shot size, setting, lighting, mood, and key props must be explicit.",
            "- Follow only the art style, format, composition, and visual constraints the user has confirmed; never turn unstated preferences into default hard constraints.",
            "",
            "## Source Material Summary",
            &summarize_source_for_spec(input.source_text.as_deref(), language),
        ]
        .join("\n")
    } else {
        [
            format!("# {} 分镜创作规格", input.title).as_str(),
            "",
            "## 目标",
            &format!("- 分镜粒度：{}", input.granularity.as_deref().map(str::trim).filter(|g| !g.is_empty()).unwrap_or("按场景和关键镜头拆分")),
            &format!("- 画幅：{}", input.aspect_ratio.as_deref().map(str::trim).filter(|a| !a.is_empty()).unwrap_or("未指定，默认按用户素材目标判断")),
            &format!("- 视觉风格：{}", input.visual_style.as_deref().map(str::trim).filter(|v| !v.is_empty()).unwrap_or("未指定，按用户素材和目标平台判断")),
            &input.max_shots.map(|shots| format!("- 镜头上限：{shots}")).unwrap_or_else(|| "- 镜头上限：未指定".to_string()),
            &input.source_kind.clone().map(|kind| format!("- 原素材：{kind}")).unwrap_or_else(|| "- 原素材：用户输入/对话需求".to_string()),
            "",
            "## 用户要求",
            input.requirements.as_deref().map(str::trim).filter(|r| !r.is_empty())
                .unwrap_or("未单独指定；以用户确认时的 instruction 为准。"),
            "",
            "## 分镜边界",
            "- 分镜是创作工具，不替用户锁死最终拍法；输出要便于继续讨论、增删、改镜头。",
            "- 每个镜头只写画面能看见、角色能演、镜头能表达的信息。",
            "- 分镜图提示词服务图像生成：角色、动作、景别、场景、光线、情绪和关键道具要清楚。",
            "- 只遵循用户已确认的画风、格式、构图和视觉限制；用户没说的，不写成默认硬限制。",
            "",
            "## 源素材摘要",
            &summarize_source_for_spec(input.source_text.as_deref(), language),
        ]
        .join("\n")
    }
}

// ── 提示词 ──────────────────────────────────────────────────────

fn script_creation_system_prompt(language: Option<&str>) -> String {
    if is_en(language) {
        [
            "You are a script-creation tool, not a novel-continuation engine.",
            "Your job is to adapt a novel, concept, outline, or existing text into a script that production can keep working from, following the spec the user has confirmed.",
            "Never decide adaptation intensity on the user's behalf; execute only the goals, format, boundaries, and constraints already confirmed in the spec.",
            "Action lines carry only what the audience can see, an actor can play, and a camera can shoot; convert interiority into behavior, dialogue, objects, evidence, or on-screen consequences.",
            "Dialogue must serve conflict, relationships, information flow, or emotional shifts; no hollow exposition.",
            "Output Markdown. No process notes, no model self-narration, no \"Here is\" preamble.",
        ]
        .join("\n")
    } else {
        [
            "你是剧本创作工具，不是小说续写器。",
            "你的任务是根据用户确认过的规格，把小说、创意、大纲或已有文本改成可继续制作的剧本。",
            "不要替用户擅自决定改编强度；只执行规格里已经确认的目标、格式、边界和限制。",
            "动作行只写观众能看见、演员能演、镜头能拍的信息；内心戏要转成行为、对白、物件、证据或场面后果。",
            "对白要服务冲突、关系、信息推进或情绪变化，不写空泛解释。",
            "输出 Markdown。不要写流程说明、模型自述或\u{201c}以下是\u{201d}。",
        ]
        .join("\n")
    }
}

fn script_creation_user_prompt(input: &ScriptCreationInput) -> String {
    let language = input.language.as_deref();
    if is_en(language) {
        [
            "## Creation Spec".to_string(),
            render_script_spec(input),
            String::new(),
            "## Full Source Material".to_string(),
            input.source_text.as_deref().map(str::trim).filter(|t| !t.is_empty())
                .unwrap_or("The user did not provide full source material; write an extensible script draft strictly from the creation spec and user requirements.")
                .to_string(),
            String::new(),
            "## Output Format".to_string(),
            format!("# {}", input.title),
            String::new(),
            "## Script".to_string(),
            String::new(),
            "Follow the target format. Vertical short drama: \"Episode N / scene slug / characters / action / dialogue / end-of-episode hook\". Standard screenplay: \"scene heading / action / character / dialogue\".".to_string(),
        ]
        .join("\n")
    } else {
        [
            "## 创作规格".to_string(),
            render_script_spec(input),
            String::new(),
            "## 完整源素材".to_string(),
            input.source_text.as_deref().map(str::trim).filter(|t| !t.is_empty())
                .unwrap_or("用户没有提供完整源素材；请严格根据创作规格和用户要求写一个可继续扩展的剧本稿。")
                .to_string(),
            String::new(),
            "## 输出格式".to_string(),
            format!("# {}", input.title),
            String::new(),
            "## 剧本正文".to_string(),
            String::new(),
            "按目标格式输出。竖屏短剧使用\u{201c}第N集 / 场次 / 人物 / 动作 / 对白 / 集尾钩子\u{201d}；标准剧本使用\u{201c}场景标题 / 动作 / 角色 / 对白\u{201d}。".to_string(),
        ]
        .join("\n")
    }
}

fn storyboard_creation_system_prompt(language: Option<&str>) -> String {
    if is_en(language) {
        [
            "You are a storyboard-creation tool: you break a script, novel excerpt, or concept into shots that can be filmed, drawn, and fed to image generation.",
            "A storyboard is not a plot summary; every shot needs a visual, character placement, action, shot size, or a visual focus.",
            "Keep the visual spec the user has confirmed; never promote visual constraints the user did not confirm into default requirements.",
            "Image prompts must be generation-ready: subject, action, setting, lighting, composition, mood, and key props all explicit.",
            "Output Markdown. No model self-narration or process explanation.",
        ]
        .join("\n")
    } else {
        [
            "你是分镜创作工具，负责把剧本、小说片段或创意拆成可拍、可画、可生图的分镜。",
            "分镜不是剧情摘要；每个镜头都要有画面、角色位置、动作、景别或视觉重点。",
            "保留用户确认的视觉规格；不要把用户没有确认的视觉限制写成默认要求。",
            "图像提示词要便于生图：主体、动作、场景、光线、构图、情绪、关键道具明确。",
            "输出 Markdown。不要写模型自述或流程解释。",
        ]
        .join("\n")
    }
}

fn storyboard_creation_user_prompt(input: &StoryboardCreationInput) -> String {
    let language = input.language.as_deref();
    let max_shots = input.max_shots.unwrap_or(24);
    if is_en(language) {
        [
            "## Storyboard Spec".to_string(),
            render_storyboard_spec(input),
            String::new(),
            "## Full Source Material".to_string(),
            input.source_text.as_deref().map(str::trim).filter(|t| !t.is_empty())
                .unwrap_or("The user did not provide full source material; write an extensible storyboard draft strictly from the storyboard spec and user requirements.")
                .to_string(),
            String::new(),
            "## Output Format".to_string(),
            format!("# {} Storyboard", input.title),
            String::new(),
            "## Storyboard".to_string(),
            String::new(),
            format!("Output at most {max_shots} shots. Each shot includes: shot number, visual, characters/objects, action, shot size/camera, dialogue/captions, suggested duration, notes."),
            String::new(),
            "## Image Prompts".to_string(),
            String::new(),
            "Write one generation-ready image prompt per shot. Each prompt MUST be its own `Prompt: ...` line; never merge it into the storyboard body, table headers, or explanations. Include only the visual constraints the user has confirmed.".to_string(),
        ]
        .join("\n")
    } else {
        [
            "## 分镜规格".to_string(),
            render_storyboard_spec(input),
            String::new(),
            "## 完整源素材".to_string(),
            input.source_text.as_deref().map(str::trim).filter(|t| !t.is_empty())
                .unwrap_or("用户没有提供完整源素材；请严格根据分镜规格和用户要求写一个可继续扩展的分镜稿。")
                .to_string(),
            String::new(),
            "## 输出格式".to_string(),
            format!("# {} 分镜", input.title),
            String::new(),
            "## 分镜表".to_string(),
            String::new(),
            format!("输出不超过 {max_shots} 个镜头。每个镜头包含：镜号、画面、人物/物件、动作、景别/机位、对白/字幕、时长建议、备注。"),
            String::new(),
            "## 图像提示词".to_string(),
            String::new(),
            "为每个镜头写一条可用于生图的提示词。每条必须单独写成 `Prompt: ...`，不要混入分镜正文、表头或解释；只写用户确认过的视觉限制。".to_string(),
        ]
        .join("\n")
    }
}

// ── 代理调用 ────────────────────────────────────────────────────

fn estimate_script_max_tokens(input: &ScriptCreationInput) -> u32 {
    let episodes = input.episode_count.unwrap_or(6);
    (episodes as u64 * 2200).clamp(12_000, 32_000) as u32
}

fn estimate_storyboard_max_tokens(input: &StoryboardCreationInput) -> u32 {
    let shots = input.max_shots.unwrap_or(24);
    (shots as u64 * 700).clamp(10_000, 24_000) as u32
}

/// `ScriptCreationAgent.writeScript`（temp 0.55）。
pub async fn write_script(router: &AgentRouter, input: &ScriptCreationInput) -> Result<String, String> {
    let language = input.language.as_deref();
    let outcome = router
        .chat(
            "script-creation-writer",
            vec![
                LLMMessage { role: LLMRole::System, content: script_creation_system_prompt(language), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: script_creation_user_prompt(input), tool_calls: None, tool_call_id: None },
            ],
            0.55,
            Some(estimate_script_max_tokens(input)),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(outcome.content.trim().to_string())
}

/// `StoryboardCreationAgent.writeStoryboard`（temp 0.45）。
pub async fn write_storyboard(router: &AgentRouter, input: &StoryboardCreationInput) -> Result<String, String> {
    let language = input.language.as_deref();
    let outcome = router
        .chat(
            "storyboard-creation-writer",
            vec![
                LLMMessage { role: LLMRole::System, content: storyboard_creation_system_prompt(language), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: storyboard_creation_user_prompt(input), tool_calls: None, tool_call_id: None },
            ],
            0.45,
            Some(estimate_storyboard_max_tokens(input)),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(outcome.content.trim().to_string())
}

// ── Markdown 提取面 ─────────────────────────────────────────────

fn normalize_heading_text(text: &str) -> String {
    // trim → **加粗剥壳 → 杂符（`*_）剥离 → 空白折叠 → lower。
    let trimmed = text.trim();
    let unbolded = trimmed
        .strip_prefix("**")
        .and_then(|body| body.strip_suffix("**"))
        .unwrap_or(trimmed);
    let stripped: String = unbolded.chars().filter(|c| !matches!(c, '`' | '*' | '_')).collect();
    stripped.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn heading_matches(text: &str, heading: &str) -> bool {
    if text == heading {
        return true;
    }
    if !text.starts_with(heading) {
        return false;
    }
    let rest = text[heading.len()..].trim();
    rest.is_empty()
        || rest.starts_with(['（', '(', '【', '[', ':', '：', '-', '—', ' '])
}

/// `extractMarkdownSection`：首个匹配标题的正文块（同级或更高级标题终止）。
pub fn extract_markdown_section(raw: &str, headings: &[&str]) -> Option<String> {
    let heading_re = regex::Regex::new(r"^(#{1,6})\s*(.+?)\s*$").ok()?;
    let level_re = regex::Regex::new(r"^(#{1,6})\s+").ok()?;
    let normalized: Vec<String> = headings.iter().map(|h| normalize_heading_text(h)).collect();
    let lines: Vec<&str> = raw.split('\n').map(|line| line.trim_end_matches('\r')).collect();
    let mut start = usize::MAX;
    let mut level = 0usize;
    for (index, line) in lines.iter().enumerate() {
        let Some(caps) = heading_re.captures(line) else { continue };
        let text = normalize_heading_text(caps.get(2).map(|m| m.as_str()).unwrap_or_default());
        if normalized.iter().any(|heading| heading_matches(&text, heading)) {
            start = index + 1;
            level = caps.get(1).map(|m| m.as_str()).unwrap_or_default().len();
            break;
        }
    }
    if start == usize::MAX {
        return None;
    }
    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate().skip(start) {
        if let Some(caps) = level_re.captures(line) {
            let line_level = caps.get(1).map(|m| m.as_str()).unwrap_or_default().len();
            if line_level <= level {
                end = index;
                break;
            }
        }
    }
    Some(lines[start..end].join("\n"))
}

fn parse_markdown_table_row(line: &str) -> Option<Vec<String>> {
    if !line.starts_with('|') || !line.ends_with('|') || line.len() < 2 {
        return None;
    }
    let cells: Vec<String> = line[1..line.len() - 1]
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect();
    if cells.len() >= 2 {
        Some(cells)
    } else {
        None
    }
}

fn is_markdown_table_separator(cells: &[String]) -> bool {
    let re = regex::Regex::new(r"^:?-{3,}:?$").unwrap();
    cells.iter().all(|cell| re.is_match(cell))
}

fn is_prompt_column_header(cell: &str) -> bool {
    let re = regex::Regex::new(r"(?i)^(?:prompt|image\s*prompt|shot\s*prompt|提示词|图像提示词|分镜图提示词)$").unwrap();
    let cleaned: String = cell.chars().filter(|c| !matches!(c, '`' | '*' | '_')).collect();
    re.is_match(cleaned.trim())
}

fn clean_prompt_text(text: &str) -> String {
    let re_tail_pipe = regex::Regex::new(r"\s*\|\s*$").unwrap();
    let re_tail_bold = regex::Regex::new(r"\*\*$").unwrap();
    let re_prefix = regex::Regex::new(
        r"(?i)^(?:Prompt(?:\s+for\s+[^:*：]+)?|提示词(?:\s*[^:*：]+)?|图像提示词|分镜图提示词)\s*[：:]\s*",
    )
    .unwrap();
    let step1 = re_tail_pipe.replace_all(text, "").to_string();
    let step2 = re_tail_bold.replace_all(&step1, "").to_string();
    let step3 = re_prefix.replace(&step2, "").to_string();
    step3.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `parseStoryboardPromptLines`：Prompt: 行 + 表格 prompt 列 + 编号行三形态。
pub fn parse_storyboard_prompt_lines(markdown: &str) -> Vec<String> {
    let prompt_re = regex::Regex::new(
        r"(?i)(?:^|[|>\-\d.)、\s])(?:\*\*)?\s*(?:Prompt(?:\s+for\s+[^:*：]+)?|提示词(?:\s*[^:*：]+)?|图像提示词|分镜图提示词)\s*(?:\*\*)?\s*[：:]\s*(.+?)\s*$",
    )
    .unwrap();
    let numbered_re = regex::Regex::new(r"^(?:[-*]\s*)?(?:\d+|[０-９]+)[.)、：:\s-]+(.+)$").unwrap();
    let mut prompts = Vec::new();
    let mut prompt_column_index: isize = -1;
    for raw_line in markdown.split('\n') {
        let line = raw_line.trim();
        if line.is_empty() {
            prompt_column_index = -1;
            continue;
        }
        if let Some(cells) = parse_markdown_table_row(line) {
            if is_markdown_table_separator(&cells) {
                continue;
            }
            if let Some(header_index) = cells.iter().position(|cell| is_prompt_column_header(cell)) {
                prompt_column_index = header_index as isize;
                continue;
            }
            if prompt_column_index >= 0 {
                let prompt = clean_prompt_text(cells.get(prompt_column_index as usize).map(String::as_str).unwrap_or(""));
                if !prompt.is_empty() {
                    prompts.push(prompt);
                }
            }
            continue;
        }
        prompt_column_index = -1;
        if let Some(caps) = prompt_re.captures(line) {
            let prompt = clean_prompt_text(caps.get(1).map(|m| m.as_str()).unwrap_or_default());
            if !prompt.is_empty() {
                prompts.push(prompt);
            }
            continue;
        }
        if let Some(caps) = numbered_re.captures(line) {
            let prompt: String = caps
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !prompt.is_empty() {
                prompts.push(prompt);
            }
        }
    }
    prompts
}

/// `extractStoryboardImagePrompts`：图像提示词小节提取 + 编号化；空回退整体。
pub fn extract_storyboard_image_prompts(raw: &str) -> String {
    let section = extract_markdown_section(raw, &["图像提示词", "分镜图提示词", "Image Prompts", "Shot Image Prompts"]);
    let source = section
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(raw.trim());
    let prompts = parse_storyboard_prompt_lines(source);
    if prompts.is_empty() {
        String::new()
    } else {
        prompts
            .iter()
            .enumerate()
            .map(|(index, prompt)| format!("{}. {prompt}", index + 1))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// `normalizeScriptEpisodeEndLabels`：按当前"第N集"标题校正"字幕：第N集完"。
pub fn normalize_script_episode_end_labels(script: &str) -> String {
    let heading_re = regex::Regex::new(r"^(#{1,6})\s*第\s*([一二三四五六七八九十百千万\d]+)\s*集(?:\s|$)").unwrap();
    let label_re = regex::Regex::new(
        r"(字幕\s*[：:]\s*)第\s*[一二三四五六七八九十百千万\d]+\s*集完",
    )
    .unwrap();
    let mut current_episode: Option<String> = None;
    let mut out_lines = Vec::new();
    for line in script.split('\n') {
        let trimmed = line.trim();
        if let Some(caps) = heading_re.captures(trimmed) {
            current_episode = caps.get(2).map(|m| m.as_str().to_string());
        }
        if current_episode.is_none() {
            out_lines.push(line.to_string());
            continue;
        }
        let episode = current_episode.clone().unwrap_or_default();
        let replaced = label_re.replace_all(line, format!("$1第{episode}集完").as_str()).to_string();
        out_lines.push(replaced);
    }
    out_lines.join("\n")
}

/// `requiredSection`：小节提取失败回退整体（trim）。
pub fn required_section(raw: &str, headings: &[&str]) -> String {
    extract_markdown_section(raw, headings)
        .map(|section| section.trim().to_string())
        .filter(|section| !section.is_empty())
        .unwrap_or_else(|| raw.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_section_extraction() {
        let raw = "# 标题\n\n## 图像提示词\n\nPrompt: 雪夜街道\nPrompt: 灯下人影\n\n## 其它\n\n后面内容";
        let section = extract_markdown_section(raw, &["图像提示词", "Image Prompts"]).unwrap();
        assert!(section.contains("雪夜街道"), "{section}");
        assert!(!section.contains("其它"), "{section}");
        // 加粗/杂符标题匹配。
        let bolded = "# A\n\n## **Image Prompts** — extra\n\ncontent";
        assert!(extract_markdown_section(bolded, &["Image Prompts"]).is_some());
        // 无匹配 → None。
        assert!(extract_markdown_section("no headings", &["X"]).is_none());
        // requiredSection 回退整体。
        assert_eq!(required_section("no headings", &["X"]), "no headings");
    }

    #[test]
    fn prompt_lines_three_forms() {
        // Prompt: 行。
        let lines = parse_storyboard_prompt_lines("Prompt: 雪夜街道\n提示词：灯下人影\n图像提示词: 远景");
        assert_eq!(lines, vec!["雪夜街道", "灯下人影", "远景"]);
        // 表格 prompt 列（表头定位 + 数据行取列）。
        let table = "| 镜号 | 提示词 |\n| --- | --- |\n| 01 | 雪夜街口 |";
        assert_eq!(parse_storyboard_prompt_lines(table), vec!["雪夜街口"]);
        // 编号行兜底。
        assert_eq!(parse_storyboard_prompt_lines("1. 第一条"), vec!["第一条"]);
        // extract_storyboard_image_prompts 编号化。
        let numbered = extract_storyboard_image_prompts("## 图像提示词\n\nPrompt: A\nPrompt: B");
        assert_eq!(numbered, "1. A\n2. B");
    }

    #[test]
    fn episode_end_label_alignment() {
        let script = "# 第一集 开端\n\n正文\n\n字幕：第三集完\n\n# 第二集\n\n字幕：第一集完";
        let normalized = normalize_script_episode_end_labels(script);
        assert!(normalized.contains("字幕：第一集完"), "{normalized}");
        let second = normalized.split("# 第二集").nth(1).unwrap_or_default();
        assert!(second.contains("字幕：第二集完"), "{normalized}");
    }

    #[test]
    fn story_graph_from_llm_text_parsing() {
        // fence + 前后噪声 → 子串提取 + projectId 强制注入。
        let fenced = "前置噪声\n```json\n{\"schemaVersion\":1,\"projectId\":\"旧id\",\"title\":\"迷雾\",\"nodes\":[{\"id\":\"start\",\"type\":\"start\"}],\"endings\":[]}\n```\n后置噪声";
        let graph = build_story_graph_from_llm_text(fenced, "mist-01").unwrap();
        assert_eq!(graph.project_id, "mist-01");
        assert_eq!(graph.title, "迷雾");
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].id, "start");
        // 空 nodes → Err。
        let empty = build_story_graph_from_llm_text("{\"schemaVersion\":1,\"nodes\":[]}", "p");
        assert!(empty.is_err());
        // 无 JSON 对象 → 双语错误。
        let bad = build_story_graph_from_llm_text("no json here", "p").unwrap_err();
        assert!(bad.contains("LLM 未返回可解析的 JSON 对象"), "{bad}");
    }

    #[test]
    fn interactive_film_spec_and_prompts_shape() {
        let input = InteractiveFilmCreationInput {
            title: "迷雾宅邸".into(),
            episode_count: Some(5),
            target_audience: Some("青年玩家".into()),
            budget: Some("低预算".into()),
            requirements: Some("悬疑多结局".into()),
            ..Default::default()
        };
        let spec = render_interactive_film_spec(&input);
        assert!(spec.starts_with("# 迷雾宅邸 互动影游创作规格"), "{spec}");
        assert!(spec.contains("- 剧情段落/集数：5"), "{spec}");
        assert!(spec.contains("- 目标受众：青年玩家"), "{spec}");
        assert!(spec.contains("- 预算约束：低预算"), "{spec}");
        assert!(spec.contains("## 互动影游边界"), "{spec}");
        // 五节用户提示词含全部小节标题。
        let user = interactive_creation_user_prompt(&input);
        for heading in ["## 剧情树", "## 变量与旗标表", "## 多结局路径", "## 互动剧本", "## 分镜与图像提示词"] {
            assert!(user.contains(heading), "user prompt 缺 {heading}");
        }
    }

    #[test]
    fn spec_rendering_zh_shapes() {
        let script_input = ScriptCreationInput {
            title: "山雨".into(),
            target_format: Some("vertical_short_drama".into()),
            episode_count: Some(8),
            ..Default::default()
        };
        let spec = render_script_spec(&script_input);
        assert!(spec.starts_with("# 山雨 剧本创作规格"), "{spec}");
        assert!(spec.contains("- 交付类型：竖屏短剧"), "{spec}");
        assert!(spec.contains("- 集数/段落数：8"), "{spec}");
        assert!(spec.contains("未单独指定；以用户确认时的 instruction 为准。"), "{spec}");

        let board_input = StoryboardCreationInput {
            title: "山雨".into(),
            visual_style: Some("水墨".into()),
            max_shots: Some(12),
            ..Default::default()
        };
        let spec = render_storyboard_spec(&board_input);
        assert!(spec.contains("- 视觉风格：水墨"), "{spec}");
        assert!(spec.contains("- 镜头上限：12"), "{spec}");
        // 无 source → 摘要占位。
        assert!(spec.contains("未提供完整源素材。"), "{spec}");
    }
}

// ── interactive-film 创作面（77 号） ──────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct InteractiveFilmCreationInput {
    pub title: String,
    pub source_kind: Option<String>,
    pub source_text: Option<String>,
    pub requirements: Option<String>,
    pub target_audience: Option<String>,
    pub episode_count: Option<u32>,
    pub episode_duration: Option<String>,
    pub budget: Option<String>,
    pub reference_mode: Option<String>,
    pub language: Option<String>,
}

pub fn render_interactive_film_spec(input: &InteractiveFilmCreationInput) -> String {
    let language = input.language.as_deref();
    if is_en(language) {
        [
            format!("# {} Interactive Film Creation Spec", input.title).as_str(),
            "",
            "## Goal",
            "- Deliverable: interactive film / interactive narrative game / film-game script",
            &input.episode_count.map(|c| format!("- Story segments/episodes: {c}")).unwrap_or_else(|| "- Story segments/episodes: unspecified; judge from the source material and user requirements".to_string()),
            &input.episode_duration.as_deref().map(|d| format!("- Per-segment/episode duration: {d}")).unwrap_or_else(|| "- Per-segment/episode duration: unspecified".to_string()),
            &input.budget.as_deref().map(|b| format!("- Budget constraint: {b}")).unwrap_or_else(|| "- Budget constraint: unspecified".to_string()),
            &input.target_audience.as_deref().map(|a| format!("- Target audience: {a}")).unwrap_or_else(|| "- Target audience: unspecified".to_string()),
            &input.reference_mode.as_deref().map(|r| format!("- Reference mode: {r}")).unwrap_or_else(|| "- Reference mode: unspecified by the user; do not impose a fixed game template".to_string()),
            &input.source_kind.as_deref().map(|k| format!("- Source material: {k}")).unwrap_or_else(|| "- Source material: user input / conversation brief".to_string()),
            "",
            "## User Requirements",
            input.requirements.as_deref().map(str::trim).filter(|r| !r.is_empty())
                .unwrap_or("Not separately specified; follow the instruction the user confirmed."),
            "",
            "## Interactive Film Boundaries",
            "- This is a creative deliverable, not a hard-numbers RPG engine design; variables, flags, relationships, and ending conditions must serve story branching.",
            "- It must include branching storylines, key player choices, how variables/flags change later plot, and the conditions for reaching each of the multiple endings.",
            "- Describe the variable system in natural language: states, relationships, secret/public status, evidence, items, identities, affinity/trust, and the like; never force fixed numeric stats or equipment tiers.",
            "- The deliverable must fit interactive film/drama production: a clear story tree, shootable nodes, playable dialogue, drawable storyboards, and image prompts usable for asset generation.",
            "- Never decide subject matter, budget, art style, or commercial punch-up intensity on the user's behalf; mark anything unspecified as adjustable.",
            "",
            "## Source Material Summary",
            &summarize_source_for_spec(input.source_text.as_deref(), language),
        ]
        .join("\n")
    } else {
        [
            format!("# {} 互动影游创作规格", input.title).as_str(),
            "",
            "## 目标",
            "- 交付类型：互动影游 / 互动叙事类游戏 / 影游剧本",
            &input.episode_count.map(|c| format!("- 剧情段落/集数：{c}")).unwrap_or_else(|| "- 剧情段落/集数：未指定，按素材和用户要求判断".to_string()),
            &input.episode_duration.as_deref().map(|d| format!("- 单段/单集时长：{d}")).unwrap_or_else(|| "- 单段/单集时长：未指定".to_string()),
            &input.budget.as_deref().map(|b| format!("- 预算约束：{b}")).unwrap_or_else(|| "- 预算约束：未指定".to_string()),
            &input.target_audience.as_deref().map(|a| format!("- 目标受众：{a}")).unwrap_or_else(|| "- 目标受众：未指定".to_string()),
            &input.reference_mode.as_deref().map(|r| format!("- 参考模式：{r}")).unwrap_or_else(|| "- 参考模式：用户未指定，不擅自套固定游戏模板".to_string()),
            &input.source_kind.as_deref().map(|k| format!("- 原素材：{k}")).unwrap_or_else(|| "- 原素材：用户输入/对话需求".to_string()),
            "",
            "## 用户要求",
            input.requirements.as_deref().map(str::trim).filter(|r| !r.is_empty())
                .unwrap_or("未单独指定；以用户确认时的 instruction 为准。"),
            "",
            "## 互动影游边界",
            "- 这是创作交付稿，不是硬数值 RPG 引擎设计；变量、旗标、关系和结局条件必须服务剧情分支。",
            "- 必须包含多分支剧情、玩家关键选择、变量/旗标如何改变后续剧情，以及多结局达成条件。",
            "- 变量系统用自然语言说明即可：状态、关系、隐瞒/公开、证据、物品、身份、好感/信任等；不要强行套固定数值或装备等级。",
            "- 交付要适配影游/互动剧制作：剧情树清晰、节点可拍、对白可演、分镜可画、图片提示词可用于资产生成。",
            "- 不替用户擅自决定题材、预算、画风和商业强化强度；未指定处写为可调整。",
            "",
            "## 源素材摘要",
            &summarize_source_for_spec(input.source_text.as_deref(), language),
        ]
        .join("\n")
    }
}

fn interactive_film_system_prompt(language: Option<&str>) -> String {
    const SHAPE: &str = r#"{"schemaVersion":1,"projectId":"","title":"","variables":[{"name":"","type":"flag|counter|relationship|item","default":0,"desc":""}],"nodes":[{"id":"","title":"","type":"start|normal|branch|ending","sceneDesc":"","dialogue":[{"speaker":"","text":"","emotion":""}],"choices":[{"id":"","text":"","targetNodeId":"","condition":{"var":"","op":">=","value":0},"effects":[{"var":"","op":"add","value":1}]}]}],"endings":[{"id":"","nodeId":"","title":"","type":"good|bad|neutral|secret","description":""}]}"#;
    if is_en(language) {
        format!(
            "You are an interactive film scriptwriter. From the user's story premise, generate a small but complete playable branching graph.\nOutput strictly JSON, with this structure:\n{SHAPE}\nRequirements: exactly 1 node with type=start; at least 2 branch nodes; at least 2 clearly differentiated endings; every path must reach some ending; condition/effects may be omitted; output nothing besides the JSON."
        )
    } else {
        format!(
            "你是互动影游编剧。根据用户的故事前提，生成一个小而完整的可玩分支图。\n严格只输出 JSON，结构如下：\n{SHAPE}\n要求：恰好 1 个 type=start 节点；至少 2 个 branch 节点；至少 2 个差异化 ending；每条路径都能到达某个 ending；condition/effects 可省略；不要输出 JSON 以外的任何文字。"
        )
    }
}

fn interactive_creation_system_prompt(language: Option<&str>) -> String {
    if is_en(language) {
        [
            "You are an interactive-film creation tool: you turn a concept, novel, script, or user brief into an interactive-film deliverable that production can build from.",
            "An interactive film is not an ordinary script: it must have a story tree, key player choices, variables/flags, relationship/evidence/item states, and the conditions for reaching each of the multiple endings.",
            "The variable system exists only to drive plot progression and branch unlocking; no default RPG stats, combat formulas, or equipment tiers. Write such rules only when the user explicitly asks for them.",
            "Output must be Markdown with the specified sections. No model self-narration, process notes, or \"Here is\" preamble.",
            "Every storyboard image prompt must be its own standalone `Prompt: ...` line so downstream asset management can pick it up; include only the visual constraints the user has confirmed.",
        ]
        .join("\n")
    } else {
        [
            "你是互动影游创作工具，负责把创意、小说、剧本或用户需求整理成可制作的互动影游交付稿。",
            "互动影游不是普通剧本：必须有剧情树、关键选择、变量/旗标、关系/证据/物品状态、多结局达成条件。",
            "变量系统只服务剧情推进和分支解锁，不要默认 RPG 数值、战斗公式或装备等级；只有用户明确要求时才写对应规则。",
            "输出必须是 Markdown，包含指定小节。不要写模型自述、流程说明或\u{201c}以下是\u{201d}。",
            "分镜图提示词必须写成单独的 `Prompt: ...` 行，便于后续资产管理；只写用户确认过的视觉限制。",
        ]
        .join("\n")
    }
}

fn interactive_creation_user_prompt(input: &InteractiveFilmCreationInput) -> String {
    let language = input.language.as_deref();
    if is_en(language) {
        [
            "## Interactive Film Spec".to_string(),
            render_interactive_film_spec(input),
            String::new(),
            "## Full Source Material".to_string(),
            input.source_text.as_deref().map(str::trim).filter(|t| !t.is_empty())
                .unwrap_or("The user did not provide full source material; write an extensible interactive-film deliverable strictly from the creation spec and user requirements.")
                .to_string(),
            String::new(),
            "## Output Format".to_string(),
            format!("# {} Interactive Film Package", input.title),
            String::new(),
            "## Story Tree".to_string(),
            "Lay out main-line nodes, branch nodes, key choices, and merge/no-return relationships as Markdown. The multi-ending structure must be visible at a glance.".to_string(),
            String::new(),
            "## Variables and Flags".to_string(),
            "List each variable/flag: name, meaning, trigger, scope of impact, and related nodes. Variables may be relationships, states, evidence, items, identities, secret/public status, ending gates, and so on.".to_string(),
            String::new(),
            "## Ending Paths".to_string(),
            "For every ending: its unlock conditions, the key choice chain, the required variables/flags, plus any failure or hidden-ending conditions.".to_string(),
            String::new(),
            "## Interactive Script".to_string(),
            "Write a playable script per node: scene, characters, action, dialogue, player choices, variable changes, and branch destinations. Never write summaries only.".to_string(),
            String::new(),
            "## Storyboard and Image Prompts".to_string(),
            "List the key shots. Each shot includes visual, characters/objects, action, shot size, and suggested duration. After each shot, add exactly one standalone `Prompt: ...` line.".to_string(),
        ]
        .join("\n")
    } else {
        [
            "## 互动影游规格".to_string(),
            render_interactive_film_spec(input),
            String::new(),
            "## 完整源素材".to_string(),
            input.source_text.as_deref().map(str::trim).filter(|t| !t.is_empty())
                .unwrap_or("用户没有提供完整源素材；请严格根据创作规格和用户要求写一个可继续扩展的互动影游交付稿。")
                .to_string(),
            String::new(),
            "## 输出格式".to_string(),
            format!("# {} 互动影游方案", input.title),
            String::new(),
            "## 剧情树".to_string(),
            "用 Markdown 列出主线节点、分支节点、关键选择、回流/不可回流关系。必须能看出多结局结构。".to_string(),
            String::new(),
            "## 变量与旗标表".to_string(),
            "列出变量/旗标名、含义、触发方式、影响范围、对应节点。变量可以是关系、状态、证据、物品、身份、公开/隐瞒、结局门槛等。".to_string(),
            String::new(),
            "## 多结局路径".to_string(),
            "列出每个结局的达成条件、关键选择链、必需变量/旗标，以及失败或隐藏结局条件。".to_string(),
            String::new(),
            "## 互动剧本".to_string(),
            "按节点写可演剧本：场景、人物、动作、对白、玩家选择、变量变化和分支去向。不要只写摘要。".to_string(),
            String::new(),
            "## 分镜与图像提示词".to_string(),
            "列出关键镜头。每个镜头包含画面、人物/物件、动作、景别、时长建议。每个镜头后必须单独写一行 `Prompt: ...`。".to_string(),
        ]
        .join("\n")
    }
}

fn estimate_interactive_film_max_tokens(input: &InteractiveFilmCreationInput) -> u32 {
    let episodes = input.episode_count.unwrap_or(6);
    (episodes as u64 * 3000).clamp(16_000, 36_000) as u32
}

/// `InteractiveFilmCreationAgent.writeInteractiveFilm`（temp 0.5）。
pub async fn write_interactive_film(
    router: &crate::llm::agent_router::AgentRouter,
    input: &InteractiveFilmCreationInput,
) -> Result<String, String> {
    let language = input.language.as_deref();
    let outcome = router
        .chat(
            "interactive-film-creation-writer",
            vec![
                LLMMessage { role: LLMRole::System, content: interactive_creation_system_prompt(language), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: interactive_creation_user_prompt(input), tool_calls: None, tool_call_id: None },
            ],
            0.5,
            Some(estimate_interactive_film_max_tokens(input)),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(outcome.content.trim().to_string())
}

/// `generateStoryGraph`（temp 0.5 / 8000）——剧情树+旗标+剧本+提示词打包前提。
pub async fn generate_story_graph_from_premise(
    router: &crate::llm::agent_router::AgentRouter,
    project_id: &str,
    title: &str,
    premise: &str,
    language: Option<&str>,
) -> Result<crate::interactive_film::StoryGraph, String> {
    let user_prompt = if is_en(language) {
        format!("Title: {title}\nPremise: {premise}")
    } else {
        format!("标题：{title}\n前提：{premise}")
    };
    let outcome = router
        .chat(
            "interactive-film-graph-writer",
            vec![
                LLMMessage { role: LLMRole::System, content: interactive_film_system_prompt(language), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user_prompt, tool_calls: None, tool_call_id: None },
            ],
            0.5,
            Some(8000),
        )
        .await
        .map_err(|e| e.to_string())?;
    build_story_graph_from_llm_text(&outcome.content, project_id)
}

/// `buildStoryGraphFromLLMText`：fence/子串提取 + schema 解析 + projectId 注入。
pub fn build_story_graph_from_llm_text(
    text: &str,
    project_id: &str,
) -> Result<crate::interactive_film::StoryGraph, String> {
    let trimmed = text.trim();
    let fenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|body| body.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let start = fenced.find('{');
    let end = fenced.rfind('}');
    let Some((start, end)) = start.zip(end).filter(|(s, e)| e > s) else {
        return Err("LLM did not return a parseable JSON object / LLM 未返回可解析的 JSON 对象".to_string());
    };
    let mut parsed: serde_json::Value =
        serde_json::from_str(&fenced[start..=end]).map_err(|e| e.to_string())?;
    if let Some(obj) = parsed.as_object_mut() {
        obj.insert("projectId".to_string(), serde_json::json!(project_id));
    }
    let graph: crate::interactive_film::StoryGraph =
        serde_json::from_value(parsed).map_err(|e| e.to_string())?;
    if graph.nodes.is_empty() {
        return Err("Invalid story graph: nodes array must not be empty".to_string());
    }
    Ok(graph)
}
