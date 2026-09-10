//! propose_action 确认卡聊天工具（84 号）。
//!
//! 移植自 `packages/core/src/agent/agent-tools.ts` 的
//! `createProposeActionTool`：聊天里把生产意图升格为确认卡——15 种
//! action、9 个结构化 actionPayload 子域（compact 清洗）、必填字段断言、
//! targetSessionKind/targetRoute 映射与双语回退文案。
//! strict 载荷校验复用 [`crate::server::agent_route::validate_action_payload_strict`]
//! （75 号 ActionPayloadSchema 逐字）。

use serde_json::{json, Map, Value};

use crate::interaction::project_tools::{error_result, ToolResult};

pub const PROPOSE_ACTIONS: &[&str] = &[
    "create_book", "short_run", "play_start", "generate_cover", "fanfic_init",
    "continuation_import", "spinoff_create", "style_imitation", "script_create",
    "storyboard_create", "interactive_film_create", "translation_create",
    "draft_structure", "connect_choice", "remove_node",
];

/// 工具依赖：表面语言 + 同会话标记（sessionKind !== "chat"）+ 请求技能。
pub struct ProposeDeps<'a> {
    pub language: &'a str,
    pub same_session: bool,
    pub requested_skills: &'a [String],
}

fn text_result(text: impl Into<String>, details: Option<Value>) -> ToolResult {
    ToolResult { text: text.into(), details, is_error: false }
}

/// `proposedActionSessionKind`：action → 目标会话 kind。
pub fn proposed_action_session_kind(action: &str) -> &'static str {
    match action {
        "create_book" => "book-create",
        "play_start" => "play",
        "script_create" => "script",
        "storyboard_create" => "storyboard",
        "interactive_film_create" => "interactive-film",
        "translation_create" => "chat",
        "draft_structure" | "connect_choice" | "remove_node" => "interactive-film-authoring",
        "fanfic_init" | "continuation_import" | "spinoff_create" | "style_imitation" => "chat",
        _ => "short",
    }
}

/// `proposedActionTargetRoute`：四个辅助入口 → import:* 路由。
fn proposed_action_target_route(action: &str) -> Option<&'static str> {
    match action {
        "fanfic_init" => Some("import:fanfic"),
        "continuation_import" => Some("import:chapters"),
        "spinoff_create" => Some("import:spinoff"),
        "style_imitation" => Some("import:imitation"),
        _ => None,
    }
}

/// `proposedActionFallbackTitle`：15 种 action 双语回退标题。
fn proposed_action_fallback_title(action: &str, is_en: bool) -> &'static str {
    match action {
        "create_book" => pick(is_en, "创建长篇书籍", "Create a long-form book"),
        "short_run" => pick(is_en, "生成 InkOS Short", "Generate InkOS Short"),
        "play_start" => pick(is_en, "启动 InkOS Play", "Start InkOS Play"),
        "generate_cover" => pick(is_en, "生成封面", "Generate cover"),
        "fanfic_init" => pick(is_en, "打开同人创作", "Open fanfiction workflow"),
        "continuation_import" => pick(is_en, "打开续写导入", "Open continuation import"),
        "spinoff_create" => pick(is_en, "打开番外创作", "Open side-story workflow"),
        "style_imitation" => pick(is_en, "打开仿写/文风分析", "Open style imitation"),
        "script_create" => pick(is_en, "创建剧本", "Create script"),
        "storyboard_create" => pick(is_en, "创建分镜", "Create storyboard"),
        "interactive_film_create" => pick(is_en, "创建互动影游", "Create interactive film"),
        "translation_create" => pick(is_en, "创建翻译项目", "Create translation project"),
        "draft_structure" => pick(is_en, "生成故事结构", "Draft story structure"),
        "connect_choice" => pick(is_en, "连接选项", "Connect choice"),
        _ => pick(is_en, "删除节点", "Remove node"),
    }
}

fn proposed_action_fallback_summary(action: &str, is_en: bool) -> &'static str {
    if proposed_action_target_route(action).is_some() {
        pick(
            is_en,
            "确认后只会打开现有 Studio 工具，不会直接生成成品。",
            "After confirmation, InkOS will only open the existing Studio tool; it will not generate finished content directly.",
        )
    } else {
        pick(
            is_en,
            "确认后会切换到对应入口并执行这条需求。",
            "After confirmation, InkOS will switch to the matching surface and run this request.",
        )
    }
}

fn pick(is_en: bool, zh: &'static str, en: &'static str) -> &'static str {
    if is_en { en } else { zh }
}

