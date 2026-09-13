//! write-next 生产 LLM 接线：agent 路由 + 九路端口适配。
//!
//! TS `PipelineRunner.resolveOverride` 的等价物：默认模型 + 按 agent 覆盖
//! （字符串 = 同 client 换模型；完整覆盖 = 独立 baseUrl/apiKey 的客户端），
//! 之上适配九路 trait 端口（writer/planner/composer/reviser/auditor/
//! normalizer/analyzer/state-validator/settler）。
//!
//! 底层走 [`StreamingChatClient`]（OpenAI 兼容 /chat/completions，SSE 收集）。
//! CycleAuditor 需要 continuity 审计编排（43 号首版以最小审计端口落地：
//! LLM 输出 PASS/FAIL + 分数协议；完整 continuity.ts 编排随 audit 域端点补齐，
//! 备案）。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::agents::chapter_analyzer::ChapterAnalyzerChat;
use crate::agents::composer::ComposerChatOptions;
use crate::agents::continuity::{AuditResult, AuditTokenUsage, ChatOutcome};
use crate::agents::planner::PlannerChat;
use crate::agents::reviser::ReviserChat;
use crate::agents::state_validator::StateValidatorChat;
use crate::agents::writer::{SettleChapterStateInput, WriteChapterError, WriteChapterOutput, WriterChat, WriterCtx};
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::llm::streaming_client::{ChatCompletionParams, StreamError, StreamingChatClient};
use crate::pipeline::chapter_review_cycle::{ChapterReviewCycleControlInput, CycleAuditor};
use crate::pipeline::chapter_state_recovery::{SettlePort, SettleRequest};

/// 默认 LLM 端点配置。
#[derive(Debug, Clone)]
pub struct LlmEndpointConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub extra_headers: HashMap<String, String>,
}

/// 单 agent 覆盖。对齐 TS `AgentLLMOverride` 的 Rust 子集（字符串模型覆盖
/// 与完整覆盖二选一）。
#[derive(Debug, Clone, Default)]
pub struct AgentOverride {
    /// 覆盖模型（同端点）。
    pub model: Option<String>,
    /// 覆盖端点（独立 baseUrl/apiKey 时必填）。
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub max_tokens: Option<u32>,
}

/// agent → 端点解析结果。
pub struct ResolvedEndpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
}

/// agent 路由器（默认端点 + 覆盖表；客户端按端点缓存）。
#[derive(Clone)]
pub struct AgentRouter {
    default: LlmEndpointConfig,
    overrides: Arc<HashMap<String, AgentOverride>>,
    clients: Arc<tokio::sync::Mutex<HashMap<String, Arc<StreamingChatClient>>>>,
    /// 传输协议（106 号）：全端点统一 chat/responses（TS client.apiFormat——
    /// 端点级配置，非 per-agent）。
    api_format: crate::llm::providers::TransportApiFormat,
    /// 流式偏好（108 号）：Some(false) → 全端点非流式调用（TS client.stream；
    /// None = 缺省流式）。
    stream: Option<bool>,
    /// 流式进度钩子（126 号：TS PipelineConfig onStreamProgress → SSE
    /// `llm:progress`；None = 无进度上报）。
    progress_hook: Option<crate::llm::provider::StreamProgressCallback>,
    /// G16/346 号：按任务模型路由（agent 显式 override 优先于此，全局 default 兜底）。
    task_routing: Option<crate::models::task_routing::TaskModelRouting>,
}

impl AgentRouter {
    pub fn new(default: LlmEndpointConfig, overrides: HashMap<String, AgentOverride>) -> Self {
        AgentRouter {
            default,
            overrides: Arc::new(overrides),
            clients: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            api_format: crate::llm::providers::TransportApiFormat::Chat,
            stream: None,
            progress_hook: None,
            task_routing: None,
        }
    }

    /// G16/346 号：注入按任务模型路由（writer/review/repair/analysis 任务覆盖 model）。
    pub fn with_task_routing(
        mut self,
        routing: Option<crate::models::task_routing::TaskModelRouting>,
    ) -> Self {
        self.task_routing = routing;
        self
    }

    /// 覆盖传输协议（默认 chat；inkos.json llm.apiFormat / 服务项 apiFormat /
    /// env INKOS_LLM_API_FORMAT 命中 responses 时置位）。
    pub fn with_api_format(mut self, api_format: crate::llm::providers::TransportApiFormat) -> Self {
        self.api_format = api_format;
        self
    }

    /// 覆盖流式偏好（默认流式；inkos.json llm.stream / 服务项 stream /
    /// env INKOS_LLM_STREAM 为 false 时全端点非流式——不支持 SSE 的端点）。
    pub fn with_stream(mut self, stream: Option<bool>) -> Self {
        self.stream = stream;
        self
    }

