//! 确认式生产任务分支（server.ts L5075-L5254 + executeConfirmedProductionAction
//! L1587-L1869）——POST /agent 的任务执行路径。
//!
//! 67 号交付面：write_next / create_book 两个意图执行器 + 任务分支骨架
//! （单任务闸门 + task 快照 + user 消息先写 + 建书迁移 + 广播 + finally 释放）。
//! 其余 9 个确认意图（short_run / generate_cover / script_create / storyboard_create /
//! interactive_film_create / translation_create / play_start / draft_structure /
//! connect_choice / remove_node）随对应业务域迁移后接线，当前落
//! `Unsupported confirmed action` 错误路径（偏差备案见 67 号记录）。
//!
//! ## 关键语义（逐字对齐 TS）
//! - **单任务闸门**：`reservedProductionSessions` 在任何 await 之前**同步预留**
//!   （sessionId → taskId），并发的第二确认请求直接 409；任务终态后释放。
//! - **快照对账**：running 快照 + 本进程注册表无该任务 id → 视为旧进程遗留，
//!   改写 error 终态落盘（否则前端恢复出永远运行中的任务卡）。
//! - **user 消息先写**：任务运行期间刷新页面时用户气泡可从 transcript 恢复；
//!   完成/失败只追加助手工具消息（instruction 不写第二遍）。
//! - **错误统一面**：执行器内一切失败（含 payload 缺失）由分支 catch 统一经
//!   `formatAgentActionFailure`：busy → 409 BOOK_BUSY，其余 → 502
//!   AGENT_ACTION_FAILED（TS 行为：ApiError 在 catch 里被转写，原 code 不透出）。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use axum::http::StatusCode;
use serde_json::{json, Map, Value};

use crate::interaction::agent_loop::AbortHandle;
use crate::interaction::session::{utc_now_ms, SessionKind};
use crate::interaction::session_transcript::{append_transcript_events, TranscriptEvent};
use crate::server::books_routes::BooksRuntime;
use crate::server::task_store::{
    load_studio_task_snapshot, save_studio_task_snapshot, StudioTaskExecution,
    StudioTaskExecutionStatus, StudioTaskSnapshot, StudioTaskStage, StudioTaskStageStatus,
};

// ── 枚举（zod 枚举逐值对齐） ─────────────────────────────────────

/// `ActionSourceSchema`（四值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionSource {
    FreeText,
    Button,
    Slash,
    QuickAction,
}

impl ActionSource {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "free-text" => Some(Self::FreeText),
            "button" => Some(Self::Button),
            "slash" => Some(Self::Slash),
            "quick-action" => Some(Self::QuickAction),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FreeText => "free-text",
            Self::Button => "button",
            Self::Slash => "slash",
            Self::QuickAction => "quick-action",
        }
    }
}

/// `normalizeStudioActionSource`：空 → free-text；非法 → 400 INVALID_ACTION_SOURCE。
pub fn normalize_action_source(value: Option<&Value>) -> Result<ActionSource, String> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Ok(ActionSource::FreeText);
    };
    if text.is_empty() {
        return Ok(ActionSource::FreeText);
    }
    ActionSource::parse(text).ok_or_else(|| format!("Invalid actionSource: {text}"))
}

/// `RequestedIntentSchema`（18 值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestedIntent {
    CreateBook,
    WriteNext,
    ShortRun,
    PlayStart,
    PlayStep,
    GenerateCover,
    EditArtifact,
    FanficInit,
    ContinuationImport,
    SpinoffCreate,
    StyleImitation,
    ScriptCreate,
    StoryboardCreate,
    InteractiveFilmCreate,
    TranslationCreate,
    DraftStructure,
    ConnectChoice,
    RemoveNode,
}

impl RequestedIntent {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "create_book" => Self::CreateBook,
            "write_next" => Self::WriteNext,
            "short_run" => Self::ShortRun,
            "play_start" => Self::PlayStart,
            "play_step" => Self::PlayStep,
            "generate_cover" => Self::GenerateCover,
            "edit_artifact" => Self::EditArtifact,
            "fanfic_init" => Self::FanficInit,
            "continuation_import" => Self::ContinuationImport,
            "spinoff_create" => Self::SpinoffCreate,
            "style_imitation" => Self::StyleImitation,
            "script_create" => Self::ScriptCreate,
            "storyboard_create" => Self::StoryboardCreate,
            "interactive_film_create" => Self::InteractiveFilmCreate,
            "translation_create" => Self::TranslationCreate,
            "draft_structure" => Self::DraftStructure,
            "connect_choice" => Self::ConnectChoice,
            "remove_node" => Self::RemoveNode,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreateBook => "create_book",
            Self::WriteNext => "write_next",
            Self::ShortRun => "short_run",
            Self::PlayStart => "play_start",
            Self::PlayStep => "play_step",
            Self::GenerateCover => "generate_cover",
            Self::EditArtifact => "edit_artifact",
            Self::FanficInit => "fanfic_init",
            Self::ContinuationImport => "continuation_import",
            Self::SpinoffCreate => "spinoff_create",
            Self::StyleImitation => "style_imitation",
            Self::ScriptCreate => "script_create",
            Self::StoryboardCreate => "storyboard_create",
            Self::InteractiveFilmCreate => "interactive_film_create",
            Self::TranslationCreate => "translation_create",
            Self::DraftStructure => "draft_structure",
            Self::ConnectChoice => "connect_choice",
            Self::RemoveNode => "remove_node",
        }
    }
}

/// `normalizeStudioRequestedIntent`：空 → None；非法 → 400 INVALID_REQUESTED_INTENT。
pub fn normalize_requested_intent(value: Option<&Value>) -> Result<Option<RequestedIntent>, String> {
    let Some(text) = value.and_then(Value::as_str) else {
        return Ok(None);
    };
    if text.is_empty() {
        return Ok(None);
    }
    RequestedIntent::parse(text)
        .map(Some)
        .ok_or_else(|| format!("Invalid requestedIntent: {text}"))
}

// ── 写章启发式（action-envelope.ts 逐字移植） ────────────────────

fn write_next_instruction_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"(?i)^(continue|继续|继续写|写下一章|write next|下一章|再来一章)$")
            .unwrap()
    })
}

/// `isWriteNextInstruction`（无 allowSlashWrite 调用形态）。
pub fn is_write_next_instruction(instruction: &str) -> bool {
    write_next_instruction_re().is_match(instruction.trim())
}

fn explicit_write_chapter_zh_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r"^(?:请|帮我|麻烦|现在|直接|开始|继续|接着|再)?\s*(?:写|续写|创作|生成)(?:出|一下)?\s*(?:第?\s*一\s*章|第?\s*1\s*章|下一章|一章|正文|章节)(?:\s|[，。,.！!？?；;：:]|$)",
        )
        .unwrap()
    })
}

fn explicit_write_chapter_en_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)^(?:please\s+)?(?:write|continue|draft|generate)\s+(?:(?:the\s+)?next\s+chapter|chapter(?:\s+(?:1|one))?)\b",
        )
        .unwrap()
    })
}

/// `isExplicitWriteChapterCommand`。
pub fn is_explicit_write_chapter_command(instruction: &str) -> bool {
    let trimmed = instruction.trim();
    if trimmed.is_empty() {
        return false;
    }
    if explicit_write_chapter_zh_re().is_match(trimmed) {
        return true;
    }
    explicit_write_chapter_en_re().is_match(trimmed)
}

/// `isWriteNextProductionRequest`：有书 + 书籍会话，且（显式 write_next intent /
/// free-text 明确写章命令 / 其它来源的写作指令启发式）。
pub fn is_write_next_production_request(
    instruction: &str,
    agent_book_id: Option<&str>,
    session_kind: SessionKind,
    action_source: ActionSource,
    requested_intent: Option<RequestedIntent>,
) -> bool {
    if agent_book_id.is_none() || session_kind != SessionKind::Book {
        return false;
    }
    if requested_intent == Some(RequestedIntent::WriteNext) {
        return true;
    }
    if action_source == ActionSource::FreeText {
        return is_explicit_write_chapter_command(instruction);
    }
    is_write_next_instruction(instruction)
}

/// `isConfirmedProductionAction`：button/slash 来源的 11 个确认 intent。
pub fn is_confirmed_production_action(action_source: ActionSource, intent: RequestedIntent) -> bool {
    if !matches!(action_source, ActionSource::Button | ActionSource::Slash) {
        return false;
    }
    matches!(
        intent,
        RequestedIntent::CreateBook
            | RequestedIntent::ShortRun
            | RequestedIntent::ScriptCreate
            | RequestedIntent::StoryboardCreate
            | RequestedIntent::InteractiveFilmCreate
            | RequestedIntent::TranslationCreate
            | RequestedIntent::PlayStart
            | RequestedIntent::GenerateCover
            | RequestedIntent::DraftStructure
            | RequestedIntent::ConnectChoice
            | RequestedIntent::RemoveNode
    )
}

/// /agent 模型解析产物（97 号四层解析）：per-request 端点覆盖。
#[derive(Debug, Clone)]
pub struct AgentModelOverride {
    pub service: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    /// 传输协议（106 号）：命中服务项 apiFormat=responses 时置位（层 1/2）。
    pub api_format: crate::llm::providers::TransportApiFormat,
}

/// /agent 模型四层解析（TS 4968-5075 逐层）：
/// 1. 前端显式 service+model——无 key 且非本地端点 → 400 双语（逐字）；
/// 2. 新配置 defaultModel + services[0]（静默失败下落）；
/// 3. secrets 首个有 key 服务的首个文本模型（listModels live+bank；静默）；
/// 4. None → 项目配置端点（现状兜底）。
///
/// 返回 Err = 层 1 的 400 响应（{error, response} 双键）。
pub async fn resolve_agent_model_override(
    root: &std::path::Path,
    service: Option<&str>,
    model: Option<&str>,
) -> Result<Option<AgentModelOverride>, (axum::http::StatusCode, axum::Json<serde_json::Value>)> {
    use axum::http::StatusCode;
    use serde_json::json;

    let resolve_key = |service: &str| -> Option<String> {
        let secrets = crate::llm::secrets::load_secrets(root).unwrap_or_default();
        crate::llm::secrets::resolve_service_api_key(&secrets, service, |name| std::env::var(name).ok())
            .filter(|key| !key.trim().is_empty())
    };

    // ── 层 1：前端显式 service+model。 ──
    if let (Some(service), Some(model)) = (service.map(str::trim).filter(|s| !s.is_empty()), model.map(str::trim).filter(|s| !s.is_empty())) {
        let Some(base_url) =
            crate::server::service_routes::resolve_configured_service_base_url(root, service, None).await
        else {
            // TS：resolveServiceModel 无 baseUrl 抛错（非 key 错 → 500）。
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({ "error": format!("Cannot resolve model \"{model}\" for service \"{service}\": no baseUrl available.") })),
            ));
        };
        let api_key = resolve_key(service);
        let api_format = crate::server::service_routes::resolve_configured_service_api_format(root, service)
            .await
            .unwrap_or(crate::llm::providers::TransportApiFormat::Chat);
        let key_optional = crate::utils::llm_endpoint_auth::is_api_key_optional_for_endpoint("openai", Some(&base_url));
        match (api_key, key_optional) {
            (Some(api_key), _) => {
                return Ok(Some(AgentModelOverride { service: service.to_string(), model: model.to_string(), api_key, base_url, api_format }));
            }
            (None, false) => {
                let lang = current_project_language(root).await;
                let (error, response) = match lang {
                    StudioLang::En => (
                        format!("Configure an API Key for {service} first"),
                        format!("Fill in an API Key for {service} in the model settings, then try again."),
                    ),
                    StudioLang::Zh => (
                        format!("请先为 {service} 配置 API Key"),
                        format!("请先在模型配置中为 {service} 填写 API Key，然后再试。"),
                    ),
                };
                return Err((StatusCode::BAD_REQUEST, axum::Json(json!({ "error": error, "response": response }))));
            }
            (None, true) => {
                // 本地端点（Ollama 等）无 key 可用。
                return Ok(Some(AgentModelOverride { service: service.to_string(), model: model.to_string(), api_key: String::new(), base_url, api_format }));
            }
        }
    }

    // ── 层 2：新配置 defaultModel + services[0]。 ──
    if let Some(config) = crate::server::project_config_routes::load_raw_config(root).await {
        if let Some(llm) = config.get("llm") {
            let default_model = llm
                .get("defaultModel")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|m| !m.is_empty());
            let services = crate::server::service_routes::normalize_service_config(llm.get("services"));
            if let (Some(first), Some(default_model)) = (services.first(), default_model) {
                if is_text_chat_model_id(default_model) {
                    let service = crate::server::service_routes::service_config_key(first);
                    if let (Some(base_url), Some(api_key)) = (
                        crate::server::service_routes::resolve_configured_service_base_url(root, &service, None).await,
                        resolve_key(&service),
                    ) {
                        if !base_url.is_empty() && !api_key.is_empty() {
                            // 服务项 apiFormat（106 号）：TS selectedEntry.apiFormat 镜像。
                            let api_format = first
                                .api_format
                                .as_deref()
                                .and_then(|value| match value {
                                    "responses" => Some(crate::llm::providers::TransportApiFormat::Responses),
                                    "chat" => Some(crate::llm::providers::TransportApiFormat::Chat),
                                    _ => None,
                                })
                                .unwrap_or(crate::llm::providers::TransportApiFormat::Chat);
                            return Ok(Some(AgentModelOverride { service, model: default_model.to_string(), api_key, base_url, api_format }));
                        }
                    }
                }
            }
        }
    }

    // ── 层 3：secrets 首个有 key 服务的首个文本模型。 ──
    if let Ok(secrets) = crate::llm::secrets::load_secrets(root) {
        // TS Object.entries 为 JSON 插入序——104 号起按磁盘键序迭代（sidecar
        // 写入的 secrets.json 保序）；读不到键序时按名排序回退（97 号备案闭合）。
        let mut order = crate::llm::secrets::service_key_order(root);
        order.retain(|name| secrets.services.contains_key(name));
        let mut rest: Vec<String> = secrets
            .services
            .keys()
            .filter(|name| !order.contains(name))
            .cloned()
            .collect();
        rest.sort();
        order.extend(rest);
        for service in order {
            let Some(secret) = secrets.services.get(&service) else {
                continue;
            };
            if secret.api_key.trim().is_empty() {
                continue;
            }
            let models = crate::llm::probe::list_models_for_service(&service, Some(&secret.api_key), None).await;
            if let Some(text_model) = models.iter().find(|m| is_text_chat_model_id(&m.id)) {
                if let Some(base_url) =
                    crate::server::service_routes::resolve_configured_service_base_url(root, &service, None).await
                {
                    if !base_url.is_empty() {
                        return Ok(Some(AgentModelOverride {
                            service: service.clone(),
                            model: text_model.id.clone(),
                            api_key: secret.api_key.clone(),
                            base_url,
                            api_format: crate::llm::providers::TransportApiFormat::Chat,
                        }));
                    }
                }
            }
        }
    }

    // ── 层 4：项目配置端点（None）。 ──
    Ok(None)
}

