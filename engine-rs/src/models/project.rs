//! 项目配置模型（inkos.json 契约）。
//!
//! 移植自 `packages/core/src/models/project.ts`（182 行）。
//! 含 LLM 配置 / 通知通道（discriminated union）/ 检测 / 写作 / daemon 调度等子配置。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::collections::HashMap;

// ── LLM ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LLMServiceEntry {
    pub service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_format: Option<LlmApiFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// LLM API 格式。对齐 TS `z.enum(["chat","responses"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"chat\" | \"responses\""))]
pub enum LlmApiFormat {
    #[serde(rename = "chat")]
    Chat,
    #[serde(rename = "responses")]
    Responses,
}

/// LLM provider。对齐 TS `z.enum(["anthropic","openai","custom"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"anthropic\" | \"openai\" | \"custom\""))]
pub enum LlmProvider {
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "openai")]
    Openai,
    #[serde(rename = "custom")]
    Custom,
}

/// 配置来源。对齐 TS `z.enum(["env","studio"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"env\" | \"studio\""))]
pub enum LlmConfigSource {
    #[serde(rename = "env")]
    Env,
    #[serde(rename = "studio")]
    Studio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LLMCoverConfig {
    pub service: CoverService,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

/// 封面服务。对齐 TS `z.enum(["kkaiapi","openai","google"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"kkaiapi\" | \"openai\" | \"google\""))]
pub enum CoverService {
    #[serde(rename = "kkaiapi")]
    Kkaiapi,
    #[serde(rename = "openai")]
    Openai,
    #[serde(rename = "google")]
    Google,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LLMConfig {
    pub provider: LlmProvider,
    #[serde(default = "default_service")]
    pub service: String,
    #[serde(default = "default_config_source")]
    pub config_source: LlmConfigSource,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default)]
    pub thinking_budget: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default = "default_chat")]
    pub api_format: LlmApiFormat,
    #[serde(default = "default_true")]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub services: Option<Vec<LLMServiceEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    pub cover: Option<LLMCoverConfig>,
}

fn default_service() -> String { "custom".into() }
fn default_config_source() -> LlmConfigSource { LlmConfigSource::Env }
fn default_temperature() -> f64 { 0.7 }
fn default_chat() -> LlmApiFormat { LlmApiFormat::Chat }
fn default_true() -> bool { true }

// ── 通知通道（discriminated union on "type"）────────────────────

/// 通知格式。对齐 TS `z.enum(["markdown","text"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"markdown\" | \"text\""))]
pub enum NotifyFormat {
    #[serde(rename = "markdown")]
    Markdown,
    #[serde(rename = "text")]
    Text,
}

/// 通知通道（按 `type` 鉴别的联合）。对齐 TS `NotifyChannelSchema` discriminated union。
/// 变体名 kebab-case 作 tag 值；字段手动 rename 为 camelCase（enum 级 rename_all 已用于变体）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum NotifyChannel {
    Telegram {
        #[serde(rename = "botToken")]
        bot_token: String,
        #[serde(rename = "chatId")]
        chat_id: String,
        #[serde(default = "default_markdown")]
        format: NotifyFormat,
    },
    #[serde(rename = "wechat-work")]
    WechatWork {
        #[serde(rename = "webhookUrl")]
        webhook_url: String,
        #[serde(default = "default_markdown")]
        format: NotifyFormat,
    },
    Feishu {
        #[serde(rename = "webhookUrl")]
        webhook_url: String,
        #[serde(default = "default_markdown")]
        format: NotifyFormat,
    },
    Webhook {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        secret: Option<String>,
        #[serde(default)]
        events: Vec<String>,
        #[serde(default = "default_markdown")]
        format: NotifyFormat,
    },
}

fn default_markdown() -> NotifyFormat { NotifyFormat::Markdown }

// ── 检测 / 质量门 / 写作 ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DetectionConfig {
    #[serde(default = "default_detection_provider")]
    pub provider: DetectionProvider,
    pub api_url: String,
    pub api_key_env: String,
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub auto_rewrite: bool,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"gptzero\" | \"originality\" | \"custom\""))]
pub enum DetectionProvider {
    #[serde(rename = "gptzero")]
    Gptzero,
    #[serde(rename = "originality")]
    Originality,
    #[serde(rename = "custom")]
    Custom,
}
fn default_detection_provider() -> DetectionProvider { DetectionProvider::Custom }
fn default_threshold() -> f64 { 0.5 }
fn default_max_retries() -> u32 { 3 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct QualityGates {
    #[serde(default = "default_qg_max_audit")]
    pub max_audit_retries: u32,
    #[serde(default = "default_qg_pause")]
    pub pause_after_consecutive_failures: u32,
    #[serde(default = "default_qg_step")]
    pub retry_temperature_step: f64,
}
fn default_qg_max_audit() -> u32 { 2 }
fn default_qg_pause() -> u32 { 3 }
fn default_qg_step() -> f64 { 0.1 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct FoundationConfig {
    #[serde(default = "default_foundation_retries")]
    pub review_retries: u32,
}
fn default_foundation_retries() -> u32 { 2 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WritingConfig {
    #[serde(default = "default_writing_retries")]
    pub review_retries: u32,
    #[serde(default = "default_review_mode")]
    pub review_mode: super::book::ChapterReviewModeVal,
    #[serde(default = "default_revision_gate")]
    pub revision_gate: super::book::RevisionGateVal,
}
fn default_writing_retries() -> u32 { 1 }
fn default_review_mode() -> super::book::ChapterReviewModeVal { super::book::ChapterReviewModeVal::Auto }
fn default_revision_gate() -> super::book::RevisionGateVal { super::book::RevisionGateVal::Strict }

// ── Agent LLM 覆盖 / 输入治理 / 研究搜索 ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AgentLLMOverride {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<LlmProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// 模型覆盖值：string | AgentLLMOverride（对齐 TS `z.union`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ModelOverrideValue {
    Str(String),
    Override(AgentLLMOverride),
}

/// 输入治理模式。对齐 TS `z.enum(["legacy","v2"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"legacy\" | \"v2\""))]
pub enum InputGovernanceMode {
    #[serde(rename = "legacy")]
    Legacy,
    #[serde(rename = "v2")]
    V2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ResearchSearchConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_research_provider")]
    pub provider: ResearchProvider,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"tavily\" | \"custom\""))]