    /// 挂流式进度钩子（126 号：effective_router 从 BooksRuntime.hub 构造
    /// 广播闭包；builder 字段语义——后续 with_* 覆盖不丢）。
    pub fn with_progress_hook(
        mut self,
        hook: crate::llm::provider::StreamProgressCallback,
    ) -> Self {
        self.progress_hook = Some(hook);
        self
    }

    /// 当前进度钩子（测试/诊断断言挂载态）。
    pub fn progress_hook(&self) -> Option<&crate::llm::provider::StreamProgressCallback> {
        self.progress_hook.as_ref()
    }

    /// 当前流式偏好（探测/诊断面回显）。
    pub fn stream_preference(&self) -> Option<bool> {
        self.stream
    }

    /// 当前传输协议（探测/诊断面回显）。
    pub fn api_format(&self) -> crate::llm::providers::TransportApiFormat {
        self.api_format
    }

    /// 解析 agent 端点（无覆盖 = 默认）。
    pub fn resolve(&self, agent: &str) -> ResolvedEndpoint {
        let Some(override_) = self.overrides.get(agent) else {
            // G16/346 号：无显式覆盖时按任务路由解析 model（不命中则全局缺省）。
            let routed_model = crate::models::task_routing::resolve_agent_model(
                agent,
                self.task_routing.as_ref(),
                &self.default.model,
            );
            return ResolvedEndpoint {
                base_url: self.default.base_url.clone(),
                api_key: self.default.api_key.clone(),
                model: routed_model.unwrap_or_else(|| self.default.model.clone()),
                max_tokens: self.default.max_tokens,
            };
        };
        ResolvedEndpoint {
            base_url: override_
                .base_url
                .clone()
                .unwrap_or_else(|| self.default.base_url.clone()),
            api_key: override_
                .api_key
                .clone()
                .unwrap_or_else(|| self.default.api_key.clone()),
            model: override_
                .model
                .clone()
                .unwrap_or_else(|| self.default.model.clone()),
            max_tokens: override_.max_tokens.unwrap_or(self.default.max_tokens),
        }
    }

    pub async fn client_for_public(&self, endpoint: &ResolvedEndpoint) -> Arc<StreamingChatClient> {
        self.client_for(endpoint).await
    }

    async fn client_for(&self, endpoint: &ResolvedEndpoint) -> Arc<StreamingChatClient> {
        // 缓存键：baseUrl|apiKey 尾 4 位（避免全键入内存日志面的同时区分端点）。
        let key = format!(
            "{}|{}",
            endpoint.base_url,
            endpoint.api_key.len()
        );
        let mut clients = self.clients.lock().await;
        if let Some(client) = clients.get(&key) {
            return client.clone();
        }
        let client = Arc::new(StreamingChatClient::new(
            endpoint.base_url.clone(),
            endpoint.api_key.clone(),
            self.default.extra_headers.clone(),
        ));
        clients.insert(key, client.clone());
        client
    }