/// 非文本模型 id 片段（子串匹配，lower+trim）。对齐 TS `NON_TEXT_MODEL_ID_PARTS`。
pub const NON_TEXT_MODEL_ID_PARTS: &[&str] = &[
    "image",
    "embedding",
    "embed",
    "rerank",
    "tts",
    "speech",
    "audio",
    "moderation",
];

/// `isTextChatModelId`：非空且不含任何非文本片段。
pub fn is_text_chat_model_id(model_id: &str) -> bool {
    let normalized = model_id.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    !NON_TEXT_MODEL_ID_PARTS.iter().any(|part| normalized.contains(part))
}

/// `nonTextModelMessage`：双语逐字。
pub fn non_text_model_message(model_id: &str, lang: StudioLang) -> String {
    pick(
        lang,
        &format!("模型 {model_id} 不适合文本聊天/写作。请在模型选择器中改用文本模型，例如 gemini-2.5-flash、gemini-2.5-pro 或对应服务的 chat 模型。"),
        &format!("Model {model_id} is not suitable for text chat/writing. Pick a text model in the model selector, e.g. gemini-2.5-flash, gemini-2.5-pro, or the service's chat model."),
    )
}

/// 归一确认意图：写章三来源 → write_next；button/slash 确认 intent → 原值；
/// 其余 → None（走聊天分支）。
pub fn resolve_confirmed_intent(
    instruction: &str,
    agent_book_id: Option<&str>,
    session_kind: SessionKind,
    action_source: ActionSource,
    requested_intent: Option<RequestedIntent>,
) -> Option<RequestedIntent> {
    if is_write_next_production_request(
        instruction,
        agent_book_id,
        session_kind,
        action_source,
        requested_intent,
    ) {
        return Some(RequestedIntent::WriteNext);
    }
    match requested_intent {
        Some(intent) if is_confirmed_production_action(action_source, intent) => Some(intent),
        _ => None,
    }
}

// ── 标签与阶段表（双语） ─────────────────────────────────────────

const AGENT_LABELS: &[(&str, &str, &str)] = &[
    ("architect", "建书", "Book setup"),
    ("writer", "写作", "Writing"),
    ("auditor", "审计", "Audit"),
    ("reviser", "修订", "Revision"),
    ("exporter", "导出", "Export"),
];

const TOOL_LABELS: &[(&str, &str, &str)] = &[
    ("read", "读取文件", "Read file"),
    ("edit", "编辑文件", "Edit file"),
    ("grep", "搜索", "Search"),
    ("ls", "列目录", "List directory"),
    ("propose_action", "确认动作", "Confirm action"),
    ("short_fiction_run", "短篇生产", "Short fiction"),
    ("script_create", "剧本创作", "Script creation"),
    ("storyboard_create", "分镜创作", "Storyboard creation"),
    ("interactive_film_create", "互动影游", "Interactive film"),
    ("translation_create", "翻译项目", "Translation"),
    ("generate_cover", "生成封面", "Cover generation"),
    ("play_edit", "编辑互动世界", "Edit interactive world"),
    ("play_start", "启动互动世界", "Start interactive world"),
    ("play_revise", "重做互动回合", "Redo interactive turn"),
    ("play_step", "推进互动世界", "Advance interactive world"),
    ("create_narrative_forecast", "剧情多线推演", "Narrative forecast"),
    ("get_narrative_forecast", "核验剧情推演", "Recheck forecast"),
    ("select_narrative_branch", "采用候选分支", "Select candidate branch"),
];

/// 项目语言（TS `currentProjectLanguage`：raw config `language`，默认 zh）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioLang {
    Zh,
    En,
}

pub async fn current_project_language(root: &Path) -> StudioLang {
    let raw = crate::server::project_config_routes::load_raw_config(root).await;
    match raw
        .as_ref()
        .and_then(|config| config.get("language"))
        .and_then(Value::as_str)
    {
        Some("en") => StudioLang::En,
        _ => StudioLang::Zh,
    }
}

fn pick(lang: StudioLang, zh: &str, en: &str) -> String {
    match lang {
        StudioLang::Zh => zh.to_string(),
        StudioLang::En => en.to_string(),
    }
}

/// `pipelineStages`（PIPELINE_STAGES 表）。
fn pipeline_stages(agent: &str, lang: StudioLang) -> Option<Vec<String>> {
    let stages: &[(&str, &str)] = match agent {
        "writer" => &[
            ("准备章节输入", "Prepare chapter input"),
            ("撰写章节草稿", "Write chapter draft"),
            ("落盘最终章节", "Save final chapter"),
            ("生成最终真相文件", "Generate final truth files"),
            ("校验真相文件变更", "Validate truth file changes"),
            ("同步记忆索引", "Sync memory index"),
            ("更新章节索引与快照", "Update chapter index and snapshot"),
        ],
        "architect" => &[
            ("生成基础设定", "Generate foundation"),
            ("保存书籍配置", "Save book config"),
            ("写入基础设定文件", "Write foundation files"),
            ("初始化控制文档", "Initialize control documents"),
            ("创建初始快照", "Create initial snapshot"),
        ],
        "reviser" => &[
            ("加载修订上下文", "Load revision context"),
            ("修订章节", "Revise chapter"),
            ("落盘修订结果", "Save revision result"),
            ("更新索引与快照", "Update index and snapshot"),
        ],
        "auditor" => &[("审计章节", "Audit chapter")],
        _ => return None,
    };
    Some(stages.iter().map(|(zh, en)| pick(lang, zh, en)).collect())
}

/// `resolveToolLabel`。
fn resolve_tool_label(tool: &str, agent: Option<&str>, lang: StudioLang) -> String {
    if tool == "sub_agent" {
        if let Some(agent) = agent {
            if let Some((_, zh, en)) = AGENT_LABELS.iter().find(|(name, _, _)| *name == agent) {
                return pick(lang, zh, en);
            }
            return agent.to_string();
        }
    }
    if let Some((_, zh, en)) = TOOL_LABELS.iter().find(|(name, _, _)| *name == tool) {
        return pick(lang, zh, en);
    }
    tool.to_string()
}

/// `formatTaskElapsed`。
fn format_task_elapsed(ms: u64, lang: StudioLang) -> String {
    let total_seconds = ms / 1000;
    let (minutes, seconds) = (total_seconds / 60, total_seconds % 60);
    if minutes == 0 {
        return pick(lang, &format!("{seconds} 秒"), &format!("{seconds}s"));
    }
    pick(
        lang,
        &format!("{minutes} 分 {seconds} 秒"),
        &format!("{minutes}m {seconds}s"),
    )
}

/// `buildRunningTaskContextBlock`：运行中任务状态注入聊天 agent 系统提示词。
pub fn build_running_task_context_block(task: &StudioTaskSnapshot, lang: StudioLang) -> String {
    let exec = &task.execution;
    let elapsed = format_task_elapsed(
        (utc_now_ms() as f64 - exec.started_at).max(0.0) as u64,
        lang,
    );
    let status = if exec.status == StudioTaskExecutionStatus::Processing {
        pick(lang, "处理中", "processing")
    } else {
        pick(lang, "运行中", "running")
    };
    let logs_tail: Vec<String> = exec
        .logs
        .as_deref()
        .map(|logs| logs.iter().rev().take(3).rev().cloned().collect())
        .unwrap_or_default();
    let logs_block = if !logs_tail.is_empty() {
        format!(
            "\n{}\n{}",
            pick(lang, "- 最近日志：", "- Recent logs:"),
            logs_tail
                .iter()
                .map(|line| format!("  - {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else {
        String::new()
    };
    let lines = [
        pick(lang, "## 后台任务状态", "## Background task status"),
        pick(
            lang,
            "本会话有一个正在后台运行的生产任务：",
            "A production task is currently running in the background of this session:",
        ),
        format!(
            "- {}：{}（{}）",
            pick(lang, "任务", "Task"),
            exec.label,
            exec.tool
        ),
        format!("- {}：{status}", pick(lang, "状态", "Status")),
        format!(
            "- {}：{elapsed}{logs_block}",
            pick(lang, "已运行", "Elapsed")
        ),
        pick(
            lang,
            "该任务在后台独立运行，本轮对话不会打断它。用户询问任务进展时，基于以上信息如实回答。不要再次发起同类生产任务，也不要声称没有任务在运行。生产类工具已临时不可用，任务结束后恢复。",
            "The task runs independently in the background; this chat turn does not interrupt it. When the user asks about its progress, answer truthfully from the information above. Do not start another production task of the same kind, and do not claim that no task is running. Production tools are temporarily unavailable and will be restored when the task finishes.",
        ),
    ];
    lines.join("\n")
}

// ── 任务执行卡（CollectedToolExec） ──────────────────────────────

/// TS `CollectedToolExec`（status 仅 running/completed/error 三态）。
#[derive(Debug, Clone)]
pub struct CollectedToolExec {
    pub id: String,
    pub tool: String,
    pub agent: Option<String>,
    pub label: String,
    pub status: &'static str,
    pub args: Option<Map<String, Value>>,
    pub result: Option<String>,
    pub details: Option<Value>,
    pub error: Option<String>,
    pub stages: Option<Vec<StudioTaskStage>>,
    pub logs: Option<Vec<String>>,
    pub started_at: f64,
    pub completed_at: Option<f64>,
}

impl CollectedToolExec {
    /// `manualToolAssistantMessage` 的 legacyDisplay / 响应 details 共用形态。
    pub fn to_json(&self) -> Value {
        let mut obj = json!({
            "id": self.id,
            "tool": self.tool,
            "label": self.label,
            "status": self.status,
            "startedAt": self.started_at,
        });
        let map = obj.as_object_mut().unwrap();
        if let Some(agent) = &self.agent {
            map.insert("agent".into(), json!(agent));
        }
        if let Some(args) = &self.args {
            map.insert("args".into(), Value::Object(args.clone()));
        }
        if let Some(result) = &self.result {
            map.insert("result".into(), json!(result));
        }
        if let Some(details) = &self.details {
            map.insert("details".into(), details.clone());
        }
        if let Some(error) = &self.error {
            map.insert("error".into(), json!(error));
        }
        if let Some(stages) = &self.stages {
            map.insert(
                "stages".into(),
                Value::Array(
                    stages
                        .iter()
                        .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
                        .collect(),
                ),
            );
        }
        if let Some(logs) = &self.logs {
            map.insert("logs".into(), json!(logs));
        }
        if let Some(completed_at) = self.completed_at {
            map.insert("completedAt".into(), json!(completed_at));
        }
        obj
    }

    fn to_execution(&self) -> StudioTaskExecution {
        StudioTaskExecution {
            id: self.id.clone(),
            tool: self.tool.clone(),
            agent: self.agent.clone(),
            label: self.label.clone(),
            status: match self.status {
                "completed" => StudioTaskExecutionStatus::Completed,
                "error" => StudioTaskExecutionStatus::Error,
                "processing" => StudioTaskExecutionStatus::Processing,
                _ => StudioTaskExecutionStatus::Running,
            },
            args: self.args.clone(),
            result: self.result.clone(),
            details: self.details.clone(),
            error: self.error.clone(),
            stages: self.stages.clone(),
            logs: self.logs.clone(),
            started_at: self.started_at,
            completed_at: self.completed_at,
        }
    }
}

/// `suppressManualTextForTool`：这些工具的结果自绘在前端，助手气泡留空。
fn suppress_manual_text_for_tool(tool: &str) -> bool {
    matches!(
        tool,
        "play_start"
            | "play_step"
            | "play_revise"
            | "script_create"
            | "storyboard_create"
            | "interactive_film_create"
    )
}

// ── 失败分类（classifyAgentFailure / formatAgentActionFailure） ──

fn agent_failure_busy_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"(?i)BookWriteLockError|locked by an active InkOS write|BOOK_BUSY")
            .unwrap()
    })
}

/// `formatAgentActionFailure`：busy → 409 BOOK_BUSY；其余一律 502
/// AGENT_ACTION_FAILED（消息原文，internal 类的改写文案不用于 action 面）。
pub fn format_agent_action_failure(message: &str) -> (StatusCode, &'static str, String) {
    if agent_failure_busy_re().is_match(message.trim()) {
        return (
            StatusCode::CONFLICT,
            "BOOK_BUSY",
            message.to_string(),
        );
    }
    (
        StatusCode::BAD_GATEWAY,
        "AGENT_ACTION_FAILED",
        message.to_string(),
    )
}

// ── 注册表（进程级内存；TS server 闭包内 Map 的等价物） ──────────