/// `compactObject`：子域清洗——字符串 trim 非空、数组滤空 trim、正数保留、
/// 其余非空保留；空结果 → None。
fn compact_object(value: Option<&Value>) -> Option<Value> {
    let object = value?.as_object()?;
    let mut out = Map::new();
    for (key, raw) in object {
        match raw {
            Value::String(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    out.insert(key.clone(), json!(trimmed));
                }
            }
            Value::Array(items) => {
                let cleaned: Vec<String> = items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(String::from)
                    .collect();
                if !cleaned.is_empty() {
                    out.insert(key.clone(), json!(cleaned));
                }
            }
            Value::Number(number) => {
                if let Some(value) = number.as_f64() {
                    if value.is_finite() && value > 0.0 {
                        out.insert(key.clone(), json!(number));
                    }
                }
            }
            other => {
                if !other.is_null() {
                    out.insert(key.clone(), other.clone());
                }
            }
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// `INCOMPLETE_PLAY_SCENE_SUFFIX`：句末悬垂（单字连词/标点 + 因为/如果/——）。
fn ends_with_incomplete_scene_suffix(text: &str) -> bool {
    if text.ends_with("因为") || text.ends_with("如果") || text.ends_with("——") {
        return true;
    }
    let Some(last) = text.chars().last() else {
        return false;
    };
    matches!(
        last,
        '叫' | '是' | '为' | '在' | '向' | '把' | '将' | '和' | '与' | '或' | '但' | '却'
            | '当' | '等' | '、' | '，' | '：' | '；' | '—' | '“' | '‘' | '《' | '（' | '('
    )
}

/// `isUsablePlayInitialScene`：非空 + ≥12 码元 + 句末不悬垂。
pub fn is_usable_play_initial_scene(value: Option<&str>) -> bool {
    let Some(text) = value.map(str::trim).filter(|t| !t.is_empty()) else {
        return false;
    };
    if text.encode_utf16().count() < 12 {
        return false;
    }
    !ends_with_incomplete_scene_suffix(text)
}

/// `normalizeSuggestedActions`：字符串或对象（action/label/text/title/
/// description）→ 空白折叠 + trim，≤4 条。
fn normalize_suggested_actions(value: Option<&Value>) -> Vec<String> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for raw in items {
        let text = match raw {
            Value::String(text) => text.clone(),
            Value::Object(object) => ["action", "label", "text", "title", "description"]
                .iter()
                .find_map(|key| object.get(*key).and_then(Value::as_str))
                .unwrap_or_default()
                .to_string(),
            _ => String::new(),
        };
        let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !normalized.is_empty() {
            out.push(normalized);
        }
        if out.len() >= 4 {
            break;
        }
    }
    out
}

fn compact_play_start_payload(value: Option<&Value>) -> Option<Value> {
    let object = value?.as_object()?;
    let mut out = Map::new();
    let trim_insert = |out: &mut Map<String, Value>, key: &str, value: Option<&str>| {
        if let Some(text) = value.map(str::trim).filter(|t| !t.is_empty()) {
            out.insert(key.to_string(), json!(text));
        }
    };
    trim_insert(&mut out, "title", object.get("title").and_then(Value::as_str));
    trim_insert(&mut out, "premise", object.get("premise").and_then(Value::as_str));
    trim_insert(&mut out, "worldContract", object.get("worldContract").and_then(Value::as_str));
    trim_insert(&mut out, "visualContract", object.get("visualContract").and_then(Value::as_str));
    if let Some(mode) = object.get("mode").and_then(Value::as_str) {
        out.insert("mode".into(), json!(mode));
    }
    let initial_scene = object.get("initialScene").and_then(Value::as_str);
    if is_usable_play_initial_scene(initial_scene) {
        out.insert("initialScene".into(), json!(initial_scene.unwrap_or_default().trim()));
    }
    let suggested = normalize_suggested_actions(object.get("suggestedActions"));
    if !suggested.is_empty() {
        out.insert("suggestedActions".into(), json!(suggested));
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// `proposedActionPayload`：按 action 拼装结构化子域（shortRun 注入 language）。
fn proposed_action_payload(args: &Value, language: &str) -> Option<Value> {
    let mut payload = Map::new();
    let domains = [
        ("create_book", "createBook", "createBook"),
        ("short_run", "shortRun", "shortRun"),
        ("play_start", "playStart", "playStart"),
        ("generate_cover", "generateCover", "generateCover"),
        ("script_create", "scriptCreate", "scriptCreate"),
        ("storyboard_create", "storyboardCreate", "storyboardCreate"),
        ("interactive_film_create", "interactiveFilmCreate", "interactiveFilmCreate"),
        ("translation_create", "translationCreate", "translationCreate"),
        // 301 号：四件创建域结构化字段补齐（TS agent-tools.ts L379-424 同名，
        // 此前 schema 未宣传导致提示词指示的必填字段无处落）。
        ("fanfic_init", "fanficCreate", "fanficCreate"),
        ("continuation_import", "continuationImport", "continuationImport"),
        ("spinoff_create", "spinoffCreate", "spinoffCreate"),
        ("style_imitation", "imitationCreate", "imitationCreate"),
    ];
    let action = args.get("action").and_then(Value::as_str).unwrap_or_default();
    let (_, arg_key, payload_key) = domains.iter().find(|(act, _, _)| *act == action)?;
    let compact = if action == "play_start" {
        compact_play_start_payload(args.get(arg_key))
    } else {
        compact_object(args.get(arg_key))
    };
    let mut compact = compact?;
    if action == "short_run" {
        // shortRun 前置注入会话语言（模型未填 language 时兜底）。
        if let Some(obj) = compact.as_object_mut() {
            obj.entry("language".to_string())
                .or_insert_with(|| json!(language));
        }
    }
    payload.insert(payload_key.to_string(), compact);
    Some(Value::Object(payload))
}

/// `assertExecutableProposedAction`：必填字段断言（缺失 → 固定错误文案）。
fn require_proposed_text(payload: Option<&Value>, path: &str) -> Result<(), String> {
    let text = payload
        .and_then(|p| p.pointer(path))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty());
    if text.is_some() {
        return Ok(());
    }
    Err(format!(
        "propose_action is missing {path}; retry with that field in the structured payload, not only in summary or instruction."
    ))
}

fn assert_executable_proposed_action(action: &str, payload: Option<&Value>) -> Result<(), String> {
    match action {
        "create_book" => require_proposed_text(payload, "/createBook/title"),
        "play_start" => {
            require_proposed_text(payload, "/playStart/title")?;
            require_proposed_text(payload, "/playStart/premise")?;
            require_proposed_text(payload, "/playStart/initialScene")
        }
        "generate_cover" => require_proposed_text(payload, "/generateCover/title"),
        "script_create" => require_proposed_text(payload, "/scriptCreate/title"),
        "storyboard_create" => require_proposed_text(payload, "/storyboardCreate/title"),
        "interactive_film_create" => require_proposed_text(payload, "/interactiveFilmCreate/title"),
        "translation_create" => {
            require_proposed_text(payload, "/translationCreate/filePath")?;
            require_proposed_text(payload, "/translationCreate/sourceLanguage")?;
            require_proposed_text(payload, "/translationCreate/targetLanguage")
        }
        // 301 号：四件创建域必填断言（对齐 TypeBox required：fanfic.title /
        // continuation.sourcePath / spinoff.title+parentBookId / imitation.title+storyIdea）。
        "fanfic_init" => require_proposed_text(payload, "/fanficCreate/title"),
        "continuation_import" => require_proposed_text(payload, "/continuationImport/sourcePath"),
        "spinoff_create" => {
            require_proposed_text(payload, "/spinoffCreate/title")?;
            require_proposed_text(payload, "/spinoffCreate/parentBookId")
        }
        "style_imitation" => {
            require_proposed_text(payload, "/imitationCreate/title")?;
            require_proposed_text(payload, "/imitationCreate/storyIdea")
        }
        _ => Ok(()),
    }
}

/// `propose_action` 执行器：确认卡生成。
pub async fn tool_propose_action(deps: &ProposeDeps<'_>, args: &Value) -> ToolResult {
    let action = args.get("action").and_then(Value::as_str).unwrap_or_default();
    if !PROPOSE_ACTIONS.contains(&action) {
        return error_result(format!(
            "Invalid propose_action.action: {action}（expected one of {}）",
            PROPOSE_ACTIONS.join(" | ")
        ));
    }
    let Some(instruction) = args
        .get("instruction")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return error_result("propose_action requires an instruction argument".to_string());
    };
    let is_en = deps.language == "en";
    let target_session_kind = proposed_action_session_kind(action);
    let target_route = proposed_action_target_route(action);
    let title = args
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| proposed_action_fallback_title(action, is_en))
        .to_string();
    let summary = args
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| proposed_action_fallback_summary(action, is_en))
        .to_string();

    let proposed = proposed_action_payload(args, deps.language);
    if let Some(payload) = &proposed {
        if let Err(message) = crate::server::agent_route::validate_action_payload_strict(payload) {
            return error_result(format!("Invalid proposed action payload: {message}"));
        }
    }
    if let Err(message) = assert_executable_proposed_action(action, proposed.as_ref()) {
        return error_result(message);
    }

    let mut details = json!({
        "kind": "proposed_action",
        "action": action,
        "targetSessionKind": target_session_kind,
        "sameSession": deps.same_session,
        "title": title,
        "summary": summary,
        "instruction": instruction,
    });
    let details_obj = details.as_object_mut().unwrap();
    if let Some(route) = target_route {
        details_obj.insert("targetRoute".into(), json!(route));
    }
    if !deps.requested_skills.is_empty() {
        details_obj.insert("requestedSkills".into(), json!(deps.requested_skills));
    }
    if let Some(payload) = proposed {
        details_obj.insert("actionPayload".into(), payload);
    }

    text_result(
        format!("{title}\n{summary}\n\nInstruction: {instruction}"),
        Some(details),
    )
}