pub enum ResearchProvider {
    #[serde(rename = "tavily")]
    Tavily,
    #[serde(rename = "custom")]
    Custom,
}
fn default_research_provider() -> ResearchProvider { ResearchProvider::Tavily }

// ── Daemon 调度 ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct DaemonSchedule {
    pub radar_cron: String,
    pub write_cron: String,
}
impl Default for DaemonSchedule {
    fn default() -> Self {
        DaemonSchedule { radar_cron: "0 */6 * * *".into(), write_cron: "*/15 * * * *".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct DaemonConfig {
    pub schedule: DaemonSchedule,
    pub max_concurrent_books: u32,
    pub chapters_per_cycle: u32,
    pub retry_delay_ms: u64,
    pub cooldown_after_chapter_ms: u64,
    pub max_chapters_per_day: u32,
    pub quality_gates: QualityGates,
}
impl Default for DaemonConfig {
    fn default() -> Self {
        DaemonConfig {
            schedule: DaemonSchedule::default(),
            max_concurrent_books: 3,
            chapters_per_cycle: 1,
            retry_delay_ms: 30_000,
            cooldown_after_chapter_ms: 10_000,
            max_chapters_per_day: 50,
            quality_gates: QualityGates {
                max_audit_retries: 2,
                pause_after_consecutive_failures: 3,
                retry_temperature_step: 0.1,
            },
        }
    }
}

// ── ProjectConfig（顶层）────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub name: String,
    #[serde(rename = "version")]
    pub version: String, // z.literal("0.1.0")
    #[serde(default = "default_language")]
    pub language: String,
    pub llm: LLMConfig,
    #[serde(default)]
    pub notify: Vec<NotifyChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detection: Option<DetectionConfig>,
    #[serde(default = "default_foundation")]
    pub foundation: FoundationConfig,
    #[serde(default = "default_writing")]
    pub writing: WritingConfig,
    pub research_search: ResearchSearchConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_overrides: Option<HashMap<String, ModelOverrideValue>>,
    #[serde(default = "default_governance")]
    pub input_governance_mode: InputGovernanceMode,
    #[serde(default)]
    pub daemon: DaemonConfig,
}
fn default_language() -> String { "zh".into() }
fn default_governance() -> InputGovernanceMode { InputGovernanceMode::V2 }
fn default_foundation() -> FoundationConfig { FoundationConfig { review_retries: 2 } }
fn default_writing() -> WritingConfig { WritingConfig { review_retries: 1, review_mode: super::book::ChapterReviewModeVal::Auto, revision_gate: super::book::RevisionGateVal::Strict } }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_channel_discriminated_roundtrip() {
        let tg = NotifyChannel::Telegram { bot_token: "t".into(), chat_id: "c".into(), format: NotifyFormat::Markdown };
        let j = serde_json::to_string(&tg).unwrap();
        assert!(j.contains(r#""type":"telegram""#));
        let back: NotifyChannel = serde_json::from_str(&j).unwrap();
        assert_eq!(tg, back);
    }

    #[test]
    fn notify_wechat_work_kebab_tag() {
        let ww = NotifyChannel::WechatWork { webhook_url: "u".into(), format: NotifyFormat::Text };
        let j = serde_json::to_string(&ww).unwrap();
        assert!(j.contains(r#""type":"wechat-work""#));
    }

    #[test]
    fn model_override_value_untagged() {
        let s = ModelOverrideValue::Str("gpt-4".into());
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#""gpt-4""#);
        let o = ModelOverrideValue::Override(AgentLLMOverride { model: "x".into(), provider: None, base_url: None, api_key_env: None, stream: None });
        let j2 = serde_json::to_string(&o).unwrap();
        assert!(j2.contains(r#""model":"x""#));
    }

    #[test]
    fn project_config_minimal_deserialize() {
        // 仅必填字段，其余走 default
        let json = r#"{"name":"demo","version":"0.1.0","llm":{"provider":"custom","baseUrl":"http://x","apiKey":"","model":"m","cover":null},"researchSearch":{"enabled":false,"provider":"tavily"}}"#;
        let p: ProjectConfig = serde_json::from_str(json).unwrap();
        assert_eq!(p.name, "demo");
        assert_eq!(p.language, "zh"); // default
        assert_eq!(p.input_governance_mode, InputGovernanceMode::V2); // default
        assert_eq!(p.daemon.max_concurrent_books, 3); // default
        assert!(p.notify.is_empty());
    }
}