/// 确认任务的单任务名额（sessionId → taskId）。**必须在任何 await 之前同步
/// 预留**，并发的第二个确认请求直接 409（check-then-act 竞态防护）。
pub fn reserved_production_sessions() -> &'static Mutex<HashMap<String, String>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 运行中确认任务的 abort 句柄（taskId → flag）。
pub fn active_confirmed_tasks() -> &'static Mutex<HashMap<String, AbortHandle>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, AbortHandle>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `loadReconciledTaskSnapshot`：running 快照且本进程注册表无该任务 → 改写
/// error 终态落盘（旧进程遗留对账）。
pub async fn load_reconciled_task_snapshot(
    root: &Path,
    session_id: &str,
) -> Option<StudioTaskSnapshot> {
    let mut task = load_studio_task_snapshot(root, session_id).await?;
    let running = matches!(
        task.execution.status,
        StudioTaskExecutionStatus::Running | StudioTaskExecutionStatus::Processing
    );
    let locally_running = active_confirmed_tasks()
        .lock()
        .unwrap()
        .contains_key(&task.execution.id);
    if !running || locally_running {
        return Some(task);
    }
    let completed_at = utc_now_ms() as f64;
    task.updated_at = completed_at;
    task.execution.status = StudioTaskExecutionStatus::Error;
    task.execution.error = Some(
        "任务已中断：Studio 服务在任务运行期间重启，任务未能继续。请重新发起。".to_string(),
    );
    task.execution.completed_at = Some(completed_at);
    let _ = save_studio_task_snapshot(root, &task).await;
    Some(task)
}

/// `findRunningTaskController`（TS L2872）：内存优先（预留表 sessionId →
/// taskId → 句柄——controller 注册与首次快照落盘之间有多个 await 间隙，
/// 磁盘快照会漏掉刚启动的任务），磁盘快照只作回退。
pub async fn find_running_task_controller(
    root: &Path,
    session_id: &str,
) -> Option<AbortHandle> {
    let reserved_task_id = reserved_production_sessions()
        .lock()
        .unwrap()
        .get(session_id)
        .cloned();
    if let Some(task_id) = reserved_task_id {
        if let Some(handle) = active_confirmed_tasks().lock().unwrap().get(&task_id) {
            return Some(handle.clone());
        }
    }
    let task = load_reconciled_task_snapshot(root, session_id).await?;
    active_confirmed_tasks()
        .lock()
        .unwrap()
        .get(&task.execution.id)
        .cloned()
}

/// `findActiveRunningTask`：对账后 running 且本进程持有句柄 → Some。
pub async fn find_active_running_task(root: &Path, session_id: &str) -> Option<StudioTaskSnapshot> {
    let task = load_reconciled_task_snapshot(root, session_id).await?;
    let running = matches!(
        task.execution.status,
        StudioTaskExecutionStatus::Running | StudioTaskExecutionStatus::Processing
    );
    if running
        && active_confirmed_tasks()
            .lock()
            .unwrap()
            .contains_key(&task.execution.id)
    {
        Some(task)
    } else {
        None
    }
}

// ── payload 辅助 ────────────────────────────────────────────────

/// `deriveBookIdFromTitle`（58 号 book_create_routes 已有等价实现，复用）。
pub use crate::server::book_create_routes::derive_book_id_from_title_pub as derive_book_id_from_title;

// ── transcript 追加（生产任务轮） ────────────────────────────────

/// 任务开始前 user 消息先写（request_started → user → request_committed）。
async fn append_production_user_turn(
    root: &Path,
    session_id: &str,
    instruction: &str,
    session_kind: SessionKind,
) {
    let request_id = uuid::Uuid::new_v4().to_string();
    let now = utc_now_ms();
    append_transcript_events(root, session_id, |_events, next_seq| {
        vec![
            TranscriptEvent::RequestStarted {
                version: 1,
                session_id: session_id.to_string(),
                seq: next_seq,
                timestamp: now,
                request_id: request_id.clone(),
                session_kind: Some(session_kind),
                input: instruction.to_string(),
            },
            TranscriptEvent::Message {
                version: 1,
                session_id: session_id.to_string(),
                request_id: request_id.clone(),
                uuid: uuid::Uuid::new_v4().to_string(),
                parent_uuid: None,
                seq: next_seq + 1,
                timestamp: now,
                role: "user".into(),
                pi_turn_index: None,
                tool_call_id: None,
                source_tool_assistant_uuid: None,
                legacy_display: None,
                message: json!({ "role": "user", "content": instruction, "timestamp": now }),
            },
            TranscriptEvent::RequestCommitted {
                version: 1,
                session_id: session_id.to_string(),
                seq: next_seq + 2,
                timestamp: now,
                request_id,
            },
        ]
    })
    .await;
}

/// 任务收尾的助手工具消息（manualToolAssistantMessage：零 usage + toolUse +
/// legacyDisplay 工具卡）。已删除会话静默跳过（appendSessionMessagesUnlessDeleted）。
async fn append_production_assistant_message(
    root: &Path,
    session_id: &str,
    response_text: &str,
    exec: &CollectedToolExec,
    provider: &str,
    model: &str,
    session_kind: SessionKind,
) {
    if crate::server::session_routes::deleted_session_ids()
        .lock()
        .unwrap()
        .contains(session_id)
    {
        return;
    }
    let request_id = uuid::Uuid::new_v4().to_string();
    let now = utc_now_ms();
    let visible_text = if suppress_manual_text_for_tool(&exec.tool) {
        String::new()
    } else {
        response_text.to_string()
    };
    append_transcript_events(root, session_id, |_events, next_seq| {
        vec![
            TranscriptEvent::RequestStarted {
                version: 1,
                session_id: session_id.to_string(),
                seq: next_seq,
                timestamp: now,
                request_id: request_id.clone(),
                session_kind: Some(session_kind),
                input: String::new(),
            },
            TranscriptEvent::Message {
                version: 1,
                session_id: session_id.to_string(),
                request_id: request_id.clone(),
                uuid: uuid::Uuid::new_v4().to_string(),
                parent_uuid: None,
                seq: next_seq + 1,
                timestamp: now,
                role: "assistant".into(),
                pi_turn_index: None,
                tool_call_id: None,
                source_tool_assistant_uuid: None,
                legacy_display: Some(crate::interaction::session_transcript::LegacyDisplay {
                    thinking: None,
                    tool_executions: vec![exec.to_json()],
                }),
                message: json!({
                    "role": "assistant",
                    "content": [{ "type": "text", "text": visible_text }],
                    "api": "anthropic-messages",
                    "provider": provider,
                    "model": model,
                    "usage": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0,
                               "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0 } },
                    "stopReason": "toolUse",
                    "timestamp": now,
                }),
            },
            TranscriptEvent::RequestCommitted {
                version: 1,
                session_id: session_id.to_string(),
                seq: next_seq + 2,
                timestamp: now,
                request_id,
            },
        ]
    })
    .await;
}

// ── 执行器 ──────────────────────────────────────────────────────

/// 工具结果载荷（content 文本 + isError + details）。
pub(crate) struct ToolOutcome {
    pub(crate) is_error: bool,
    pub(crate) text: String,
    pub(crate) details: Value,
}

/// write_next 错误呈现（101 号：Aborted → 双语逐字（与轮间中止同一文案），
/// 其余 to_string）。
fn write_next_error_text(
    error: crate::pipeline::write_next::WriteNextError,
    lang: StudioLang,
) -> String {
    if matches!(error, crate::pipeline::write_next::WriteNextError::Aborted) {
        pick(
            lang,
            "操作已中止：用户请求停止该任务。",
            "Operation aborted: the user requested to stop this task.",
        )
    } else {
        error.to_string()
    }
}

/// write_next 执行器（createWriteNextChapterTool）：单章 / 多章连写。
/// 多章每轮之间轮询 abort；单章写作链内四个安全点检查（101 号接
/// WriteNextConfig.abort——章首/草稿后/审查环后/落盘前，TS
/// throwIfOperationAborted 检查点逐位对齐）。
async fn execute_write_next(
    runtime: &BooksRuntime,
    book_id: &str,
    chapter_count: u32,
    lang: StudioLang,
    abort: &AbortHandle,
    mut on_progress: impl FnMut(String),
) -> Result<ToolOutcome, String> {
    use crate::pipeline::write_next::{write_next_chapter, ChapterPipelineResult, WriteNextConfig};

    if !(1..=20).contains(&chapter_count) {
        return Err(format!(
            "chapterCount must be an integer between 1 and 20; received {chapter_count}."
        ));
    }

    let agents = crate::server::books_routes::build_write_next_agents(runtime);
    let ctx = crate::server::books_routes::build_write_next_ctx(runtime);
    let config = WriteNextConfig {
        abort: Some(abort.clone()),
        ..Default::default()
    };

    if chapter_count > 1 {
        on_progress(pick(
            lang,
            &format!("正在为 {book_id} 连续写 {chapter_count} 章…"),
            &format!("Writing {chapter_count} consecutive chapters for {book_id}..."),
        ));
        let mut results: Vec<ChapterPipelineResult> = Vec::new();
        for _ in 0..chapter_count {
            if *abort.lock().unwrap() {
                return Err(pick(
                    lang,
                    "操作已中止：用户请求停止该任务。",
                    "Operation aborted: the user requested to stop this task.",
                ));
            }
            let result = write_next_chapter(
                &runtime.state,
                &agents,
                &ctx,
                &config,
                book_id,
                None,
                None,
                None,
            )
            .await
            .map_err(|e| write_next_error_text(e, lang))?;
            on_progress(pick(
                lang,
                &format!(
                    "第 {}/{} 章已落盘：第 {} 章《{}》。",
                    results.len() + 1,
                    chapter_count,
                    result.chapter_number,
                    result.title
                ),
                &format!(
                    "{}/{} persisted: chapter {} \"{}\".",
                    results.len() + 1,
                    chapter_count,
                    result.chapter_number,
                    result.title
                ),
            ));
            let keep_going = result.status == "ready-for-review";
            results.push(result);
            if !keep_going {
                break;
            }
        }
        let last = results.last();
        let stopped_status = last
            .filter(|r| r.status != "ready-for-review")
            .map(|r| r.status);
        let response_text = match (stopped_status, last) {
            (Some(stopped), Some(last)) => pick(
                lang,
                &format!(
                    "已完成 {}/{} 章；第 {} 章状态为 {}，批量写作已停止，请复核后再继续。",
                    results.len(),
                    chapter_count,
                    last.chapter_number,
                    stopped
                ),
                &format!(
                    "Completed {}/{} chapters. Chapter {} ended with {}, so the batch stopped for review.",
                    results.len(),
                    chapter_count,
                    last.chapter_number,
                    stopped
                ),
            ),
            _ => pick(
                lang,
                &format!(
                    "已连续完成 {} 章（第 {} 章至第 {} 章）。",
                    results.len(),
                    results.first().map(|r| r.chapter_number).unwrap_or(0),
                    last.map(|r| r.chapter_number).unwrap_or(0)
                ),
                &format!(
                    "Completed {} consecutive chapters (chapters {}-{}).",
                    results.len(),
                    results.first().map(|r| r.chapter_number).unwrap_or(0),
                    last.map(|r| r.chapter_number).unwrap_or(0)
                ),
            ),
        };
        let mut details = json!({
            "kind": "chapters_written",
            "bookId": book_id,
            "requestedCount": chapter_count,
            "completedCount": results.len(),
            "chapters": results
                .iter()
                .map(|r| {
                    json!({
                        "chapterNumber": r.chapter_number,
                        "title": r.title,
                        "wordCount": r.word_count,
                        "status": r.status,
                    })
                })
                .collect::<Vec<_>>(),
        });
        if let Some(stopped) = stopped_status {
            details["stoppedStatus"] = json!(stopped);
        }
        Ok(ToolOutcome {
            is_error: stopped_status.is_some(),
            text: response_text,
            details,
        })
    } else {
        on_progress(pick(
            lang,
            &format!("正在为 {book_id} 写下一章…"),
            &format!("Writing the next chapter for {book_id}..."),
        ));
        let write_result = write_next_chapter(
            &runtime.state,
            &agents,
            &ctx,
            &config,
            book_id,
            None,
            None,
            None,
        )
        .await
        .map_err(|e| write_next_error_text(e, lang))?;
            let write_needs_review = write_result.status != "ready-for-review";
        let title_part = if write_result.title.is_empty() {
            String::new()
        } else {
            format!("《{}》", write_result.title)
        };
        let response_text = if write_needs_review {
            pick(
                lang,
                &format!(
                    "已为 {book_id} 写出第 {} 章{title_part}，字数 {}，但审稿未通过，状态 {}，需要复核后再继续。",
                    write_result.chapter_number, write_result.word_count, write_result.status
                ),
                &format!(
                    "Wrote chapter {} for {book_id}: {} words, but the review did not pass (status: {}). Manual review is required before continuing.",
                    write_result.chapter_number, write_result.word_count, write_result.status
                ),
            )
        } else {
            pick(
                lang,
                &format!(
                    "已为 {book_id} 完成第 {} 章{title_part}，字数 {}，状态 {}。",
                    write_result.chapter_number, write_result.word_count, write_result.status
                ),
                &format!(
                    "Completed chapter {} for {book_id}: {} words, status {}.",
                    write_result.chapter_number, write_result.word_count, write_result.status
                ),
            )
        };
        Ok(ToolOutcome {
            is_error: write_needs_review,
            text: response_text,
            details: json!({
                "kind": "chapter_written",
                "bookId": book_id,
                "chapterNumber": write_result.chapter_number,
                "title": write_result.title,
                "wordCount": write_result.word_count,
                "status": write_result.status,
            }),
        })
    }
}