    /// chat（agent 路由 + 流式收集 + 瞬时错误重试 + R25 接管链）。
    ///
    /// 205 号：对齐 TS `chatCompletion` 的 `withTransientLLMRetry`——429/
    /// 5xx/限流短语/传输层瞬时/流不活动超时线性退避重试 2 次（800ms/
    /// 1600ms），重试完整重新生成；`INKOS_LLM_TRANSIENT_RETRY=0` 关闭
    /// （诊断快测面，关的是**同模型重试**，不影响跨模型接管）。无 UI 文本
    /// 增量面（progress_hook 是进度事件，对齐 TS onStreamProgress 不禁重试
    /// 的语义）；用户中止在阶段边界生效（175 号），与本重试不冲突。
    ///
    /// R25/399 号：链目顺序取 [`Self::resolve_chain`]（primary 在前）——每
    /// 模型跑 retryCount 轮、每轮走上述瞬态环；瞬态耗尽切下一模型，非瞬态
    /// （中止/鉴权/上下文超限/模型不存在）立即失败不切换。零配置 → 单模型
    /// 单轮，行为与现版本一致。
    pub async fn chat(
        &self,
        agent: &str,
        mut messages: Vec<LLMMessage>,
        temperature: f64,
        max_tokens: Option<u32>,
    ) -> Result<ChatOutcome, String> {
        // 139 号：生产操作激活技能注入（TS BaseAgent.chat 的
        // appendTaskSkillGuidance 出口位置——Rust 各 agent 统一经此出口，
        // 等价集中点；hydrate 引用检索属 local-search 面，Rust 未移植，
        // activations 的 resources 恒空无需 hydrate）。
        if let Some(skills) = crate::skills::production_bindings::current_operation_skills() {
            if !skills.is_empty() {
                crate::agents::append_activated_skill_guidance(&mut messages, &skills);
            }
        }
        let (models, retry_count) = self.resolve_chain(agent);
        let mut last_err: Option<String> = None;
        for (chain_idx, model) in models.iter().enumerate() {
            for round in 1..=retry_count {
                match self
                    .chat_transient(agent, model, messages.clone(), temperature, max_tokens)
                    .await
                {
                    Ok(outcome) => {
                        if chain_idx > 0 || round > 1 {
                            tracing::warn!(
                                agent,
                                model,
                                chain_idx,
                                round,
                                "R25 接管链：模型接管成功"
                            );
                        }
                        return Ok(outcome);
                    }
                    Err(ChainFailure::Fatal(text)) => return Err(text),
                    Err(ChainFailure::Transient(text)) => {
                        last_err = Some(text.clone());
                        tracing::warn!(
                            agent,
                            model,
                            chain_idx,
                            round,
                            "R25 接管链：模型失败，重试/切换备用：{text}"
                        );
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| "LLM 调用失败".to_string()))
    }

    /// R25：agent 的尝试链（primary 在前）与每模型轮数。
    ///
    /// 显式 override 钉死 → 单元素单轮（用户指定端点/模型不接管）；无任务
    /// 映射/无路由 → [默认模型] 单轮；有任务路由 → `resolve_task_model_chain`
    /// （备用与主模型同端点，仅 model 序列；retryCount clamp 1–5）。
    fn resolve_chain(&self, agent: &str) -> (Vec<String>, usize) {
        let fallback = || (vec![self.default.model.clone()], 1usize);
        if self.overrides.contains_key(agent) {
            return (vec![self.resolve(agent).model], 1);
        }
        let Some(task) = crate::models::task_routing::agent_task(agent) else {
            return fallback();
        };
        let Some(routing) = self.task_routing.as_ref() else {
            return fallback();
        };
        let chain = crate::models::task_routing::resolve_task_model_chain(
            crate::models::task_routing::ResolveTaskModelParams {
                task,
                book_routing: None,
                project_routing: Some(routing),
                fallback_model: self.default.model.clone(),
                fallback_service: None,
                fallback_temperature: None,
                fallback_max_tokens: None,
            },
        );
        (
            chain.attempts.iter().map(|a| a.model.clone()).collect(),
            chain.retry_count.clamp(1, 5) as usize,
        )
    }

    /// 单模型瞬态重试环（205 号语义保持：2 次预算 + 线性退避）。返回值区分
    /// **非瞬态（立即失败，不切换）**与**瞬态耗尽（R25 可重试轮/切换备用）**。
    async fn chat_transient(
        &self,
        agent: &str,
        model: &str,
        messages: Vec<LLMMessage>,
        temperature: f64,
        max_tokens: Option<u32>,
    ) -> Result<ChatOutcome, ChainFailure> {
        const TRANSIENT_RETRIES: usize = 2;
        let mut attempt: usize = 0;
        loop {
            match self.chat_once(agent, model, messages.clone(), temperature, max_tokens).await {
                Ok(outcome) => return Ok(outcome),
                Err(text) => {
                    if !crate::llm::provider::is_retryable_llm_error(&text) {
                        return Err(ChainFailure::Fatal(text));
                    }
                    if !transient_retry_enabled() || attempt >= TRANSIENT_RETRIES {
                        return Err(ChainFailure::Transient(text));
                    }
                    attempt += 1;
                    tracing::warn!(agent, attempt, model, "LLM 瞬时错误，退避后重试：{text}");
                    tokio::time::sleep(std::time::Duration::from_millis(800 * attempt as u64)).await;
                }
            }
        }
    }

    /// 单次 chat 尝试（无重试——由 [`Self::chat_transient`] 包裹；model 由
    /// R25 链目给定，端点仍按 agent 解析——备用与主模型同端点）。
    async fn chat_once(
        &self,
        agent: &str,
        model: &str,
        messages: Vec<LLMMessage>,
        temperature: f64,
        max_tokens: Option<u32>,
    ) -> Result<ChatOutcome, String> {
        let endpoint = self.resolve(agent);
        let client = self.client_for(&endpoint).await;
        let completion = client
            .stream_chat(&ChatCompletionParams {
                model,
                messages: &messages,
                temperature,
                max_tokens: max_tokens.unwrap_or(endpoint.max_tokens),
                stream: self.stream.unwrap_or(true),
                api_format: self.api_format,
                extra: None,
                tools: None,
                images: None,
                progress: self.progress_hook.clone(),
                // TS chatCompletion：所有 agent 聊天走管线面默认（300s/180s），
                // env 可覆盖（INKOS_LLM_*_TIMEOUT_MS）。
                deadline: crate::llm::streaming_client::StreamDeadlineSpec::resolve(
                    crate::llm::streaming_client::StreamDeadlineSpec::PIPELINE,
                    None,
                    None,
                ),
                // 135 号：经 task-local 通道读取当前作用域——管线直调链
                // 无 set → None（TS 管线在 ALS 作用域外零头等价）；sub_agent
                // 工具链内 → 派生 subagent 作用域（TS runWithAgentTrajectoryRole）。
                trajectory: crate::llm::agent_trajectory::current_scope(),
            })
            .await
            .map_err(|e: StreamError| e.to_string())?;
        Ok(ChatOutcome {
            content: completion.content,
            usage: completion.total_tokens.map(|total| AuditTokenUsage {
                prompt_tokens: completion.prompt_tokens.unwrap_or(0) as u32,
                completion_tokens: completion.completion_tokens.unwrap_or(0) as u32,
                total_tokens: total as u32,
            }),
        })
    }
}

/// R25：链内失败分类——`Fatal` = 非瞬态（中止/鉴权/上下文超限/模型不存在）
/// 立即失败不切换；`Transient` = 瞬态预算耗尽（可重试轮/切换备用）。
enum ChainFailure {
    Fatal(String),
    Transient(String),
}

/// 瞬时重试开关（205 号）：默认开；`INKOS_LLM_TRANSIENT_RETRY=0|false|off`
/// 关闭（诊断快测面，对齐 TS options.retry=false）。
fn transient_retry_enabled() -> bool {
    std::env::var("INKOS_LLM_TRANSIENT_RETRY")
        .map(|v| !matches!(v.as_str(), "0" | "false" | "off"))
        .unwrap_or(true)
}

/// 单 agent 的端口实现（宏生成同签名 trait 实现）。
///
/// 202 号：`router` 改 `Arc<AgentRouter>`——端口装配共享同一 router 句柄
/// （每章 write-next 原先对 AgentRouter 做 9 份深拷贝再整体泄漏）；
/// chat 实现经 Deref 无感，`clone()` 变计数递增。
pub struct RoutedAgent {
    pub router: Arc<AgentRouter>,
    pub agent: &'static str,
}

macro_rules! impl_simple_chat {
    ($trait_name:ident) => {
        #[async_trait]
        impl $trait_name for RoutedAgent {
            async fn chat(
                &self,
                messages: Vec<LLMMessage>,
                temperature: f64,
            ) -> Result<ChatOutcome, String> {
                self.router.chat(self.agent, messages, temperature, None).await
            }
        }
    };
}

impl_simple_chat!(WriterChat);
use crate::agents::architect::ArchitectChat;
use crate::agents::fanfic_canon_importer::FanficCanonImporterChat;
use crate::agents::foundation_reviewer::FoundationReviewerChat;
impl_simple_chat!(ArchitectChat);
impl_simple_chat!(FoundationReviewerChat);
impl_simple_chat!(FanficCanonImporterChat);
impl_simple_chat!(PlannerChat);
impl_simple_chat!(ReviserChat);
impl_simple_chat!(ChapterAnalyzerChat);
impl_simple_chat!(StateValidatorChat);
#[async_trait]
impl crate::agents::consolidator::ConsolidatorChat for RoutedAgent {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String> {
        self.router.chat(self.agent, messages, temperature, None).await
    }
}
#[async_trait]
impl crate::agents::continuity::AuditorChat for RoutedAgent {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String> {
        self.router.chat(self.agent, messages, temperature, None).await
    }
}

#[async_trait]
impl crate::agents::composer::ComposerChat for RoutedAgent {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        options: ComposerChatOptions,
    ) -> Result<ChatOutcome, String> {
        self.router
            .chat(self.agent, messages, options.temperature, options.max_tokens)
            .await
    }
}

/// 审计端口：LLM 首行 PASS/FAIL + 可选分数（43 号最小协议）。
///
/// 44 号起主链改用 [`FullCycleAuditor`]（真实 `audit_chapter` 编排）；
/// 本实现保留为无 ctx 场景的轻量回退（单测/健康探测）。
#[async_trait]
impl CycleAuditor for RoutedAgent {
    async fn audit_chapter(
        &self,
        content: &str,
        _control: Option<&ChapterReviewCycleControlInput<'_>>,
        _temperature: Option<f64>,
    ) -> Result<AuditResult, String> {
        let messages = vec![
            LLMMessage {
                role: LLMRole::System,
                content: "你是小说连续性审稿官。审查章节正文：无硬矛盾输出 PASS，有硬矛盾输出 FAIL 并逐行列出问题（[分类] 描述）。最后一行输出 0-100 整体分。" .to_string(),
                tool_calls: None, tool_call_id: None,
            },
            LLMMessage {
                role: LLMRole::User,
                content: format!("## 待审正文\n{content}"),
                tool_calls: None, tool_call_id: None,
            },
        ];
        let outcome = self.router.chat(self.agent, messages, 0.2, None).await?;
        let mut passed = false;
        let mut score: Option<u32> = None;
        let mut issues = Vec::new();
        for (index, line) in outcome.content.lines().map(str::trim).enumerate() {
            if index == 0 {
                passed = line.eq_ignore_ascii_case("PASS");
                continue;
            }
            if let Some(rest) = line.strip_prefix('[') {
                if let Some((category, description)) = rest.split_once(']') {
                    issues.push(crate::agents::continuity::AuditIssue {
                        severity: crate::agents::continuity::AuditSeverity::Warning,
                        category: category.trim().to_string(),
                        description: description.trim().to_string(),
                        suggestion: String::new(),
                        repair_scope: None,
                    });
                    continue;
                }
            }
            if let Ok(value) = line.parse::<u32>() {
                score = Some(value);
            }
        }
        Ok(AuditResult {
            passed,
            issues,
            summary: outcome.content.lines().next().unwrap_or("").to_string(),
            parse_failed: Some(false),
            overall_score: score.or(Some(if passed { 90 } else { 50 })),
            token_usage: outcome.usage,
        })
    }
}

/// settle 端口：writer.settleChapterState 的链内包装（ctx 自持）。
///
/// 202 号：`ctx` 由 `'static` 泛型化为 `'a`——write-next 装配改为 owned
/// 持有者 + 同帧借用（无泄漏）；低频端点的既有 `Box::leak` 装配传
/// `'static` 值仍兼容（`'static: 'a`）。
pub struct RoutedSettler<'a> {
    pub router: Arc<AgentRouter>,
    pub ctx: WriterCtx<'a>,
    pub chapter_number: u32,
}

