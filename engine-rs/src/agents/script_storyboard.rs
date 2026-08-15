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