/// create_book 执行器（createSubAgentTool 的 architect 建书分支）：
/// buildStudioBookConfig 派生 + init_book 同步执行。
pub(crate) async fn execute_create_book(
    runtime: &BooksRuntime,
    instruction: &str,
    title: &str,
    action_payload: Option<&Value>,
    mut on_progress: impl FnMut(String),
) -> Result<ToolOutcome, String> {
    let payload = action_payload.and_then(|p| p.get("createBook"));
    let field = |name: &str| {
        payload
            .and_then(|p| p.get(name))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    let genre = field("genre").unwrap_or("general");
    let platform = field("platform");
    let language = field("language");
    let target_chapters = payload
        .and_then(|p| p.get("targetChapters"))
        .and_then(Value::as_f64)
        .map(|v| v as u32);
    let chapter_word_count = payload
        .and_then(|p| p.get("chapterWordCount"))
        .and_then(Value::as_f64)
        .map(|v| v as u32);

    // id 派生：确认卡 title 优先；派生失败回退 book-{毫秒 base36}（TS 语义）。
    let now = crate::utils::utc_time::utc_now_iso();
    let mut book = crate::server::book_create_routes::build_studio_book_config_pub(
        title, genre, language, platform, target_chapters, chapter_word_count, &now,
    );
    if book.id.is_empty() {
        book.id = format!("book-{}", to_base36(utc_now_ms()));
    }

    on_progress(format!("Starting architect for book \"{}\"...", book.id));
    crate::server::book_create_routes::init_book(runtime, &book, Some(instruction), None, None)
        .await?;
    on_progress(format!(
        "Architect finished — book \"{}\" foundation created.",
        book.id
    ));
    Ok(ToolOutcome {
        is_error: false,
        text: format!(
            "Book \"{title}\" ({}) initialised successfully. Foundation files are ready.",
            book.id
        ),
        details: json!({ "kind": "book_created", "bookId": book.id, "title": title }),
    })
}

/// `createShortFictionRunTool`（78 号）：runShortFictionProduction 同链。
/// 结果文本逐字（含封面未生成的三行降级说明）；details = kind + result 全字段。
#[allow(clippy::too_many_arguments)]
async fn execute_short_run(
    runtime: &BooksRuntime,
    direction: &str,
    reference: Option<&str>,
    story_id: Option<&str>,
    chapter_count: Option<u32>,
    chars_per_chapter: Option<u32>,
    language: crate::utils::language::WritingLanguage,
    cover: bool,
    mut on_progress: impl FnMut(String) + Send,
) -> Result<ToolOutcome, String> {
    use crate::agents::short_fiction::ShortFictionReference;
    use crate::pipeline::short_fiction_runner::{run_short_fiction_production, ShortFictionRunOptions};
    let root = runtime.state.project_root();
    let reference_struct = reference.map(|text| ShortFictionReference { text: text.to_string() });
    let result = run_short_fiction_production(ShortFictionRunOptions {
        project_root: root,
        router: &runtime.router,
        direction,
        reference: reference_struct.as_ref(),
        story_id,
        out_dir: None,
        chapter_count,
        chars_per_chapter,
        language,
        cover,
        on_progress: &mut on_progress,
    })
    .await?;
    let cover_lines = match &result.cover_image_path {
        Some(path) => format!("Cover image: {path}"),
        None => [
            "Cover image: not generated.".to_string(),
            format!(
                "Cover image reason: {}",
                summarize_cover_generation_error(result.cover_error.as_deref())
            ),
            "The short fiction draft, synopsis, selling points, and cover prompt were still written successfully.".to_string(),
        ]
        .join("\n"),
    };
    let text = format!(
        "Short fiction \"{}\" completed.\nFinal: {}\nSales package: {}\nCover prompt: {}\n{cover_lines}",
        result.story_id,
        result.final_markdown_path,
        result.sales_package_path,
        result.cover_prompt_path
    );
    let mut details = serde_json::to_value(&result).unwrap_or(Value::Null);
    if let Some(map) = details.as_object_mut() {
        map.insert("kind".to_string(), json!("short_fiction_created"));
    }
    Ok(ToolOutcome { is_error: false, text, details })
}

/// `assertShortRunCharsPerChapter`：最终语言（payload ?? 会话）下的范围断言——
/// 确认卡层只能做 600-1200 并集校验，越界组合在这里以双语错误拦截。
fn assert_short_run_chars_per_chapter(
    value: Option<u32>,
    language: crate::utils::language::WritingLanguage,
) -> Result<(), String> {
    use crate::utils::language::WritingLanguage;
    let Some(value) = value else {
        return Ok(());
    };
    let (min, max) = match language {
        WritingLanguage::En => (600, 800),
        WritingLanguage::Zh => (900, 1200),
    };
    if (min..=max).contains(&value) {
        return Ok(());
    }
    Err(match language {
        WritingLanguage::En => format!(
            "charsPerChapter={value} 超出英文短篇的合法范围（每章 {min}-{max} 个英文单词）。charsPerChapter={value} is outside the valid range for English shorts ({min}-{max} words per chapter)."
        ),
        WritingLanguage::Zh => format!(
            "charsPerChapter={value} 超出中文短篇的合法范围（每章 {min}-{max} 个汉字）。charsPerChapter={value} is outside the valid range for Chinese shorts ({min}-{max} characters per chapter)."
        ),
    })
}

/// `summarizeCoverGenerationError`：503/502/api-key 关键词分支 + 300 截断。
fn summarize_cover_generation_error(error: Option<&str>) -> String {
    let text = error.unwrap_or("not generated").trim();
    if text.contains("HTTP 503") {
        return "cover provider returned HTTP 503; retry later or switch the Studio cover provider/model.".to_string();
    }
    if text.contains("HTTP 502") {
        return "cover provider returned HTTP 502; retry later or switch the Studio cover provider/model.".to_string();
    }
    if text.to_lowercase().contains("api key") {
        return "cover API key is missing; configure it in Studio service settings.".to_string();
    }
    text.chars().take(300).collect()
}

/// `createScriptCreationTool`：runScriptCreation 同链（76 号 runner）。
#[allow(clippy::too_many_arguments)]
async fn execute_script_create(
    runtime: &BooksRuntime,
    title: &str,
    instruction: &str,
    source_kind: &str,
    target_format: &str,
    source_text: &str,
    source_path: &str,
    requirements: &str,
    episode_count: Option<u32>,
    episode_duration: &str,
    project_id: &str,
    out_dir: &str,
    mut on_progress: impl FnMut(String) + Send,
) -> Result<ToolOutcome, String> {
    use crate::pipeline::script_storyboard_runner::{run_script_creation, ScriptCreationRunOptions};
    let root = runtime.state.project_root();
    fn optional(value: &str) -> Option<&str> {
        if value.is_empty() { None } else { Some(value) }
    }
    let result = run_script_creation(ScriptCreationRunOptions {
        project_root: root,
        router: &runtime.router,
        title,
        instruction,
        source_kind: optional(source_kind),
        target_format: optional(target_format),
        source_text: optional(source_text),
        source_path: optional(source_path),
        requirements: optional(requirements),
        episode_count,
        episode_duration: optional(episode_duration),
        language: None,
        project_id: optional(project_id),
        out_dir: optional(out_dir),
        on_progress: &mut on_progress,
    })
    .await?;
    let text = format!(
        "Script project \"{title}\" created.\nID: {}\nSpec: {}\nScript: {}",
        result.project_id, result.spec_path, result.script_path
    );
    on_progress(format!("Script drafted: {}", result.script_path));
    Ok(ToolOutcome {
        is_error: false,
        text,
        details: json!({
            "kind": "script_project_created",
            "projectId": result.project_id,
            "baseDir": result.base_dir,
            "specPath": result.spec_path,
            "scriptPath": result.script_path,
        }),
    })
}

/// `createStoryboardCreationTool`：runStoryboardCreation 同链。
#[allow(clippy::too_many_arguments)]
async fn execute_storyboard_create(
    runtime: &BooksRuntime,
    title: &str,
    instruction: &str,
    source_kind: &str,
    source_text: &str,
    source_path: &str,
    requirements: &str,
    visual_style: &str,
    aspect_ratio: &str,
    granularity: &str,
    max_shots: Option<u32>,
    project_id: &str,
    out_dir: &str,
    mut on_progress: impl FnMut(String) + Send,
) -> Result<ToolOutcome, String> {
    use crate::pipeline::script_storyboard_runner::{
        run_storyboard_creation, StoryboardCreationRunOptions,
    };
    let root = runtime.state.project_root();
    fn optional(value: &str) -> Option<&str> {
        if value.is_empty() { None } else { Some(value) }
    }
    let result = run_storyboard_creation(StoryboardCreationRunOptions {
        project_root: root,
        router: &runtime.router,
        title,
        instruction,
        source_kind: optional(source_kind),
        source_text: optional(source_text),
        source_path: optional(source_path),
        requirements: optional(requirements),
        visual_style: optional(visual_style),
        aspect_ratio: optional(aspect_ratio),
        granularity: optional(granularity),
        max_shots,
        language: None,
        project_id: optional(project_id),
        out_dir: optional(out_dir),
        on_progress: &mut on_progress,
    })
    .await?;
    let text = format!(
        "Storyboard project \"{title}\" created.\nID: {}\nStoryboard: {}\nImage prompts: {}\nAssets manifest: {}",
        result.project_id, result.storyboard_path, result.image_prompts_path, result.assets_manifest_path
    );
    Ok(ToolOutcome {
        is_error: false,
        text,
        details: json!({
            "kind": "storyboard_project_created",
            "projectId": result.project_id,
            "baseDir": result.base_dir,
            "specPath": result.spec_path,
            "storyboardPath": result.storyboard_path,
            "imagePromptsPath": result.image_prompts_path,
            "assetsManifestPath": result.assets_manifest_path,
            "assetsDir": result.assets_dir,
        }),
    })
}

/// `createInteractiveFilmCreationTool`：五节交付稿 + story graph 全链（77 号）。
#[allow(clippy::too_many_arguments)]
async fn execute_interactive_film_create(
    runtime: &BooksRuntime,
    title: &str,
    instruction: &str,
    source_kind: &str,
    source_text: &str,
    source_path: &str,
    requirements: &str,
    target_audience: &str,
    episode_count: Option<u32>,
    episode_duration: &str,
    budget: &str,
    reference_mode: &str,
    project_id: &str,
    out_dir: &str,
    mut on_progress: impl FnMut(String) + Send,
) -> Result<ToolOutcome, String> {
    use crate::pipeline::script_storyboard_runner::{
        run_interactive_film_creation, InteractiveFilmCreationRunOptions,
    };
    let root = runtime.state.project_root();
    fn optional(value: &str) -> Option<&str> {
        if value.is_empty() { None } else { Some(value) }
    }
    let result = run_interactive_film_creation(InteractiveFilmCreationRunOptions {
        project_root: root,
        router: &runtime.router,
        title,
        instruction,
        source_kind: optional(source_kind),
        source_text: optional(source_text),
        source_path: optional(source_path),
        requirements: optional(requirements),
        target_audience: optional(target_audience),
        episode_count,
        episode_duration: optional(episode_duration),
        budget: optional(budget),
        reference_mode: optional(reference_mode),
        language: None,
        project_id: optional(project_id),
        out_dir: optional(out_dir),
        on_progress: &mut on_progress,
    })
    .await?;
    let text = format!(
        "Interactive film \"{}\" completed.\nSpec: {}\nStory graph: {}\nStory tree: {}\nFlags: {}\nScript: {}\nStoryboard: {}\nImage prompts: {}\nImage assets: {}",
        result.project_id,
        result.spec_path,
        result.story_graph_path,
        result.story_tree_path,
        result.flags_path,
        result.script_path,
        result.storyboard_path,
        result.image_prompts_path,
        result.assets_manifest_path
    );
    Ok(ToolOutcome {
        is_error: false,
        text,
        details: json!({
            "kind": "interactive_film_created",
            "projectId": result.project_id,
            "baseDir": result.base_dir,
            "storyGraphPath": result.story_graph_path,
            "specPath": result.spec_path,
            "storyTreePath": result.story_tree_path,
            "flagsPath": result.flags_path,
            "scriptPath": result.script_path,
            "storyboardPath": result.storyboard_path,
            "imagePromptsPath": result.image_prompts_path,
            "assetsManifestPath": result.assets_manifest_path,
            "assetsDir": result.assets_dir,
        }),
    })
}

/// `createGenerateCoverTool`：generateShortFictionCover 同链（74 号 cover 基础
/// 设施 + 76 号 generic/short 双模式提示词）。89 号提 pub(crate)：聊天壳复用。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_generate_cover(
    runtime: &BooksRuntime,
    title: &str,
    intro: &str,
    selling_points: &[String],
    cover_prompt: &str,
    output_dir: &str,
    mut on_progress: impl FnMut(String),
) -> Result<ToolOutcome, String> {
    use crate::llm::cover::generate_short_fiction_cover;
    on_progress("Generating cover image...".to_string());
    let root = runtime.state.project_root();
    fn optional(value: &str) -> Option<&str> {
        if value.is_empty() { None } else { Some(value) }
    }
    let result = generate_short_fiction_cover(
        root,
        title,
        optional(intro),
        selling_points,
        optional(cover_prompt),
        optional(output_dir),
    )
    .await?;
    let text = format!(
        "Cover generated for \"{}\".\nCover prompt: {}\nCover image: {}",
        result.title, result.cover_prompt_path, result.cover_image_path
    );
    Ok(ToolOutcome {
        is_error: false,
        text,
        details: json!({
            "kind": "cover_generated",
            "title": result.title,
            "outputDir": result.output_dir,
            "coverPromptPath": result.cover_prompt_path,
            "coverImagePath": result.cover_image_path,
        }),
    })
}

/// `createTranslationCreateTool`：createTranslationProjectFromFile 同链（70 号
/// 域本体）——只摄取分段，翻译执行是独立长任务（run 端点）。
async fn execute_translation_create(
    runtime: &BooksRuntime,
    file_path: &str,
    source_language: &str,
    target_language: &str,
    title: &str,
    segment_max_chars: Option<usize>,
    mut on_progress: impl FnMut(String),
) -> Result<ToolOutcome, String> {
    use crate::translation::types::CreateTranslationProjectInput;
    on_progress(format!("Creating translation project from {file_path}..."));
    let root = runtime.state.project_root();
    let input = CreateTranslationProjectInput {
        file_path,
        source_language,
        target_language,
        title: if title.is_empty() { None } else { Some(title) },
        segment_max_chars,
    };
    let result = crate::translation::project::create_translation_project_from_file(root, &input)
        .await?;
    let manifest = &result.manifest;
    let text = [
        format!("Translation project \"{}\" created.", manifest.title),
        format!("ID: {}", manifest.id),
        format!(
            "Source: {} {} -> {}",
            manifest.source.kind.as_str(),
            manifest.source_language,
            manifest.target_language
        ),
        format!("Chapters: {}", manifest.chapters.len()),
        format!("Manifest: {}", result.manifest_path),
    ]
    .join("\n");
    Ok(ToolOutcome {
        is_error: false,
        text,
        details: json!({
            "kind": "translation_project_created",
            "projectDir": result.project_dir,
            "manifestPath": result.manifest_path,
            "manifest": serde_json::to_value(manifest).unwrap_or(Value::Null),
        }),
    })
}