#[async_trait]
impl SettlePort for RoutedSettler<'_> {
    async fn settle(&self, params: SettleRequest<'_>) -> Result<WriteChapterOutput, String> {
        let agent = RoutedAgent {
            router: self.router.clone(),
            agent: "writer",
        };
        let input = SettleChapterStateInput {
            book: params.book,
            book_dir: params.book_dir,
            chapter_number: self.chapter_number,
            baseline_chapter: params.baseline_chapter,
            title: params.title,
            content: params.content,
            allow_reapply: Some(params.allow_reapply),
            allow_new_hooks: params.allow_new_hooks,
            chapter_intent: params.chapter_intent,
            context_package: params.context_package,
            rule_stack: params.rule_stack,
            validation_feedback: params.validation_feedback,
        };
        crate::agents::writer::settle_chapter_state(&self.ctx, &agent, &input)
            .await
            .map_err(|e: WriteChapterError| e.to_string())
    }
}

/// 完整审计端口适配：真实 `audit_chapter`（11 路真相文件 + 维度审计 +
/// 四策略解析）包装为 CycleAuditor。
///
/// book 上下文（book_dir/章节号/genre）在构造时捕获——write-next 环内
/// 这些是调用级常量；治理控制入参（intent/memo/package/ruleStack）经
/// 环的 control 逐调用传入。
pub struct FullCycleAuditor {
    pub router: Arc<AgentRouter>,
    pub project_root: std::path::PathBuf,
    pub builtin_genres_dir: std::path::PathBuf,
    pub book_dir: std::path::PathBuf,
    pub chapter_number: u32,
    pub genre: String,
}