/// `propose_action` schema（ProposeActionParams 逐字）。
pub fn propose_action_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "propose_action",
            "description": "Ask the user to confirm a production action from general chat. Use this before creating books, generating shorts/covers, or starting play worlds when the user has not clicked a confirmation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": PROPOSE_ACTIONS,
                        "description": "The production or assisted Studio workflow the user appears to want, but which needs explicit confirmation from general chat.",
                    },
                    "instruction": {
                        "type": "string",
                        "description": "The exact production instruction to run after the user confirms. It must be self-contained: include title, story direction, active target, output directory, cover visual direction, or any referenced context that would otherwise be lost when switching sessions.",
                    },
                    "title": {
                        "type": "string",
                        "description": "Short user-facing title for the confirmation card.",
                    },
                    "summary": {
                        "type": "string",
                        "description": "One or two sentences explaining what will happen if the user confirms.",
                    },
                    "createBook": {
                        "type": "object",
                        "description": "Structured execution args for action=create_book. Put platform/length here; do not leave them only in instruction text.",
                        "properties": {
                            "title": { "type": "string", "description": "Confirmed long-form book title." },
                            "genre": { "type": "string", "description": "Confirmed book genre/category." },
                            "platform": { "type": "string", "enum": ["tomato", "qidian", "feilu", "other"], "description": "Confirmed target platform, e.g. tomato for 番茄." },
                            "language": { "type": "string", "enum": ["zh", "en"], "description": "Confirmed writing language." },
                            "targetChapters": { "type": "number", "description": "Confirmed total chapter count." },
                            "chapterWordCount": { "type": "number", "description": "Confirmed per-chapter length in the book's native unit." },
                        },
                    },
                    "shortRun": {
                        "type": "object",
                        "description": "Structured execution args for action=short_run.",
                        "properties": {
                            "direction": { "type": "string", "description": "Confirmed standalone short direction." },
                            "reference": { "type": "string", "description": "Optional confirmed reference notes or constraints." },
                            "storyId": { "type": "string", "description": "Optional confirmed output id under shorts/." },
                            "language": { "type": "string", "enum": ["zh", "en"], "description": "Output language of the short fiction. Fill the language the user asked the story to be written in; it may differ from the conversation language (e.g. a Chinese chat asking for an English short => en). When the user does not name one, it defaults to the conversation language." },
                            "chapters": { "type": "number", "description": "Confirmed complete short chapter count, 12-18." },
                            "charsPerChapter": { "type": "number", "minimum": 600, "maximum": 1200, "description": "Confirmed per-chapter length in the story language's native unit. zh shorts only accept 900-1200 Chinese characters; en shorts only accept 600-800 English words. Values outside the selected language's range are rejected before the task starts. Do not put total story length here." },
                            "cover": { "type": "boolean", "description": "Whether to attempt cover generation." },
                        },
                    },
                    "playStart": {
                        "type": "object",
                        "description": "Structured execution args for action=play_start.",
                        "properties": {
                            "title": { "type": "string", "description": "Confirmed interactive world title." },
                            "premise": { "type": "string", "description": "Confirmed playable premise." },
                            "worldContract": { "type": "string", "description": "Confirmed durable world contract in natural language: time semantics, role autonomy, object/clue/relationship rules, taboos, or other long-lived rules the user explicitly asked for. Do not invent RPG/level systems." },
                            "visualContract": { "type": "string", "description": "Confirmed visual contract for Play illustrations in natural language. Only include user-defined visual semantics; do not invent game frames, colored tiers, UI, or stats." },
                            "mode": { "type": "string", "enum": ["open", "guided"], "description": "Confirmed play mode: open for free actions, guided for suggested choices." },
                            "initialScene": { "type": "string", "description": "Confirmed opening scene shown to the player after confirmation. It must be pure narrative prose, not a title/setup/rules summary, not a question prompt, and not an action/options list." },
                            "suggestedActions": { "type": "array", "description": "Optional action springboards shown as separate UI chips. Do not include these in initialScene.", "items": { "type": "string" } },
                        },
                    },
                    "generateCover": {
                        "type": "object",
                        "description": "Structured execution args for action=generate_cover.",
                        "properties": {
                            "title": { "type": "string", "description": "Confirmed cover title." },
                            "intro": { "type": "string", "description": "Confirmed synopsis/hook for the cover." },
                            "sellingPoints": { "type": "string", "description": "Confirmed selling points for the cover." },
                            "coverPrompt": { "type": "string", "description": "Confirmed visual direction." },
                            "outputDir": { "type": "string", "description": "Confirmed output directory." },
                        },
                    },
                    "scriptCreate": {
                        "type": "object",
                        "description": "Structured execution args for action=script_create.",
                        "properties": {
                            "title": { "type": "string", "description": "Confirmed script project title." },
                            "sourceKind": { "type": "string", "description": "Source type, e.g. novel excerpt, original idea, outline, existing script." },
                            "targetFormat": { "type": "string", "enum": ["vertical_short_drama", "screenplay", "audio_drama", "interactive_script", "general_script"], "description": "Confirmed script output format." },
                            "sourceText": { "type": "string", "description": "User-provided source text. For long sources, prefer sourcePath instead of summarizing." },
                            "sourcePath": { "type": "string", "description": "Optional project-relative source file path." },
                            "requirements": { "type": "string", "description": "Confirmed script format, production constraints, tone, episode structure, or user preferences." },
                            "episodeCount": { "type": "number", "description": "Optional target episode/segment count." },
                            "episodeDuration": { "type": "string", "description": "Optional per-episode/per-segment duration." },
                            "projectId": { "type": "string", "description": "Optional output id under dramas/." },
                            "outDir": { "type": "string", "description": "Optional project-relative output directory. Default dramas/." },
                        },
                    },
                    "storyboardCreate": {
                        "type": "object",
                        "description": "Structured execution args for action=storyboard_create.",
                        "properties": {
                            "title": { "type": "string", "description": "Confirmed storyboard project title." },
                            "sourceKind": { "type": "string", "description": "Source type, e.g. script, novel excerpt, idea, scene list." },
                            "sourceText": { "type": "string", "description": "User-provided source text. For long sources, prefer sourcePath instead of summarizing." },
                            "sourcePath": { "type": "string", "description": "Optional project-relative source file path." },
                            "requirements": { "type": "string", "description": "Confirmed shot/storyboard requirements." },
                            "visualStyle": { "type": "string", "description": "Confirmed visual style, if the user specified one." },
                            "aspectRatio": { "type": "string", "description": "Confirmed aspect ratio, e.g. 9:16, 16:9, 1:1." },
                            "granularity": { "type": "string", "description": "Confirmed storyboard granularity." },
                            "maxShots": { "type": "number", "description": "Optional max shot count." },
                            "projectId": { "type": "string", "description": "Optional output id under storyboards/." },
                            "outDir": { "type": "string", "description": "Optional project-relative output directory. Default storyboards/." },
                        },
                    },
                    "interactiveFilmCreate": {
                        "type": "object",
                        "description": "Structured execution args for action=interactive_film_create.",
                        "properties": {
                            "title": { "type": "string", "description": "Confirmed interactive-film project title." },
                            "sourceKind": { "type": "string", "description": "Source type, e.g. novel excerpt, script, outline, original idea." },
                            "sourceText": { "type": "string", "description": "User-provided source text. For long sources, prefer sourcePath instead of summarizing." },
                            "sourcePath": { "type": "string", "description": "Optional project-relative source file path." },
                            "requirements": { "type": "string", "description": "Confirmed branching, variable/flag, ending, production, visual, or market requirements." },
                            "targetAudience": { "type": "string", "description": "Confirmed target audience or market." },
                            "episodeCount": { "type": "number", "description": "Optional target episode/segment count." },
                            "episodeDuration": { "type": "string", "description": "Optional per-episode/per-segment duration." },
                            "budget": { "type": "string", "description": "Optional budget or production constraints." },
                            "referenceMode": { "type": "string", "description": "Optional reference mode, e.g. 盛世天下-style multi-ending interactive drama." },
                            "projectId": { "type": "string", "description": "Optional output id under interactive-films/." },
                            "outDir": { "type": "string", "description": "Optional project-relative output directory. Default interactive-films/." },
                        },
                    },
                    "translationCreate": {
                        "type": "object",
                        "description": "Structured execution args for action=translation_create.",
                        "properties": {
                            "filePath": { "type": "string", "description": "Project-relative EPUB/PDF/TXT/Markdown source file path to translate." },
                            "sourceLanguage": { "type": "string", "description": "Source language as a human-readable name, e.g. Auto detect, Japanese, English, Chinese (Simplified), 繁体中文（台湾）. Do not require ISO abbreviations." },
                            "targetLanguage": { "type": "string", "description": "Target language as a human-readable name, e.g. Chinese (Simplified), English, Japanese, Korean, Brazilian Portuguese. Do not require ISO abbreviations." },
                            "title": { "type": "string", "description": "Optional translation project title." },
                            "segmentMaxChars": { "type": "number", "description": "Optional long-paragraph split threshold." },
                        },
                    },
                    "fanficCreate": {
                        "type": "object",
                        "description": "Structured execution args for action=fanfic_init. This creates the book directly after confirmation.",
                        "properties": {
                            "title": { "type": "string", "description": "Confirmed fanfiction book title." },
                            "sourceText": { "type": "string", "description": "Provided canon/source text. Prefer sourcePath for uploaded or long files." },
                            "sourcePath": { "type": "string", "description": "Project-relative uploaded canon/source file path." },
                            "sourceName": { "type": "string", "description": "Human-readable source work name." },
                            "mode": { "type": "string", "enum": ["canon", "au", "ooc", "cp"], "description": "Confirmed fanfiction mode." },
                            "genre": { "type": "string", "description": "Confirmed genre." },
                            "platform": { "type": "string", "enum": ["tomato", "qidian", "feilu", "other"] },
                            "language": { "type": "string", "enum": ["zh", "en"] },
                            "targetChapters": { "type": "number", "description": "Confirmed total chapter count." },
                            "chapterWordCount": { "type": "number", "description": "Confirmed per-chapter length." },
                        },
                    },
                    "continuationImport": {
                        "type": "object",
                        "description": "Structured execution args for action=continuation_import. This imports and rebuilds state directly after confirmation.",
                        "properties": {
                            "bookId": { "type": "string", "description": "Existing target book id. Omit when creating a new continuation book." },
                            "title": { "type": "string", "description": "New continuation book title when bookId is omitted." },
                            "sourcePath": { "type": "string", "description": "Project-relative uploaded novel file or chapter directory." },
                            "splitPattern": { "type": "string", "description": "Optional custom chapter-heading regex source." },
                            "resumeFrom": { "type": "number", "description": "Resume interrupted replay from this 1-based chapter number." },
                            "genre": { "type": "string", "description": "Genre for a newly created continuation book." },
                            "platform": { "type": "string", "enum": ["tomato", "qidian", "feilu", "other"] },
                            "language": { "type": "string", "enum": ["zh", "en"] },
                            "targetChapters": { "type": "number", "description": "Target total chapters for a new book." },
                            "chapterWordCount": { "type": "number", "description": "Per-chapter length for a new book." },
                        },
                    },
                    "spinoffCreate": {
                        "type": "object",
                        "description": "Structured execution args for action=spinoff_create. This creates the side-story directly after confirmation.",
                        "properties": {
                            "title": { "type": "string", "description": "Confirmed side-story title." },
                            "parentBookId": { "type": "string", "description": "Existing InkOS parent book id whose canon is inherited." },
                            "direction": { "type": "string", "description": "Confirmed standalone side-story direction." },
                            "genre": { "type": "string", "description": "Optional genre override; defaults to the parent book." },
                            "platform": { "type": "string", "enum": ["tomato", "qidian", "feilu", "other"] },
                            "language": { "type": "string", "enum": ["zh", "en"] },
                            "targetChapters": { "type": "number", "description": "Optional chapter count; defaults to the parent book." },
                            "chapterWordCount": { "type": "number", "description": "Optional chapter length; defaults to the parent book." },
                        },
                    },
                    "imitationCreate": {
                        "type": "object",
                        "description": "Structured execution args for action=style_imitation. This creates an original book and style guide directly after confirmation.",
                        "properties": {
                            "title": { "type": "string", "description": "Confirmed original imitation-project title." },
                            "referenceText": { "type": "string", "description": "Reference prose. Prefer referencePath for uploaded or long files." },
                            "referencePath": { "type": "string", "description": "Project-relative uploaded reference-work path." },
                            "storyIdea": { "type": "string", "description": "Confirmed original story idea; do not copy the reference plot." },
                            "sourceName": { "type": "string", "description": "Human-readable reference work name." },
                            "genre": { "type": "string", "description": "Confirmed genre." },
                            "platform": { "type": "string", "enum": ["tomato", "qidian", "feilu", "other"] },
                            "language": { "type": "string", "enum": ["zh", "en"] },
                            "targetChapters": { "type": "number", "description": "Confirmed total chapter count." },
                            "chapterWordCount": { "type": "number", "description": "Confirmed per-chapter length." },
                        },
                    },
                },
                "required": ["action", "instruction"],
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_deps() -> ProposeDeps<'static> {
        ProposeDeps {
            language: "zh",
            same_session: false,
            requested_skills: &[],
        }
    }

    #[test]
    fn session_kind_and_route_mappings() {
        assert_eq!(proposed_action_session_kind("create_book"), "book-create");
        assert_eq!(proposed_action_session_kind("short_run"), "short");
        assert_eq!(proposed_action_session_kind("translation_create"), "chat");
        assert_eq!(proposed_action_session_kind("remove_node"), "interactive-film-authoring");
        assert_eq!(proposed_action_target_route("fanfic_init"), Some("import:fanfic"));
        assert_eq!(proposed_action_target_route("continuation_import"), Some("import:chapters"));
        assert_eq!(proposed_action_target_route("style_imitation"), Some("import:imitation"));
        assert_eq!(proposed_action_target_route("create_book"), None);
        // 回退文案：生产动作 vs 辅助入口。
        assert_eq!(proposed_action_fallback_title("create_book", false), "创建长篇书籍");
        assert_eq!(proposed_action_fallback_title("create_book", true), "Create a long-form book");
        assert_eq!(proposed_action_fallback_title("remove_node", false), "删除节点");
        assert!(proposed_action_fallback_summary("create_book", false).contains("切换到对应入口"));
        assert!(proposed_action_fallback_summary("fanfic_init", false).contains("只会打开现有 Studio 工具"));
    }

    #[test]
    fn initial_scene_usability() {
        assert!(is_usable_play_initial_scene(Some("雪夜的古宅里，灯火在穿堂风里明明灭灭，门厅深处传来一声轻响。")));
        // 过短。
        assert!(!is_usable_play_initial_scene(Some("太短的场景。")));
        // 句末悬垂（连词/标点）。
        assert!(!is_usable_play_initial_scene(Some("雪夜的古宅里灯火明灭，门厅深处似乎有人在，")));
        assert!(!is_usable_play_initial_scene(Some("他推开门，看见一个人影站在")));
        assert!(!is_usable_play_initial_scene(None));
    }

    #[test]
    fn compact_object_branches() {
        let compacted = compact_object(Some(&json!({
            "title": "  雪夜谜案 ",
            "genre": "  ",
            "reference": "",
            "targetChapters": 200,
            "chapterWordCount": 0,
            "chapterWordCount2": -5,
            "tags": ["a", " ", "b", ""],
            "emptyTags": ["", "  "],
            "cover": true
        })))
        .unwrap();
        assert_eq!(compacted["title"], "雪夜谜案");
        assert!(compacted.get("genre").is_none(), "空白字符串被剥：{compacted}");
        assert_eq!(compacted["targetChapters"], 200);
        assert!(compacted.get("chapterWordCount").is_none(), "非正数被剥");
        assert_eq!(compacted["tags"], json!(["a", "b"]));
        assert!(compacted.get("emptyTags").is_none(), "全空数组被剥");
        assert_eq!(compacted["cover"], true, "布尔保留");
        assert!(compact_object(Some(&json!({ "a": " " }))).is_none(), "空结果 → None");
    }

    #[tokio::test]
    async fn propose_action_card_and_payload() {
        let deps = make_deps();
        let result = tool_propose_action(
            &deps,
            &json!({
                "action": "create_book",
                "instruction": "写一本《雪夜谜案》悬疑小说，主角是刑警林昭。",
                "createBook": { "title": "雪夜谜案", "genre": "悬疑", "platform": "tomato", "targetChapters": 200 }
            }),
        )
        .await;
        assert!(!result.is_error, "{}", result.text);
        // 文本：回退标题 + 回退摘要 + Instruction。
        assert_eq!(
            result.text,
            "创建长篇书籍\n确认后会切换到对应入口并执行这条需求。\n\nInstruction: 写一本《雪夜谜案》悬疑小说，主角是刑警林昭。"
        );
        let details = result.details.unwrap();
        assert_eq!(details["kind"], "proposed_action");
        assert_eq!(details["action"], "create_book");
        assert_eq!(details["targetSessionKind"], "book-create");
        assert_eq!(details["sameSession"], false);
        assert!(details.get("targetRoute").is_none());
        assert_eq!(
            details["actionPayload"]["createBook"],
            json!({ "title": "雪夜谜案", "genre": "悬疑", "platform": "tomato", "targetChapters": 200 })
        );
    }

    #[tokio::test]
    async fn propose_action_short_run_injects_language_and_validates() {
        let mut deps = make_deps();
        let language = "en".to_string();
        deps.language = &language;
        let result = tool_propose_action(
            &deps,
            &json!({
                "action": "short_run",
                "instruction": "Write a short mystery.",
                "title": "Short",
                "summary": "Generates a short.",
                "shortRun": { "direction": "snowy manor mystery", "charsPerChapter": 700 }
            }),
        )
        .await;
        assert!(!result.is_error, "{}", result.text);
        let details = result.details.unwrap();
        assert_eq!(details["title"], "Short");
        assert_eq!(details["targetSessionKind"], "short");
        // 会话语言注入（模型未填 language）。
        assert_eq!(details["actionPayload"]["shortRun"]["language"], "en");
        assert_eq!(details["actionPayload"]["shortRun"]["charsPerChapter"], 700);
        // en 700 合法（600-800）。

        // zh 会话 700 越界（900-1200）→ strict 校验拒绝。
        let deps_zh = make_deps();
        let invalid = tool_propose_action(
            &deps_zh,
            &json!({
                "action": "short_run",
                "instruction": "写短篇",
                "shortRun": { "direction": "雪夜", "language": "zh", "charsPerChapter": 700 }
            }),
        )
        .await;
        assert!(invalid.is_error);
        assert!(invalid.text.starts_with("Invalid proposed action payload:"), "{}", invalid.text);
    }

    #[tokio::test]
    async fn propose_action_missing_field_and_play_compaction() {
        let deps = make_deps();
        // create_book 缺 title → 固定错误文案。
        let missing = tool_propose_action(
            &deps,
            &json!({ "action": "create_book", "instruction": "写书", "createBook": { "genre": "悬疑" } }),
        )
        .await;
        assert!(missing.is_error);
        assert_eq!(
            missing.text,
            "propose_action is missing /createBook/title; retry with that field in the structured payload, not only in summary or instruction."
        );
        // translation_create 三必填。
        let translation = tool_propose_action(
            &deps,
            &json!({
                "action": "translation_create",
                "instruction": "翻译这本书",
                "translationCreate": { "filePath": " books/a.epub ", "sourceLanguage": "日语" }
            }),
        )
        .await;
        assert!(translation.is_error);
        assert!(translation.text.contains("propose_action is missing /translationCreate/targetLanguage"), "{}", translation.text);

        // play_start：initialScene 悬垂被剥 → 必填断言拦截。
        let play = tool_propose_action(
            &deps,
            &json!({
                "action": "play_start",
                "instruction": "开世界",
                "playStart": {
                    "title": "雪夜古宅",
                    "premise": "大雪封山的旧宅。",
                    "initialScene": "他推开门，看见",
                    "suggestedActions": [{ "label": " 查看门厅 " }, "上二楼", "", { "text": "听动静" }, "第五个"]
                }
            }),
        )
        .await;
        assert!(play.is_error);
        assert!(play.text.contains("propose_action is missing /playStart/initialScene"), "{}", play.text);

        // 合法 play_start：suggestedActions 对象归一 + ≤4。
        let ok = tool_propose_action(
            &deps,
            &json!({
                "action": "play_start",
                "instruction": "开世界",
                "playStart": {
                    "title": "雪夜古宅",
                    "premise": "大雪封山的旧宅。",
                    "initialScene": "雪夜的古宅里，灯火在穿堂风里明明灭灭，门厅深处传来一声轻响。",
                    "suggestedActions": [{ "label": " 查看门厅 " }, "上二楼", "", { "text": "听动静" }, "第五个"]
                }
            }),
        )
        .await;
        assert!(!ok.is_error, "{}", ok.text);
        let details = ok.details.unwrap();
        assert_eq!(details["targetSessionKind"], "play");
        assert_eq!(
            details["actionPayload"]["playStart"]["suggestedActions"],
            json!(["查看门厅", "上二楼", "听动静", "第五个"])
        );

        // 非法 action / 缺 instruction。
        let bad_action = tool_propose_action(&deps, &json!({ "action": "bogus", "instruction": "x" })).await;
        assert!(bad_action.is_error && bad_action.text.contains("Invalid propose_action.action"));
        let no_instruction = tool_propose_action(&deps, &json!({ "action": "short_run" })).await;
        assert!(no_instruction.is_error && no_instruction.text.contains("requires an instruction"));
    }

    #[test]
    fn schema_shape_and_requested_skills() {
        let schema = propose_action_schema();
        assert_eq!(schema["function"]["name"], "propose_action");
        assert_eq!(schema["function"]["parameters"]["required"], json!(["action", "instruction"]));
        // 301 号：12 个结构化子域全宣传（含四件创建域）。
        let props = &schema["function"]["parameters"]["properties"];
        for key in ["createBook", "shortRun", "playStart", "generateCover", "scriptCreate", "storyboardCreate", "interactiveFilmCreate", "translationCreate", "fanficCreate", "continuationImport", "spinoffCreate", "imitationCreate"] {
            assert!(props.get(key).is_some(), "schema 缺 {key}");
        }
        assert_eq!(
            schema["function"]["parameters"]["properties"]["action"]["enum"].as_array().unwrap().len(),
            15
        );
        // requestedSkills 非空时进 details。
        let skills = vec!["style-a".to_string(), "Style-A".to_string()];
        let deps = ProposeDeps { language: "zh", same_session: true, requested_skills: &skills };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(tool_propose_action(
            &deps,
            &json!({
                "action": "fanfic_init",
                "instruction": "开同人",
                "fanficCreate": { "title": "同人书", "sourcePath": ".inkos/uploads/canon/a.txt" }
            }),
        ));
        assert!(result.text.starts_with("打开同人创作"), "actual: {}", result.text);
        let details = result.details.unwrap();
        assert_eq!(details["targetRoute"], "import:fanfic");
        assert_eq!(details["sameSession"], true);
        assert_eq!(details["requestedSkills"], json!(["style-a", "Style-A"]));
        assert_eq!(details["actionPayload"]["fanficCreate"]["title"], "同人书");

        // 301 号：四件创建域必填断言。
        let no_title = runtime.block_on(tool_propose_action(
            &deps,
            &json!({ "action": "fanfic_init", "instruction": "开同人" }),
        ));
        assert!(no_title.is_error && no_title.text.contains("/fanficCreate/title"), "{}", no_title.text);
        let no_source = runtime.block_on(tool_propose_action(
            &deps,
            &json!({ "action": "continuation_import", "instruction": "续写导入" }),
        ));
        assert!(no_source.is_error && no_source.text.contains("/continuationImport/sourcePath"), "{}", no_source.text);
        let no_parent = runtime.block_on(tool_propose_action(
            &deps,
            &json!({ "action": "spinoff_create", "instruction": "开番外", "spinoffCreate": { "title": "番外" } }),
        ));
        assert!(no_parent.is_error && no_parent.text.contains("/spinoffCreate/parentBookId"), "{}", no_parent.text);
        let no_idea = runtime.block_on(tool_propose_action(
            &deps,
            &json!({ "action": "style_imitation", "instruction": "仿写", "imitationCreate": { "title": "仿写书", "referenceText": "林动睁开双眼。" } }),
        ));
        assert!(no_idea.is_error && no_idea.text.contains("/imitationCreate/storyIdea"), "{}", no_idea.text);

        // 合法 imitation：payload 带结构化子域。
        let ok_imit = runtime.block_on(tool_propose_action(
            &deps,
            &json!({
                "action": "style_imitation",
                "instruction": "仿写",
                "imitationCreate": { "title": "仿写书", "storyIdea": "全新的江湖故事", "referenceText": "林动睁开双眼。" }
            }),
        ));
        assert!(!ok_imit.is_error, "{}", ok_imit.text);
        let ok_details = ok_imit.details.unwrap();
        assert_eq!(ok_details["actionPayload"]["imitationCreate"]["storyIdea"], "全新的江湖故事");
    }
}