/// `createConnectChoiceTool`：StoryNode 解析 → upsert delta → applyGraphDelta。
async fn execute_connect_choice(
    runtime: &BooksRuntime,
    project_id: &str,
    node_value: &Value,
    mut on_progress: impl FnMut(String),
) -> Result<ToolOutcome, String> {
    use crate::interactive_film as film;
    on_progress(format!("Connecting choices on {project_id}..."));
    let root = runtime.state.project_root();
    // StoryNodeSchema.parse 等价：serde 严格化（非法字段/缺 id 400 抛错 → Err）。
    let node: film::StoryNode = serde_json::from_value(node_value.clone())
        .map_err(|e| format!("invalid connect-choice node: {e}"))?;
    let delta = film::StoryGraphDelta {
        world_anchor: None,
        characters: None,
        nodes: Some(film::UpsertRemove {
            upsert: vec![node.clone()],
            remove: Vec::new(),
        }),
        variables: None,
        endings: None,
        notes: Vec::new(),
    };
    let (_, rev) = crate::server::interactive_film_routes::apply_graph_delta(root, project_id, &delta).await?;
    Ok(ToolOutcome {
        is_error: false,
        text: format!("Choices updated on node {} (rev {rev}).", node.id),
        details: json!({ "kind": "graph_updated", "rev": rev }),
    })
}

/// `createRemoveNodeTool`：nodeId → remove delta → applyGraphDelta。
async fn execute_remove_node(
    runtime: &BooksRuntime,
    project_id: &str,
    node_id: &str,
    mut on_progress: impl FnMut(String),
) -> Result<ToolOutcome, String> {
    use crate::interactive_film as film;
    on_progress(format!("Removing node {node_id}..."));
    let root = runtime.state.project_root();
    let delta = film::StoryGraphDelta {
        world_anchor: None,
        characters: None,
        nodes: Some(film::UpsertRemove {
            upsert: Vec::new(),
            remove: vec![node_id.to_string()],
        }),
        variables: None,
        endings: None,
        notes: Vec::new(),
    };
    let (_, rev) = crate::server::interactive_film_routes::apply_graph_delta(root, project_id, &delta).await?;
    Ok(ToolOutcome {
        is_error: false,
        text: format!("Node {node_id} removed (rev {rev})."),
        details: json!({ "kind": "graph_updated", "rev": rev }),
    })
}