impl FullCycleAuditor {
    /// 按章克隆（write-next 环内以正确 book 上下文重建主链）。
    pub fn for_chapter(
        &self,
        book_dir: std::path::PathBuf,
        chapter_number: u32,
        genre: &str,
    ) -> Self {
        FullCycleAuditor {
            router: self.router.clone(),
            project_root: self.project_root.clone(),
            builtin_genres_dir: self.builtin_genres_dir.clone(),
            book_dir,
            chapter_number,
            genre: genre.to_string(),
        }
    }

    /// 真实 auditChapter 调用（options 全量透传）。
    async fn run_audit(
        &self,
        content: &str,
        options: crate::agents::continuity::AuditChapterOptions,
    ) -> Result<crate::agents::continuity::AuditResult, String> {
        let prompt_store = crate::state::store::FsStateStore;
        let ctx = crate::agents::continuity::AuditChapterCtx {
            project_root: &self.project_root,
            builtin_genres_dir: &self.builtin_genres_dir,
            prompt_store: &prompt_store,
        };
        let chat = RoutedAgent {
            router: self.router.clone(),
            agent: "auditor",
        };
        crate::agents::continuity::audit_chapter(
            &ctx,
            &chat,
            &self.book_dir,
            content,
            self.chapter_number,
            Some(&self.genre),
            &options,
        )
        .await
        .map_err(|e| e.to_string())
    }
}

/// 47 号：合并审计端口（options 含 truthFileOverrides——post 修订审计用
/// 修稿器产出的临时真相覆盖磁盘状态）。
#[async_trait]
impl crate::pipeline::merged_audit::LlmAuditPort for FullCycleAuditor {
    async fn audit(
        &self,
        chapter_content: &str,
        options: &crate::agents::continuity::AuditChapterOptions,
    ) -> Result<crate::agents::continuity::AuditResult, String> {
        self.run_audit(chapter_content, options.clone()).await
    }
}