/// `createDraftStructureTool`：图上下文 → 编剧提示词（zh 逐字）→ LLM →
/// {nodes:[...]} 解析 → upsert delta（phase structure）。
async fn execute_draft_structure(
    runtime: &BooksRuntime,
    project_id: &str,
    instruction: &str,
    mut on_progress: impl FnMut(String),
) -> Result<ToolOutcome, String> {
    use crate::interactive_film as film;
    on_progress(format!("Drafting structure for {project_id}..."));
    let root = runtime.state.project_root();
    let context = match film::load_story_graph(root, project_id).await {
        Ok(Some(graph)) => summarize_story_graph_for_authoring(&graph),
        _ => "(empty graph)".to_string(),
    };
    let system_prompt = "你是互动影游编剧。根据上下文与指令，生成分支骨架 JSON：{ \"nodes\": [StoryNode...] }。恰好 1 个 type=start，至少 2 个 branch，至少 2 个差异化 ending 节点；每条路径都能到某个 ending；只输出 JSON。";
    let user_prompt = format!("{context}\n\n骨架指令：{instruction}");
    let outcome = runtime
        .router
        .chat(
            "film-authoring",
            vec![
                crate::llm::provider::LLMMessage {
                    role: crate::llm::provider::LLMRole::System,
                    content: system_prompt.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                },
                crate::llm::provider::LLMMessage {
                    role: crate::llm::provider::LLMRole::User,
                    content: user_prompt,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            0.7,
            None,
        )
        .await
        .map_err(|e| e.to_string())?;
    // extractJson：fence 剥离 + 首 { 到末 }。
    let parsed = extract_json_object(&outcome.content)
        .ok_or_else(|| "draft_structure: LLM returned no JSON".to_string())?;
    let raw_nodes = parsed.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
    if raw_nodes.is_empty() {
        return Err("draft_structure: LLM returned no nodes".to_string());
    }
    let mut nodes = Vec::new();
    for raw in raw_nodes {
        nodes.push(
            serde_json::from_value::<film::StoryNode>(raw)
                .map_err(|e| format!("draft_structure: invalid node: {e}"))?,
        );
    }
    let node_count = nodes.len();
    let delta = film::StoryGraphDelta {
        world_anchor: None,
        characters: None,
        nodes: Some(film::UpsertRemove {
            upsert: nodes,
            remove: Vec::new(),
        }),
        variables: None,
        endings: None,
        notes: Vec::new(),
    };
    let (_, rev) = crate::server::interactive_film_routes::apply_graph_delta(root, project_id, &delta).await?;
    Ok(ToolOutcome {
        is_error: false,
        text: format!("Structure drafted: {node_count} nodes (rev {rev})."),
        details: json!({ "kind": "graph_updated", "rev": rev }),
    })
}

/// fence 剥离 + 首 `{` 到末 `}` 子串提取（extractJson 等价）。
fn extract_json_object(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end > start {
        serde_json::from_str(&trimmed[start..=end]).ok()
    } else {
        None
    }
}

/// `buildFilmAuthoringContext`（zh）：图谱摘要 + 角色档案。
fn summarize_story_graph_for_authoring(graph: &crate::interactive_film::StoryGraph) -> String {
    use crate::interactive_film as film;
    let mut lines = vec![format!(
        "# 互动影游：{}",
        if graph.title.is_empty() { &graph.project_id } else { &graph.title }
    )];
    if let Some(anchor) = &graph.world_anchor {
        lines.push(format!(
            "核心：{} / 主题：{} / 题材：{} / 规则：{} / 时长：{}分",
            anchor.story_core, anchor.theme, anchor.genre, anchor.world_rules, anchor.duration_minutes as i64
        ));
    }
    if !graph.variables.is_empty() {
        let names: Vec<&str> = graph.variables.iter().map(|v| v.name.as_str()).collect();
        lines.push(format!("变量：{}", names.join(", ")));
    }
    lines.push("节点：".to_string());
    for node in &graph.nodes {
        let edges: Vec<String> = node
            .choices
            .iter()
            .map(|choice| format!("{}→{}", choice.text, choice.target_node_id))
            .collect();
        lines.push(format!(
            "- {}[{}] {}{}",
            node.id,
            film::node_type_str(node.node_type),
            node.title,
            if edges.is_empty() { String::new() } else { format!(" -> {}", edges.join(", ")) }
        ));
    }
    let mut blocks = vec![lines.join("\n")];
    if !graph.characters.is_empty() {
        let mut char_lines = vec!["角色档案：".to_string()];
        for character in &graph.characters {
            let voice = character
                .voice_profile
                .as_ref()
                .map(|profile| {
                    [profile.speaking_rhythm.clone(), profile.vocabulary.clone()]
                        .iter()
                        .filter(|part| !part.is_empty())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" / ")
                })
                .filter(|voice| !voice.is_empty());
            char_lines.push(format!(
                "- {}（{}）动机：{}{}",
                character.name,
                film::character_role_str(character.role),
                character.motivation,
                voice.map(|voice| format!(" 口吻：{voice}")).unwrap_or_default()
            ));
        }
        blocks.push(char_lines.join("\n"));
    }
    blocks.join("\n\n")
}

/// `createPlayStartTool` 语义：worldId 即会话 id（1:1 绑定，两个 play 会话互
/// 不串台）+ runId 固定 "main"；首开（transcript 空）写 scene/状态/助手轮；
/// seedOpening 失败 fail-open（HUD 增强而非启动前提）。
#[allow(clippy::too_many_arguments)]
async fn execute_play_start(
    runtime: &BooksRuntime,
    session_id: &str,
    play_mode: Option<&str>,
    title: &str,
    premise: &str,
    world_contract: &str,
    visual_contract: &str,
    mode: &str,
    initial_scene: &str,
    suggested_actions: Vec<String>,
    mut on_progress: impl FnMut(String),
) -> Result<ToolOutcome, String> {
    use crate::play::PlayWorldInput;

    let root = runtime.state.project_root();
    // safePlayId：trim + 80 上限 + 危险字符拒绝（回退会话 id 本身）。
    let world_id: String = session_id.trim().chars().take(80).collect();
    if world_id.is_empty()
        || world_id == "."
        || world_id == ".."
        || world_id.contains('/')
        || world_id.contains('\\')
        || world_id.contains('\0')
    {
        return Err(format!("Invalid play id: {session_id:?}"));
    }
    let run_id = "main";

    on_progress("Starting interactive world...".to_string());
    let mode = if mode.is_empty() {
        play_mode.unwrap_or("open")
    } else {
        mode
    };
    // TS 逐字：inferLanguage([title, premise, worldContract, visualContract,
    // initialScene].filter(Boolean).join("\n"))——空段剔除后按内容判 zh/en。
    let language_source = [title, premise, world_contract, visual_contract, initial_scene]
        .iter()
        .filter(|segment| !segment.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let language = if crate::utils::infer_language(Some(&language_source)) == crate::utils::WritingLanguage::En {
        "en"
    } else {
        "zh"
    };
    let world = crate::play::create_world(
        root,
        &PlayWorldInput {
            id: &world_id,
            title,
            premise,
            world_contract,
            visual_contract,
            mode,
            language,
        },
    )
    .await?;
    crate::play::ensure_run(root, &world_id, run_id).await?;

    let existing_transcript = crate::play::read_transcript(root, &world_id, run_id).await;
    // 缺省开场正文按世界语言分流（TS world.language === "en" 分支逐字）。
    let default_scene = if language == "en" {
        let premise_line = if premise.is_empty() {
            "The scene is set. Make your first move."
        } else {
            premise
        };
        format!("You enter \"{title}\".\n{premise_line}")
    } else if premise.is_empty() {
        format!("你进入「{title}」。\n场景已经就位，等待你的第一个动作。")
    } else {
        format!("你进入「{title}」。\n{premise}")
    };
    let scene_text = if initial_scene.trim().is_empty() {
        default_scene
    } else {
        initial_scene.trim().to_string()
    };
    if existing_transcript.is_empty() {
        crate::play::write_projection(root, &world_id, run_id, "projections/scene.md", &format!("{scene_text}\n")).await?;
        crate::play::save_current_state(
            root,
            &world_id,
            run_id,
            &json!({
                "turn": 0,
                "worldId": world_id,
                "runId": run_id,
                "mode": mode,
                "premise": premise,
                "worldContract": world_contract,
                "visualContract": visual_contract,
            }),
        )
        .await?;
        crate::play::append_transcript_turn(root, &world_id, run_id, "assistant", &scene_text).await?;
    }

    // 开场图谱播种（fail-open：模型漂移不阻断启动）。
    let mut graph = None;
    let mut seed_mutation = None;
    if existing_transcript.is_empty() {
        let agents = crate::play_runner::PlayAgents { router: &runtime.router };
        let runner = crate::play_runner::PlayRunner {
            project_root: root,
            world_id: world_id.clone(),
            run_id: run_id.to_string(),
        };
        let result = runner
            .seed_opening(&agents, &scene_text, &suggested_actions)
            .await;
        if let Ok(Some(mutation)) = result {
            let run_dir = crate::play::run_dir(root, &world_id, run_id)?;
            graph = Some(crate::play::play_graph_snapshot(&run_dir));
            seed_mutation = Some(mutation);
        }
    }

    let mut details = json!({
        "kind": "play_world_started",
        "worldId": world_id,
        "runId": run_id,
        "title": world.get("title"),
        "mode": world.get("mode"),
        "premise": world.get("premise"),
        "worldContract": world.get("worldContract"),
        "visualContract": world.get("visualContract"),
        "sceneText": scene_text,
        "suggestedActions": suggested_actions,
    });
    if let Some(mutation) = seed_mutation {
        details["seedMutation"] = mutation;
    }
    if let Some(graph) = graph {
        details["graph"] = graph;
    }
    Ok(ToolOutcome {
        is_error: false,
        text: scene_text,
        details,
    })
}

fn to_base36(mut value: u64) -> String {
    if value == 0 {
        return "0".to_string();
    }
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

// ── 主流程 ──────────────────────────────────────────────────────

/// 生产分支参数包（post_agent 装配后传入）。
pub struct ProductionRequest<'a> {
    pub instruction: &'a str,
    pub session_id: &'a str,
    pub book_id: Option<&'a str>,
    pub session_kind: SessionKind,
    pub play_mode: Option<&'a str>,
    pub intent: RequestedIntent,
    pub action_payload: Option<&'a Value>,
    pub language: StudioLang,
    pub source_request_id: Option<&'a str>,
    /// manualToolAssistantMessage 的 provider/model 字段（请求级 service/model
    /// 回退项目配置；67 号取项目 LLM 配置标签）。
    pub provider_label: String,
    pub model_label: String,
}

/// 分支成功结果（200 响应材料）。
pub struct ProductionOutcome {
    pub response_text: String,
    pub exec: CollectedToolExec,
    /// 建书迁移后的活动书（无迁移 = 请求时 book_id）。
    pub active_book_id: Option<String>,
}

/// 分支失败（agent:error 广播与 502/409 响应材料；exec 供 agent:error 附带）。
pub struct ProductionError {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
    pub exec: Option<CollectedToolExec>,
}

/// 确认式生产任务分支主链：预留 → 快照检查 → 建书预备广播 → user 先写 →
/// 执行（快照 + tool:start/end）→ 建书迁移 → 收尾助手消息。任务终态后必然
/// 释放预留与 abort 注册（对应 TS finally）。
pub async fn run_confirmed_production(
    runtime: &BooksRuntime,
    request: ProductionRequest<'_>,
) -> Result<ProductionOutcome, ProductionError> {
    let task_id = format!(
        "direct-{}-{}",
        request.intent.as_str(),
        uuid::Uuid::new_v4()
    );

    // ── 单任务闸门：任何 await 之前同步预留 ──
    {
        let mut reserved = reserved_production_sessions().lock().unwrap();
        if reserved.contains_key(request.session_id) {
            return Err(busy_error(request.language));
        }
        reserved.insert(request.session_id.to_string(), task_id.clone());
    }
    let reserved_session_id = request.session_id.to_string();

    let abort: AbortHandle = Arc::new(Mutex::new(false));
    active_confirmed_tasks()
        .lock()
        .unwrap()
        .insert(task_id.clone(), abort.clone());

    let result = run_confirmed_production_locked(runtime, &request, &task_id, &abort).await;

    active_confirmed_tasks().lock().unwrap().remove(&task_id);
    reserved_production_sessions()
        .lock()
        .unwrap()
        .remove(&reserved_session_id);
    result
}

fn busy_error(lang: StudioLang) -> ProductionError {
    let message = pick(
        lang,
        "当前会话已有一个生产任务在运行，请等它完成，或先用停止按钮结束它，再发起新任务。",
        "A production task is already running in this session. Wait for it to finish, or stop it first, then start a new task.",
    );
    ProductionError {
        status: StatusCode::CONFLICT,
        code: "PRODUCTION_TASK_ALREADY_RUNNING".to_string(),
        message,
        exec: None,
    }
}

#[allow(clippy::too_many_lines)]
async fn run_confirmed_production_locked(
    runtime: &BooksRuntime,
    request: &ProductionRequest<'_>,
    task_id: &str,
    abort: &AbortHandle,
) -> Result<ProductionOutcome, ProductionError> {
    let root = runtime.state.project_root();
    let lang = request.language;
    let session_id = request.session_id;

    // 预留成功后再走快照检查（防旧进程遗留的运行中快照被覆盖）。
    if find_active_running_task(root, session_id).await.is_some() {
        return Err(busy_error(lang));
    }

    // 建书预备：book:creating 广播 + create-status 内存态（与 /books/create 共享）。
    let mut pending_book_id: Option<String> = None;
    if request.intent == RequestedIntent::CreateBook {
        let title = request
            .action_payload
            .and_then(|p| p.get("createBook"))
            .and_then(|p| p.get("title"))
            .and_then(Value::as_str);
        if let Some(title) = title {
            let derived = derive_book_id_from_title(title);
            if !derived.is_empty() {
                pending_book_id = Some(derived);
            }
        }
    }
    if let Some(derived) = &pending_book_id {
        crate::server::book_create_routes::set_create_status(
            derived,
            crate::server::book_create_routes::BookCreateStatus {
                status: "creating".to_string(),
                error: None,
            },
        )
        .await;
        runtime
            .hub
            .broadcast("book:creating", &json!({ "bookId": derived, "sessionId": session_id }));
    }

    // 任务开始前先把用户指令写进 transcript（刷新恢复用户气泡）。
    append_production_user_turn(root, session_id, request.instruction, request.session_kind).await;

    // ── 执行卡装配（intent → tool/agent/params/stages） ──
    let mut params = Map::new();
    let agent: Option<&str>;
    match request.intent {
        RequestedIntent::ShortRun => {
            let payload = request.action_payload.and_then(|p| p.get("shortRun"));
            // direction 兜底链：payload.direction ?? instruction.trim()（都空 → 502）。
            let direction = payload
                .and_then(|p| p.get("direction"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| {
                    let trimmed = request.instruction.trim();
                    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
                })
                .ok_or_else(|| {
                    production_exec_error(
                        lang,
                        pick(lang, "确认短篇缺少方向，请重新生成确认卡。", "The short fiction confirmation is missing a direction. Regenerate the confirmation card."),
                    )
                })?;
            params.insert("direction".into(), json!(direction));
            for name in ["reference", "storyId"] {
                if let Some(value) = payload
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    params.insert(name.into(), json!(value));
                }
            }
            for name in ["chapters", "charsPerChapter"] {
                if let Some(value) = payload
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_u64)
                    .filter(|v| *v > 0)
                {
                    params.insert(name.into(), json!(value));
                }
            }
            // cover：显式 false 也展开（TS `cover !== undefined` 判定）。
            if let Some(cover) = payload.and_then(|p| p.get("cover")).and_then(Value::as_bool) {
                params.insert("cover".into(), json!(cover));
            }
            agent = None;
        }
        RequestedIntent::ScriptCreate => {
            let payload = request.action_payload.and_then(|p| p.get("scriptCreate"));
            let field = |name: &str| {
                payload
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            };
            let title = field("title").ok_or_else(|| {
                production_exec_error(
                    lang,
                    pick(lang, "确认创建剧本缺少标题，请重新生成确认卡。", "The script creation confirmation is missing a title. Regenerate the confirmation card."),
                )
            })?.to_string();
            params.insert("title".into(), json!(title));
            params.insert("instruction".into(), json!(request.instruction));
            for name in ["sourceKind", "targetFormat", "sourceText", "sourcePath", "requirements", "episodeDuration", "projectId", "outDir"] {
                if let Some(value) = field(name) {
                    params.insert(name.into(), json!(value));
                }
            }
            if let Some(count) = payload.and_then(|p| p.get("episodeCount")).and_then(Value::as_u64) {
                params.insert("episodeCount".into(), json!(count));
            }
            agent = None;
        }
        RequestedIntent::StoryboardCreate => {
            let payload = request.action_payload.and_then(|p| p.get("storyboardCreate"));
            let field = |name: &str| {
                payload
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            };
            let title = field("title").ok_or_else(|| {
                production_exec_error(
                    lang,
                    pick(lang, "确认创建分镜缺少标题，请重新生成确认卡。", "The storyboard creation confirmation is missing a title. Regenerate the confirmation card."),
                )
            })?.to_string();
            params.insert("title".into(), json!(title));
            params.insert("instruction".into(), json!(request.instruction));
            for name in ["sourceKind", "sourceText", "sourcePath", "requirements", "visualStyle", "aspectRatio", "granularity", "projectId", "outDir"] {
                if let Some(value) = field(name) {
                    params.insert(name.into(), json!(value));
                }
            }
            if let Some(shots) = payload.and_then(|p| p.get("maxShots")).and_then(Value::as_u64) {
                params.insert("maxShots".into(), json!(shots));
            }
            agent = None;
        }
        RequestedIntent::InteractiveFilmCreate => {
            let payload = request.action_payload.and_then(|p| p.get("interactiveFilmCreate"));
            let field = |name: &str| {
                payload
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            };
            let title = field("title").ok_or_else(|| {
                production_exec_error(
                    lang,
                    pick(lang, "确认创建互动影游缺少标题，请重新生成确认卡。", "The interactive film creation confirmation is missing a title. Regenerate the confirmation card."),
                )
            })?.to_string();
            params.insert("title".into(), json!(title));
            params.insert("instruction".into(), json!(request.instruction));
            for name in ["sourceKind", "sourceText", "sourcePath", "requirements", "targetAudience", "episodeDuration", "budget", "referenceMode", "projectId", "outDir"] {
                if let Some(value) = field(name) {
                    params.insert(name.into(), json!(value));
                }
            }
            if let Some(count) = payload.and_then(|p| p.get("episodeCount")).and_then(Value::as_u64) {
                params.insert("episodeCount".into(), json!(count));
            }
            agent = None;
        }
        RequestedIntent::GenerateCover => {
            let payload = request.action_payload.and_then(|p| p.get("generateCover"));
            let field = |name: &str| {
                payload
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            };
            let title = field("title").ok_or_else(|| {
                production_exec_error(
                    lang,
                    pick(lang, "确认生成封面缺少标题，请重新生成确认卡。", "The cover generation confirmation is missing a title. Regenerate the confirmation card."),
                )
            })?.to_string();
            params.insert("title".into(), json!(title));
            for name in ["intro", "sellingPoints", "coverPrompt", "outputDir"] {
                if let Some(value) = field(name) {
                    params.insert(name.into(), json!(value));
                }
            }
            agent = None;
        }
        RequestedIntent::TranslationCreate => {
            let payload = request.action_payload.and_then(|p| p.get("translationCreate"));
            let field = |name: &str| {
                payload
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            };
            let file_path = field("filePath").ok_or_else(|| {
                production_exec_error(
                    lang,
                    pick(
                        lang,
                        "确认创建翻译项目缺少文件路径，请重新生成确认卡。",
                        "The translation confirmation is missing a file path. Regenerate the confirmation card.",
                    ),
                )
            })?.to_string();
            let source_language = field("sourceLanguage").ok_or_else(|| {
                production_exec_error(
                    lang,
                    pick(
                        lang,
                        "确认创建翻译项目缺少源语言，请重新生成确认卡。",
                        "The translation confirmation is missing a source language. Regenerate the confirmation card.",
                    ),
                )
            })?.to_string();
            let target_language = field("targetLanguage").ok_or_else(|| {
                production_exec_error(
                    lang,
                    pick(
                        lang,
                        "确认创建翻译项目缺少目标语言，请重新生成确认卡。",
                        "The translation confirmation is missing a target language. Regenerate the confirmation card.",
                    ),
                )
            })?.to_string();
            params.insert("filePath".into(), json!(file_path));
            params.insert("sourceLanguage".into(), json!(source_language));
            params.insert("targetLanguage".into(), json!(target_language));
            if let Some(title) = field("title") {
                params.insert("title".into(), json!(title));
            }
            if let Some(segment_max_chars) = payload
                .and_then(|p| p.get("segmentMaxChars"))
                .and_then(Value::as_u64)
            {
                params.insert("segmentMaxChars".into(), json!(segment_max_chars));
            }
            agent = None;
        }
        RequestedIntent::ConnectChoice => {
            let payload = request.action_payload.and_then(|p| p.get("connectChoice"));
            let node_value = payload
                .and_then(|p| p.get("node"))
                .cloned()
                .filter(|node| node.is_object())
                .ok_or_else(|| {
                    production_exec_error(
                        lang,
                        pick(
                            lang,
                            "确认连接选择缺少节点数据，请重新生成确认卡。",
                            "The connect-choice confirmation is missing node data. Regenerate the confirmation card.",
                        ),
                    )
                })?;
            // projectId 兜底链：payload.projectId ?? bookId。
            let project_id = payload
                .and_then(|p| p.get("projectId"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| request.book_id.map(String::from))
                .ok_or_else(|| production_exec_error(lang, "interactive-film action requires a project id (bookId)".to_string()))?;
            params.insert("node".into(), node_value);
            params.insert("projectId".into(), json!(project_id));
            agent = None;
        }
        RequestedIntent::RemoveNode => {
            let payload = request.action_payload.and_then(|p| p.get("removeNode"));
            let node_id = payload
                .and_then(|p| p.get("nodeId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    production_exec_error(
                        lang,
                        pick(
                            lang,
                            "确认删除节点缺少 nodeId，请重新生成确认卡。",
                            "The remove-node confirmation is missing a nodeId. Regenerate the confirmation card.",
                        ),
                    )
                })?
                .to_string();
            let project_id = payload
                .and_then(|p| p.get("projectId"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| request.book_id.map(String::from))
                .ok_or_else(|| production_exec_error(lang, "interactive-film action requires a project id (bookId)".to_string()))?;
            params.insert("nodeId".into(), json!(node_id));
            params.insert("projectId".into(), json!(project_id));
            agent = None;
        }
        RequestedIntent::DraftStructure => {
            let payload = request.action_payload.and_then(|p| p.get("draftStructure"));
            let project_id = payload
                .and_then(|p| p.get("projectId"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| request.book_id.map(String::from))
                .ok_or_else(|| production_exec_error(lang, "interactive-film action requires a project id (bookId)".to_string()))?;
            let instruction = payload
                .and_then(|p| p.get("instruction"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(request.instruction)
                .to_string();
            params.insert("projectId".into(), json!(project_id));
            params.insert("instruction".into(), json!(instruction));
            agent = None;
        }
        RequestedIntent::PlayStart => {
            let payload = request.action_payload.and_then(|p| p.get("playStart"));
            let field = |name: &str| {
                payload
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            };
            let title = field("title")
                .ok_or_else(|| {
                    production_exec_error(
                        lang,
                        pick(
                            lang,
                            "确认启动互动世界缺少标题，请重新生成确认卡。",
                            "The interactive world start confirmation is missing a title. Regenerate the confirmation card.",
                        ),
                    )
                })?
                .to_string();
            params.insert("title".into(), json!(title));
            if let Some(premise) = field("premise") {
                params.insert("premise".into(), json!(premise));
            }
            if let Some(contract) = field("worldContract") {
                params.insert("worldContract".into(), json!(contract));
            }
            if let Some(visual) = field("visualContract") {
                params.insert("visualContract".into(), json!(visual));
            }
            if let Some(mode) = field("mode") {
                params.insert("mode".into(), json!(mode));
            }
            if let Some(scene) = field("initialScene") {
                params.insert("initialScene".into(), json!(scene));
            }
            if let Some(actions) = payload
                .and_then(|p| p.get("suggestedActions"))
                .and_then(Value::as_array)
            {
                params.insert(
                    "suggestedActions".into(),
                    json!(actions
                        .iter()
                        .filter_map(|item| item.as_str().map(String::from))
                        .collect::<Vec<_>>()),
                );
            }
            agent = None;
        }
        RequestedIntent::CreateBook => {
            // requirePayloadText：缺失文案（最终经统一错误面 → 502）。
            let title = request
                .action_payload
                .and_then(|p| p.get("createBook"))
                .and_then(|p| p.get("title"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .ok_or_else(|| {
                    production_exec_error(lang, pick(
                        lang,
                        "确认建书缺少书名，请重新生成确认卡。",
                        "The book creation confirmation is missing a title. Regenerate the confirmation card.",
                    ))
                })?;
            let payload = request
                .action_payload
                .and_then(|p| p.get("createBook"));
            let optional_str = |name: &str| {
                payload
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            };
            params.insert("agent".into(), json!("architect"));
            params.insert("instruction".into(), json!(request.instruction));
            params.insert("title".into(), json!(title));
            if let Some(genre) = optional_str("genre") {
                params.insert("genre".into(), json!(genre));
            }
            if let Some(platform) = optional_str("platform") {
                params.insert("platform".into(), json!(platform));
            }
            if let Some(language) = optional_str("language") {
                params.insert("language".into(), json!(language));
            }
            let optional_num = |name: &str| {
                payload
                    .and_then(|p| p.get(name))
                    .and_then(Value::as_f64)
                    .filter(|v| v.fract() == 0.0 && *v >= 1.0)
            };
            if let Some(target_chapters) = optional_num("targetChapters") {
                params.insert("targetChapters".into(), json!(target_chapters as u64));
            }
            if let Some(chapter_word_count) = optional_num("chapterWordCount") {
                params.insert("chapterWordCount".into(), json!(chapter_word_count as u64));
            }
            agent = Some("architect");
        }
        RequestedIntent::WriteNext => {
            let book_id = request.book_id.ok_or_else(|| {
                production_exec_error(
                    lang,
                    pick(
                        lang,
                        "写下一章需要先打开一本书。",
                        "Writing the next chapter requires an active book.",
                    ),
                )
            })?;
            let chapter_count = request
                .action_payload
                .and_then(|p| p.get("writeNext"))
                .and_then(|p| p.get("chapterCount"))
                .and_then(Value::as_f64)
                .map(|v| v as u32)
                .unwrap_or(1);
            params.insert("agent".into(), json!("writer"));
            params.insert("bookId".into(), json!(book_id));
            agent = Some("writer");
            let _ = chapter_count; // 消费面在执行器
        }
        other => {
            return Err(production_exec_error(
                lang,
                format!("Unsupported confirmed action: {}", other.as_str()),
            ));
        }
    }
    let tool_name = match request.intent {
        RequestedIntent::PlayStart => "play_start",
        RequestedIntent::ConnectChoice => "connect_choice",
        RequestedIntent::RemoveNode => "remove_node",
        RequestedIntent::DraftStructure => "draft_structure",
        RequestedIntent::TranslationCreate => "translation_create",
        RequestedIntent::ScriptCreate => "script_create",
        RequestedIntent::StoryboardCreate => "storyboard_create",
        RequestedIntent::InteractiveFilmCreate => "interactive_film_create",
        RequestedIntent::GenerateCover => "generate_cover",
        RequestedIntent::ShortRun => "short_fiction_run",
        _ => "sub_agent",
    };

    let stages: Option<Vec<StudioTaskStage>> = agent.and_then(|agent| {
        pipeline_stages(agent, lang).map(|labels| {
            labels
                .into_iter()
                .map(|label| StudioTaskStage {
                    label,
                    status: StudioTaskStageStatus::Pending,
                })
                .collect()
        })
    });
    let mut exec = CollectedToolExec {
        id: task_id.to_string(),
        tool: tool_name.to_string(),
        agent: agent.map(String::from),
        label: resolve_tool_label(tool_name, agent, lang),
        status: "running",
        args: Some(params),
        result: None,
        details: None,
        error: None,
        stages,
        logs: None,
        started_at: utc_now_ms() as f64,
        completed_at: None,
    };

    persist_confirmed_task(root, session_id, request.intent, &exec, request.source_request_id)
        .await;

    // background: true 标明后台生产任务的工具启动（前端把聊天轮重分类为任务轮）。
    {
        let mut payload = json!({
            "sessionId": session_id,
            "id": exec.id,
            "tool": exec.tool,
            "args": exec.args,
            "background": true,
        });
        if let Some(stages) = &exec.stages {
            payload["stages"] =
                json!(stages.iter().map(|s| s.label.clone()).collect::<Vec<_>>());
        }
        if let Some(source_request_id) = request.source_request_id {
            payload["sourceRequestId"] = json!(source_request_id);
        }
        runtime.hub.broadcast("tool:start", &payload);
    }

    // ── 执行（进度回调 → logs 快照；progress 不改 status，对齐 TS onUpdate） ──
    // 进度持久化经 spawn 保持回调同步；句柄入 sink 队列，终态持久化前 drain
    // 等待——否则滞后的进度快照（running）会覆盖终态快照（completed/error）。
    /// 进度面共享 sink：logs 累积 + spawn 的进度持久化句柄队列（终态前 drain）。
    type ProgressSink = (Vec<String>, Vec<tokio::task::JoinHandle<()>>);
    let sink: Arc<Mutex<ProgressSink>> = Arc::new(Mutex::new((Vec::new(), Vec::new())));
    let make_on_progress = {
        let sink = sink.clone();
        let base = exec.clone();
        let root_log = root.to_path_buf();
        let session_log = session_id.to_string();
        let intent = request.intent;
        let source_id = request.source_request_id.map(String::from);
        move |message: String| {
            let handle = {
                let mut sink = sink.lock().unwrap();
                sink.0.push(message);
                let mut entry = base.clone();
                let mut logs = sink.0.clone();
                logs.truncate(80);
                entry.logs = Some(logs);
                let root_log = root_log.clone();
                let session_log = session_log.clone();
                let source_id = source_id.clone();
                tokio::spawn(async move {
                    persist_confirmed_task(
                        &root_log,
                        &session_log,
                        intent,
                        &entry,
                        source_id.as_deref(),
                    )
                    .await;
                })
            };
            sink.lock().unwrap().1.push(handle);
        }
    };
    let outcome = match request.intent {
        RequestedIntent::ShortRun => {
            let mut on_progress = make_on_progress;
            let args = exec.args.clone().unwrap_or_default();
            let field = |name: &str| {
                args.get(name)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_default()
            };
            // 最终语言：payload.shortRun.language ?? 会话语言（TS tool 语义）。
            let language = match request
                .action_payload
                .and_then(|p| p.get("shortRun"))
                .and_then(|p| p.get("language"))
                .and_then(Value::as_str)
            {
                Some("en") => crate::utils::language::WritingLanguage::En,
                Some(_) => crate::utils::language::WritingLanguage::Zh,
                None => match lang {
                    StudioLang::En => crate::utils::language::WritingLanguage::En,
                    StudioLang::Zh => crate::utils::language::WritingLanguage::Zh,
                },
            };
            let chars_per_chapter = args
                .get("charsPerChapter")
                .and_then(Value::as_u64)
                .map(|v| v as u32);
            // 越界组合（如 zh 会话 + en 卡面 1100）在任务开跑前拦截（双语错误）。
            // 以 Err 值走统一错误面（快照 error 态 + tool:end + 502——TS throw 同链）。
            if let Err(message) = assert_short_run_chars_per_chapter(chars_per_chapter, language) {
                Err(message)
            } else {
                let optional = |name: &str| {
                    let value = field(name);
                    if value.is_empty() { None } else { Some(value) }
                };
                // cover 缺省 = true（TS `options.cover === false` 才禁用）。
                let cover = args
                    .get("cover")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                execute_short_run(
                    runtime,
                    &field("direction"),
                    optional("reference").as_deref(),
                    optional("storyId").as_deref(),
                    args.get("chapters").and_then(Value::as_u64).map(|v| v as u32),
                    chars_per_chapter,
                    language,
                    cover,
                    &mut on_progress,
                )
                .await
            }
        }
        RequestedIntent::ScriptCreate => {
            let mut on_progress = make_on_progress;
            let args = exec.args.clone().unwrap_or_default();
            let field = |name: &str| {
                args.get(name)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_default()
            };
            execute_script_create(
                runtime,
                &field("title"),
                request.instruction,
                &field("sourceKind"),
                &field("targetFormat"),
                &field("sourceText"),
                &field("sourcePath"),
                &field("requirements"),
                args.get("episodeCount").and_then(Value::as_u64).map(|v| v as u32),
                &field("episodeDuration"),
                &field("projectId"),
                &field("outDir"),
                &mut on_progress,
            )
            .await
        }
        RequestedIntent::StoryboardCreate => {
            let mut on_progress = make_on_progress;
            let args = exec.args.clone().unwrap_or_default();
            let field = |name: &str| {
                args.get(name)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_default()
            };
            execute_storyboard_create(
                runtime,
                &field("title"),
                request.instruction,
                &field("sourceKind"),
                &field("sourceText"),
                &field("sourcePath"),
                &field("requirements"),
                &field("visualStyle"),
                &field("aspectRatio"),
                &field("granularity"),
                args.get("maxShots").and_then(Value::as_u64).map(|v| v as u32),
                &field("projectId"),
                &field("outDir"),
                &mut on_progress,
            )
            .await
        }
        RequestedIntent::InteractiveFilmCreate => {
            let mut on_progress = make_on_progress;
            let args = exec.args.clone().unwrap_or_default();
            let field = |name: &str| {
                args.get(name)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_default()
            };
            execute_interactive_film_create(
                runtime,
                &field("title"),
                request.instruction,
                &field("sourceKind"),
                &field("sourceText"),
                &field("sourcePath"),
                &field("requirements"),
                &field("targetAudience"),
                args.get("episodeCount").and_then(Value::as_u64).map(|v| v as u32),
                &field("episodeDuration"),
                &field("budget"),
                &field("referenceMode"),
                &field("projectId"),
                &field("outDir"),
                &mut on_progress,
            )
            .await
        }
        RequestedIntent::GenerateCover => {
            let mut on_progress = make_on_progress;
            let args = exec.args.clone().unwrap_or_default();
            let field = |name: &str| {
                args.get(name)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_default()
            };
            let selling_points = crate::llm::cover::normalize_selling_points(Some(&field("sellingPoints")));
            execute_generate_cover(
                runtime,
                &field("title"),
                &field("intro"),
                &selling_points,
                &field("coverPrompt"),
                &field("outputDir"),
                &mut on_progress,
            )
            .await
        }
        RequestedIntent::TranslationCreate => {
            let mut on_progress = make_on_progress;
            let args = exec.args.clone().unwrap_or_default();
            let field = |name: &str| {
                args.get(name)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            execute_translation_create(
                runtime,
                &field("filePath"),
                &field("sourceLanguage"),
                &field("targetLanguage"),
                &field("title"),
                args.get("segmentMaxChars").and_then(Value::as_u64).map(|v| v as usize),
                &mut on_progress,
            )
            .await
        }
        RequestedIntent::ConnectChoice => {
            let mut on_progress = make_on_progress;
            let args = exec.args.clone().unwrap_or_default();
            let project_id = args.get("projectId").and_then(Value::as_str).unwrap_or_default().to_string();
            let node_value = args.get("node").cloned().unwrap_or(Value::Null);
            execute_connect_choice(runtime, &project_id, &node_value, &mut on_progress).await
        }
        RequestedIntent::RemoveNode => {
            let mut on_progress = make_on_progress;
            let args = exec.args.clone().unwrap_or_default();
            let project_id = args.get("projectId").and_then(Value::as_str).unwrap_or_default().to_string();
            let node_id = args.get("nodeId").and_then(Value::as_str).unwrap_or_default().to_string();
            execute_remove_node(runtime, &project_id, &node_id, &mut on_progress).await
        }
        RequestedIntent::DraftStructure => {
            let mut on_progress = make_on_progress;
            let args = exec.args.clone().unwrap_or_default();
            let project_id = args.get("projectId").and_then(Value::as_str).unwrap_or_default().to_string();
            let instruction = args.get("instruction").and_then(Value::as_str).unwrap_or_default().to_string();
            execute_draft_structure(runtime, &project_id, &instruction, &mut on_progress).await
        }
        RequestedIntent::PlayStart => {
            let mut on_progress = make_on_progress;
            let args = exec.args.clone().unwrap_or_default();
            let field = |name: &str| {
                args.get(name)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_default()
            };
            execute_play_start(
                runtime,
                request.session_id,
                request.play_mode,
                &field("title"),
                &field("premise"),
                &field("worldContract"),
                &field("visualContract"),
                &field("mode"),
                &field("initialScene"),
                args.get("suggestedActions")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                &mut on_progress,
            )
            .await
        }
        RequestedIntent::CreateBook => {
            let mut on_progress = make_on_progress;
            let title = exec
                .args
                .as_ref()
                .and_then(|args| args.get("title"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            execute_create_book(
                runtime,
                request.instruction,
                &title,
                request.action_payload,
                &mut on_progress,
            )
            .await
        }
        RequestedIntent::WriteNext => {
            let book_id = exec
                .args
                .as_ref()
                .and_then(|args| args.get("bookId"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let chapter_count = request
                .action_payload
                .and_then(|p| p.get("writeNext"))
                .and_then(|p| p.get("chapterCount"))
                .and_then(Value::as_f64)
                .map(|v| v as u32)
                .unwrap_or(1);
            let mut on_progress = make_on_progress;
            execute_write_next(runtime, &book_id, chapter_count, lang, abort, &mut on_progress)
                .await
        }
        _ => unreachable!("执行器装配已过滤未支持 intent"),
    };

    // 终态持久化前 drain 进度写入（保证快照终态不被滞后的进度快照覆盖）。
    {
        let handles: Vec<_> = sink.lock().unwrap().1.drain(..).collect();
        for handle in handles {
            let _ = handle.await;
        }
        let mut logs = sink.lock().unwrap().0.clone();
        logs.truncate(80);
        if !logs.is_empty() {
            exec.logs = Some(logs);
        }
    }

    match outcome {
        Ok(tool_outcome) => {
            exec.status = if tool_outcome.is_error {
                "error"
            } else {
                "completed"
            };
            exec.completed_at = Some(utc_now_ms() as f64);
            exec.result = Some(tool_outcome.text.clone());
            exec.details = Some(tool_outcome.details.clone());
            if let Some(stages) = &mut exec.stages {
                for stage in stages {
                    stage.status = StudioTaskStageStatus::Completed;
                }
            }
            persist_confirmed_task(root, session_id, request.intent, &exec, request.source_request_id)
                .await;
            runtime.hub.broadcast(
                "tool:end",
                &json!({
                    "sessionId": session_id,
                    "id": exec.id,
                    "tool": exec.tool,
                    "result": { "content": [{ "type": "text", "text": tool_outcome.text }], "details": tool_outcome.details },
                    "details": tool_outcome.details,
                    "isError": tool_outcome.is_error,
                }),
            );

            // ── 建书迁移：architect 完成 → 绑定会话 + book:created ──
            let mut active_book_id = request.book_id.map(String::from);
            if exec.tool == "sub_agent"
                && exec.agent.as_deref() == Some("architect")
                && exec.status == "completed"
            {
                let resolved = exec
                    .details
                    .as_ref()
                    .and_then(|d| d.get("bookId"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                if let Some(book_id) = resolved {
                    // 迁移失败（已绑定）静默跳过（TS catch SessionAlreadyMigratedError）。
                    let migrated =
                        crate::interaction::book_session_store::migrate_book_session(
                            root, session_id, &book_id,
                        )
                        .await
                        .ok()
                        .flatten();
                    let _ = migrated;
                    active_book_id = Some(book_id.clone());
                    let book_summary = runtime
                        .state
                        .load_book_config(&book_id)
                        .await
                        .ok()
                        .map(|config| json!({ "id": config.id, "title": config.title }));
                    crate::server::book_create_routes::set_create_status_removed(&book_id).await;
                    runtime.hub.broadcast(
                        "book:created",
                        &json!({ "bookId": book_id, "sessionId": session_id, "book": book_summary }),
                    );
                }
            }

            let response_text = exec
                .result
                .clone()
                .unwrap_or_else(|| pick(lang, "已完成。", "Done."));
            append_production_assistant_message(
                root,
                session_id,
                &response_text,
                &exec,
                &request.provider_label,
                &request.model_label,
                request.session_kind,
            )
            .await;
            Ok(ProductionOutcome {
                response_text: if suppress_manual_text_for_tool(&exec.tool) {
                    String::new()
                } else {
                    response_text
                },
                exec,
                active_book_id,
            })
        }
        Err(message) => {
            exec.status = "error";
            exec.completed_at = Some(utc_now_ms() as f64);
            exec.error = Some(message.clone());
            persist_confirmed_task(root, session_id, request.intent, &exec, request.source_request_id)
                .await;
            runtime.hub.broadcast(
                "tool:end",
                &json!({
                    "sessionId": session_id,
                    "id": exec.id,
                    "tool": exec.tool,
                    "result": { "content": [{ "type": "text", "text": message }] },
                    "isError": true,
                }),
            );
            // 建书失败：book:error 广播 + create-status error 态。
            if let Some(pending) = &pending_book_id {
                crate::server::book_create_routes::set_create_status(
                    pending,
                    crate::server::book_create_routes::BookCreateStatus {
                        status: "error".to_string(),
                        error: Some(message.clone()),
                    },
                )
                .await;
                runtime.hub.broadcast(
                    "book:error",
                    &json!({ "bookId": pending, "sessionId": session_id, "error": message }),
                );
            }
            // 失败同样只补助手工具消息（指令已在任务开始时写入）。
            append_production_assistant_message(
                root,
                session_id,
                &message,
                &exec,
                &request.provider_label,
                &request.model_label,
                request.session_kind,
            )
            .await;
            let (status, code, message) = format_agent_action_failure(&message);
            Err(ProductionError {
                status,
                code: code.to_string(),
                message,
                exec: Some(exec),
            })
        }
    }
}

/// 执行阶段错误（payload 缺失 / 未支持 intent）：与执行失败同错误面。
fn production_exec_error(_lang: StudioLang, message: String) -> ProductionError {
    let (status, code, message) = format_agent_action_failure(&message);
    ProductionError {
        status,
        code: code.to_string(),
        message,
        exec: None,
    }
}

/// `persistConfirmedTask`：快照落盘（已删除会话守卫）。
async fn persist_confirmed_task(
    root: &Path,
    session_id: &str,
    intent: RequestedIntent,
    exec: &CollectedToolExec,
    source_request_id: Option<&str>,
) {
    if crate::server::session_routes::deleted_session_ids()
        .lock()
        .unwrap()
        .contains(session_id)
    {
        return;
    }
    let snapshot = StudioTaskSnapshot {
        version: 1,
        session_id: session_id.to_string(),
        source_request_id: source_request_id.map(String::from),
        requested_intent: intent.as_str().to_string(),
        updated_at: utc_now_ms() as f64,
        execution: exec.to_execution(),
    };
    let _ = save_studio_task_snapshot(root, &snapshot).await;
}

#[cfg(test)]
mod model_resolver_tests {
    use super::*;

    #[tokio::test]
    async fn layer1_missing_key_returns_bilingual_400() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // deepseek preset base_url 非本地端点；无 secrets → 层 1 无 key 400。
        let result = resolve_agent_model_override(&root, Some("deepseek"), Some("deepseek-chat")).await;
        let (status, body) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.0["error"], "请先为 deepseek 配置 API Key");
        assert_eq!(
            body.0["response"],
            "请先在模型配置中为 deepseek 填写 API Key，然后再试。"
        );
        // en 项目语言。
        std::fs::write(root.join("inkos.json"), r#"{"language":"en"}"#).unwrap();
        let (_, body) = resolve_agent_model_override(&root, Some("deepseek"), Some("deepseek-chat"))
            .await
            .unwrap_err();
        assert_eq!(body.0["error"], "Configure an API Key for deepseek first");
        assert_eq!(
            body.0["response"],
            "Fill in an API Key for deepseek in the model settings, then try again."
        );
    }

    #[tokio::test]
    async fn layer1_with_key_and_layer4_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // 层 1 命中：secrets 有 key + 非本地无 baseUrl 的 preset → 500 错误面？
        // 用 custom 服务（inkos.json services 带 baseUrl）。
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("inkos.json"),
            r#"{"llm":{"services":[{"service":"custom:my","baseUrl":"http://127.0.0.1:9","apiFormat":"chat"}]}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{"services":{"custom:my":{"apiKey":"sk-x"}}}"#,
        )
        .unwrap();
        let resolved = resolve_agent_model_override(&root, Some("custom:my"), Some("m1")).await.unwrap().unwrap();
        assert_eq!(resolved.service, "custom:my");
        assert_eq!(resolved.model, "m1");
        assert_eq!(resolved.api_key, "sk-x");
        assert_eq!(resolved.base_url, "http://127.0.0.1:9");

        // 层 4：无显式 service/model 且无 defaultModel/有 key 服务（custom:my
        // 的 models 探测不可达 127.0.0.1:9 → 静默下落）→ None。
        let resolved = resolve_agent_model_override(&root, None, None).await.unwrap();
        // 层 3 可能命中 custom:my（bank 无 custom 条目 → listModels 空 → 下落）。
        assert!(resolved.is_none());
    }
}

#[cfg(test)]
mod model_guard_tests {
    use super::*;

    #[test]
    fn text_model_id_detection() {
        assert!(is_text_chat_model_id("gemini-2.5-flash"));
        assert!(is_text_chat_model_id("  GLM-4.7 "));
        assert!(!is_text_chat_model_id(""));
        assert!(!is_text_chat_model_id("some-image-model"));
        assert!(!is_text_chat_model_id("text-embedding-3"));
        assert!(!is_text_chat_model_id("tts-1-hd"));
        assert!(!is_text_chat_model_id("whisper-audio"));
    }

    #[test]
    fn non_text_model_message_bilingual() {
        let zh = non_text_model_message("img-x", StudioLang::Zh);
        assert!(zh.starts_with("模型 img-x 不适合文本聊天/写作。"), "{zh}");
        let en = non_text_model_message("img-x", StudioLang::En);
        assert!(en.starts_with("Model img-x is not suitable"), "{en}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_chapter_heuristics_match_ts_regexes() {
        // isExplicitWriteChapterCommand（zh 正反例逐字）。
        for hit in ["写下一章", "帮我写第一章！", "请写第一章", "续写一章，突出冲突。", "接着写正文", "直接写章节"] {
            assert!(is_explicit_write_chapter_command(hit), "应命中：{hit}");
        }
        // TS 前缀组仅单个可选词：双前缀（请帮我）不命中——逐字固化。
        for miss in ["写个大纲", "帮我看看第一章", "请帮我写第一章！", "现在开始写正文", "你好", "write a summary"] {
            assert!(!is_explicit_write_chapter_command(miss), "不应命中：{miss}");
        }
        // en 分支（词边界 + 可选 the）。
        assert!(is_explicit_write_chapter_command("write the next chapter"));
        assert!(is_explicit_write_chapter_command("Please continue chapter one"));
        assert!(!is_explicit_write_chapter_command("writing chapters"));

        // isWriteNextInstruction（全词匹配；不含 /write）。
        for hit in ["继续", "继续写", "写下一章", "continue", "WRITE NEXT", "再来一章"] {
            assert!(is_write_next_instruction(hit), "应命中：{hit}");
        }
        for miss in ["/write", "继续写两章", "  ", "continue!"] {
            assert!(!is_write_next_instruction(miss), "不应命中：{miss}");
        }
    }

    #[test]
    fn resolve_confirmed_intent_normalizes_write_next_sources() {
        // 三来源归一：显式 intent / free-text 明确命令 / 其它来源写作指令。
        assert_eq!(
            resolve_confirmed_intent(
                "写下一章",
                Some("b1"),
                SessionKind::Book,
                ActionSource::FreeText,
                None,
            ),
            Some(RequestedIntent::WriteNext)
        );
        assert_eq!(
            resolve_confirmed_intent(
                "继续",
                Some("b1"),
                SessionKind::Book,
                ActionSource::QuickAction,
                None,
            ),
            Some(RequestedIntent::WriteNext)
        );
        assert_eq!(
            resolve_confirmed_intent(
                "随便聊聊",
                Some("b1"),
                SessionKind::Book,
                ActionSource::FreeText,
                None,
            ),
            None
        );
        // 无书 / 非书籍会话 → 聊天分支。
        assert_eq!(
            resolve_confirmed_intent(
                "写下一章",
                None,
                SessionKind::Book,
                ActionSource::FreeText,
                None,
            ),
            None
        );
        assert_eq!(
            resolve_confirmed_intent(
                "写下一章",
                None,
                SessionKind::Chat,
                ActionSource::FreeText,
                None,
            ),
            None
        );
        // button/slash 确认 intent 透传。
        assert_eq!(
            resolve_confirmed_intent(
                "建书",
                None,
                SessionKind::Chat,
                ActionSource::Button,
                Some(RequestedIntent::CreateBook),
            ),
            Some(RequestedIntent::CreateBook)
        );
        // write_next 显式 intent 在无书时不走任务分支（BOOK_ID_REQUIRED 由
        // 执行器错误面承担——TS isWriteNextProductionRequest 先挡）。
        assert_eq!(
            resolve_confirmed_intent(
                "写",
                None,
                SessionKind::Book,
                ActionSource::Button,
                Some(RequestedIntent::WriteNext),
            ),
            None
        );
    }

    #[test]
    fn agent_action_failure_classification() {
        let (status, code, _) =
            format_agent_action_failure("BookWriteLockError: locked by an active InkOS write");
        assert_eq!((status, code), (StatusCode::CONFLICT, "BOOK_BUSY"));

        let (status, code, message) = format_agent_action_failure("确认建书缺少书名，请重新生成确认卡。");
        assert_eq!((status, code), (StatusCode::BAD_GATEWAY, "AGENT_ACTION_FAILED"));
        assert_eq!(message, "确认建书缺少书名，请重新生成确认卡。");
    }

    #[test]
    fn enum_parsing_rejects_unknown_values() {
        assert_eq!(ActionSource::parse("quick-action"), Some(ActionSource::QuickAction));
        assert_eq!(ActionSource::parse("free text"), None);
        assert_eq!(RequestedIntent::parse("create_book"), Some(RequestedIntent::CreateBook));
        assert_eq!(RequestedIntent::parse("create-book"), None);
        assert!(normalize_action_source(Some(&json!("button"))).is_ok());
        assert!(normalize_action_source(Some(&json!("weird"))).is_err());
        assert!(normalize_requested_intent(Some(&json!(""))).unwrap().is_none());
        assert!(normalize_requested_intent(Some(&json!("nope"))).is_err());
    }

    #[test]
    fn tool_label_and_stage_tables_align_ts() {
        assert_eq!(resolve_tool_label("sub_agent", Some("writer"), StudioLang::Zh), "写作");
        assert_eq!(resolve_tool_label("sub_agent", Some("architect"), StudioLang::Zh), "建书");
        assert_eq!(resolve_tool_label("read", None, StudioLang::Zh), "读取文件");
        assert_eq!(resolve_tool_label("sub_agent", Some("unknown-agent"), StudioLang::Zh), "unknown-agent");
        assert_eq!(pipeline_stages("writer", StudioLang::Zh).unwrap().len(), 7);
        assert_eq!(pipeline_stages("architect", StudioLang::Zh).unwrap().len(), 5);
        assert_eq!(pipeline_stages("exporter", StudioLang::Zh), None);
    }

    #[test]
    fn running_task_context_block_renders_zh() {
        let task = StudioTaskSnapshot {
            version: 1,
            session_id: "s".into(),
            source_request_id: None,
            requested_intent: "write_next".into(),
            updated_at: 1000.0,
            execution: StudioTaskExecution {
                id: "t".into(),
                tool: "sub_agent".into(),
                agent: Some("writer".into()),
                label: "写作".into(),
                status: StudioTaskExecutionStatus::Running,
                args: None,
                result: None,
                details: None,
                error: None,
                stages: None,
                logs: Some(vec!["正在为 b1 写下一章…".into()]),
                started_at: 500.0,
                completed_at: None,
            },
        };
        let block = build_running_task_context_block(&task, StudioLang::Zh);
        assert!(block.contains("## 后台任务状态"));
        assert!(block.contains("- 任务：写作（sub_agent）"));
        assert!(block.contains("- 状态：运行中"));
        assert!(block.contains("正在为 b1 写下一章…"));
    }

    #[test]
    fn base36_matches_js_date_tostring() {
        assert_eq!(to_base36(0), "0");
        assert_eq!(to_base36(35), "z");
        assert_eq!(to_base36(36), "10");
        assert_eq!(to_base36(1782988000000), "mr3d0pog");
    }
}