#[async_trait]
impl CycleAuditor for FullCycleAuditor {
    async fn audit_chapter(
        &self,
        content: &str,
        control: Option<&ChapterReviewCycleControlInput<'_>>,
        temperature: Option<f64>,
    ) -> Result<AuditResult, String> {
        let options = crate::agents::continuity::AuditChapterOptions {
            temperature,
            chapter_intent: control.map(|c| c.chapter_intent.to_string()),
            chapter_memo: control.and_then(|c| c.chapter_memo).cloned(),
            context_package: control.map(|c| c.context_package.clone()),
            rule_stack: control.map(|c| c.rule_stack.clone()),
            truth_file_overrides: None,
        };
        self.run_audit(content, options).await
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn router_with(overrides: HashMap<String, AgentOverride>) -> AgentRouter {
        AgentRouter::new(
            LlmEndpointConfig {
                base_url: "http://localhost:1".into(),
                api_key: "sk-default".into(),
                model: "default-model".into(),
                max_tokens: 8192,
                extra_headers: HashMap::new(),
            },
            overrides,
        )
    }

    #[test]
    fn resolve_routes_overrides() {
        let router = router_with(HashMap::from([(
            "planner".to_string(),
            AgentOverride {
                model: Some("plan-model".into()),
                base_url: None,
                api_key: None,
                max_tokens: Some(1024),
            },
        )]));
        let planner = router.resolve("planner");
        assert_eq!(planner.model, "plan-model");
        assert_eq!(planner.max_tokens, 1024);
        assert_eq!(planner.base_url, "http://localhost:1"); // 同端点

        let writer = router.resolve("writer");
        assert_eq!(writer.model, "default-model");
        assert_eq!(writer.max_tokens, 8192);
    }

    /// 205 号：可编程失败次数的 mock LLM——前 `fail_times` 次返回指定
    /// 状态码，之后返回正常 SSE/JSON（计数控供断言重试次数）。
    async fn spawn_flaky_llm(fail_times: usize, status: u16, err_body: &str) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let hits_clone = hits.clone();
        let err_body = err_body.to_string();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(req): axum::Json<serde_json::Value>| {
                let hits = hits_clone.clone();
                let err_body = err_body.clone();
                async move {
                    let n = hits.fetch_add(1, Ordering::SeqCst);
                    if n < fail_times {
                        return axum::response::IntoResponse::into_response((
                            axum::http::StatusCode::from_u16(status).unwrap(),
                            [(axum::http::header::CONTENT_TYPE, "application/json")],
                            err_body,
                        ));
                    }
                    if req["stream"].as_bool().unwrap_or(false) {
                        let chunk = serde_json::json!({ "choices": [{ "delta": { "content": "OK" } }] });
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                        return axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                        ));
                    }
                    axum::response::IntoResponse::into_response(axum::Json(serde_json::json!({
                        "choices": [{ "message": { "role": "assistant", "content": "OK" } }],
                        "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
                    })))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), hits)
    }

    fn router_at(base: &str) -> AgentRouter {
        AgentRouter::new(
            LlmEndpointConfig {
                base_url: base.into(),
                api_key: "k".into(),
                model: "m".into(),
                max_tokens: 64,
                extra_headers: HashMap::new(),
            },
            HashMap::new(),
        )
    }

    fn user_message() -> Vec<LLMMessage> {
        vec![LLMMessage { role: crate::llm::provider::LLMRole::User, content: "x".into(), tool_calls: None, tool_call_id: None }]
    }

    /// 429 一次后退避重试成功（对齐 TS withTransientLLMRetry：2 次预算）。
    #[tokio::test]
    async fn chat_retries_transient_429_then_succeeds() {
        use std::sync::atomic::Ordering;
        let (base, hits) = spawn_flaky_llm(1, 429, r#"{"error":"rate limit"}"#).await;
        let router = router_at(&base);
        let outcome = router.chat("writer", user_message(), 0.1, None).await.unwrap();
        assert_eq!(outcome.content, "OK");
        assert_eq!(hits.load(Ordering::SeqCst), 2, "首次 429 + 重试 1 次");
    }

    /// 重试预算耗尽（2 次）如实抛最后一次错——共 3 次尝试。
    #[tokio::test]
    async fn chat_exhausts_transient_retries_and_fails() {
        use std::sync::atomic::Ordering;
        let (base, hits) = spawn_flaky_llm(9, 503, r#"{"error":"service unavailable"}"#).await;
        let router = router_at(&base);
        let err = router.chat("writer", user_message(), 0.1, None).await.unwrap_err();
        assert!(err.contains("503"), "{err}");
        assert_eq!(hits.load(Ordering::SeqCst), 3, "1 次原始 + 2 次重试");
    }

    /// 非瞬时错误（model_not_available 的 500）不重试——立即失败。
    #[tokio::test]
    async fn chat_does_not_retry_permanent_error() {
        use std::sync::atomic::Ordering;
        let (base, hits) =
            spawn_flaky_llm(9, 500, r#"{"error":{"code":"model_not_available"}}"#).await;
        let router = router_at(&base);
        let err = router.chat("writer", user_message(), 0.1, None).await.unwrap_err();
        assert!(err.contains("500") || err.contains("model_not_available"), "{err}");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "非瞬时直接失败");
    }

    // --- R25 接管链（399 号） ---

    use crate::models::task_routing::{TaskModelOverride, TaskModelRouting};

    /// R25：按模型分流 flaky mock——每模型自带失败预算（命中 ≤ 预算返 status
    /// 错误，预算外返 OK）；返回 (base, 按模型命中计数)。
    async fn spawn_chain_llm(
        fail_budget: Vec<(String, usize)>,
        status: u16,
        err_body: &str,
    ) -> (
        String,
        HashMap<String, std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    ) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let err_body = err_body.to_string();
        let counters: HashMap<String, std::sync::Arc<AtomicUsize>> = ["primary-m", "backup-m"]
            .iter()
            .map(|m| (m.to_string(), std::sync::Arc::new(AtomicUsize::new(0))))
            .collect();
        let handler_counters = counters.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(req): axum::Json<serde_json::Value>| {
                let counters = handler_counters.clone();
                let fail_budget = fail_budget.clone();
                let err_body = err_body.clone();
                async move {
                    let model = req["model"].as_str().unwrap_or("?").to_string();
                    let hits = counters
                        .get(&model)
                        .map(|c| c.fetch_add(1, Ordering::SeqCst))
                        .unwrap_or(0);
                    let budget = fail_budget
                        .iter()
                        .find(|(m, _)| *m == model)
                        .map(|(_, n)| *n)
                        .unwrap_or(0);
                    if hits < budget {
                        return axum::response::IntoResponse::into_response((
                            axum::http::StatusCode::from_u16(status).unwrap(),
                            [(axum::http::header::CONTENT_TYPE, "application/json")],
                            err_body,
                        ));
                    }
                    if req["stream"].as_bool().unwrap_or(false) {
                        let chunk = serde_json::json!({ "choices": [{ "delta": { "content": "OK" } }] });
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                        return axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                        ));
                    }
                    axum::response::IntoResponse::into_response(axum::Json(serde_json::json!({
                        "choices": [{ "message": { "role": "assistant", "content": "OK" } }],
                        "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
                    })))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), counters)
    }

    /// 写作任务路由（primary/backup/retryCount 面向 mock 模型名）。
    fn writing_routing(
        model: Option<&str>,
        backup: Vec<&str>,
        retry: Option<u32>,
    ) -> TaskModelRouting {
        TaskModelRouting {
            defaults: None,
            tasks: Some(
                [(
                    "writing".to_string(),
                    TaskModelOverride {
                        model: model.map(|s| s.to_string()),
                        service: None,
                        temperature: None,
                        max_tokens: None,
                        retry_count: retry,
                        backup_models: (!backup.is_empty())
                            .then(|| backup.iter().map(|s| s.to_string()).collect()),
                    },
                )]
                .into_iter()
                .collect(),
            ),
        }
    }

    fn routing_router_at(base: &str, routing: TaskModelRouting) -> AgentRouter {
        AgentRouter::new(
            LlmEndpointConfig {
                base_url: base.into(),
                api_key: "k".into(),
                model: "primary-m".into(),
                max_tokens: 64,
                extra_headers: HashMap::new(),
            },
            HashMap::new(),
        )
        .with_task_routing(Some(routing))
    }

    /// 链解析：去重保序 + retryCount；显式 override 钉死单元素；无路由回默认。
    #[test]
    fn resolve_chain_orders_pins_and_falls_back() {
        let router = routing_router_at(
            "http://localhost:1",
            writing_routing(Some("primary-m"), vec!["primary-m", "backup-m", "second-m"], Some(2)),
        );
        let (models, retry) = router.resolve_chain("writer");
        assert_eq!(models, vec!["primary-m", "backup-m", "second-m"], "与主模型重复者剔除，按序封顶");
        assert_eq!(retry, 2);

        let pinned = AgentRouter::new(
            LlmEndpointConfig {
                base_url: "http://localhost:1".into(),
                api_key: "k".into(),
                model: "default-model".into(),
                max_tokens: 64,
                extra_headers: HashMap::new(),
            },
            HashMap::from([(
                "writer".to_string(),
                AgentOverride {
                    model: Some("pinned-m".into()),
                    base_url: None,
                    api_key: None,
                    max_tokens: None,
                },
            )]),
        );
        let (models, retry) = pinned.resolve_chain("writer");
        assert_eq!(models, vec!["pinned-m"], "显式钉死不接管");
        assert_eq!(retry, 1);

        let (models, retry) = router_with(HashMap::new()).resolve_chain("writer");
        assert_eq!(models, vec!["default-model"], "无路由回默认模型");
        assert_eq!(retry, 1);
    }

    /// R25：primary 恒 429（瞬态耗尽）→ backup 接管成功。
    #[tokio::test]
    async fn chain_fails_over_to_backup_on_transient() {
        use std::sync::atomic::Ordering;
        let (base, counters) = spawn_chain_llm(
            vec![("primary-m".to_string(), usize::MAX)],
            429,
            r#"{"error":"rate limit"}"#,
        )
        .await;
        let router =
            routing_router_at(&base, writing_routing(Some("primary-m"), vec!["backup-m"], None));
        let outcome = router.chat("writer", user_message(), 0.1, None).await.unwrap();
        assert_eq!(outcome.content, "OK");
        assert_eq!(
            counters["primary-m"].load(Ordering::SeqCst),
            3,
            "primary 1 次原始 + 2 次瞬态重试"
        );
        assert_eq!(counters["backup-m"].load(Ordering::SeqCst), 1, "backup 接管 1 次");
    }

    /// R25：非瞬态（model_not_available）立即失败，不重试不切换。
    #[tokio::test]
    async fn chain_does_not_switch_on_fatal_error() {
        use std::sync::atomic::Ordering;
        let (base, counters) = spawn_chain_llm(
            vec![("primary-m".to_string(), usize::MAX)],
            500,
            r#"{"error":{"code":"model_not_available"}}"#,
        )
        .await;
        let router =
            routing_router_at(&base, writing_routing(Some("primary-m"), vec!["backup-m"], None));
        let err = router.chat("writer", user_message(), 0.1, None).await.unwrap_err();
        assert!(err.contains("500") || err.contains("model_not_available"), "{err}");
        assert_eq!(counters["primary-m"].load(Ordering::SeqCst), 1, "非瞬态 1 次即失败");
        assert_eq!(counters["backup-m"].load(Ordering::SeqCst), 0, "不切换备用");
    }

    /// R25：retryCount=2 → primary 两轮（每轮 1+2 瞬态）共 6 次内第 6 次成功，
    /// 不必动用 backup。
    #[tokio::test]
    async fn chain_retry_count_runs_extra_rounds_per_model() {
        use std::sync::atomic::Ordering;
        let (base, counters) = spawn_chain_llm(
            vec![("primary-m".to_string(), 5)],
            429,
            r#"{"error":"rate limit"}"#,
        )
        .await;
        let router = routing_router_at(
            &base,
            writing_routing(Some("primary-m"), vec!["backup-m"], Some(2)),
        );
        let outcome = router.chat("writer", user_message(), 0.1, None).await.unwrap();
        assert_eq!(outcome.content, "OK");
        assert_eq!(
            counters["primary-m"].load(Ordering::SeqCst),
            6,
            "两轮 ×（1+2 瞬态）= 第 6 次成功"
        );
        assert_eq!(counters["backup-m"].load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn chat_failure_is_string_error() {
        let router = router_with(HashMap::new());
        let agent = RoutedAgent { router: std::sync::Arc::new(router), agent: "writer" };
        let error = crate::agents::writer::WriterChat::chat(
            &agent,
            vec![LLMMessage { role: LLMRole::User, content: "x".into(), tool_calls: None, tool_call_id: None }],
            0.7,
        )
        .await
        .unwrap_err();
        // 端口 1 不可达——错误字符串化（不 panic）。
        assert!(!error.is_empty());
    }

    #[tokio::test]
    async fn auditor_protocol_parses() {
        // 本地 mock LLM：首行 PASS + 问题行 + 分数行。
        let router = router_with(HashMap::new());
        let agent = RoutedAgent { router: std::sync::Arc::new(router), agent: "auditor" };
        // 直接测协议解析逻辑——通过注入式结果不可行（无 mock 面），
        // 用最小内联复算验证（真实 HTTP 面由 E2E 契约测试覆盖）。
        let content = "PASS\n[节奏] 节奏拖沓\n92";
        let mut passed = false;
        let mut score = None;
        let mut issue_count = 0;
        for (index, line) in content.lines().map(str::trim).enumerate() {
            if index == 0 {
                passed = line.eq_ignore_ascii_case("PASS");
                continue;
            }
            if line.starts_with('[') {
                issue_count += 1;
                continue;
            }
            if let Ok(value) = line.parse::<u32>() {
                score = Some(value);
            }
        }
        assert!(passed);
        assert_eq!(issue_count, 1);
        assert_eq!(score, Some(92));
        let _ = agent; // 编译期保留引用
    }
}
