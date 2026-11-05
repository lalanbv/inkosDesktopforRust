//! 短篇生产 runner 编排层（892 号 M3d）：`packages/core/src/pipeline/short-fiction-runner.ts`
//! 忠实移植——
//! - 入口 + 每作品 tokio 书锁（L194-225；`withWorkMutationScope` 的 Rust 等价
//!   = adopt.rs 同源 per-work tokio Mutex）
//! - `loadCompletedShortRun` 幂等重放（L227-306：七工件 + inputHash/requestHash/
//!   reviewHash 核对 + 未 persisted 历史回退观察）
//! - `produceShort` 四阶段编排（L349-723：outline→draft→review→package + 封面
//!   门面）与 `runShortFictionStage` 单阶段入口（L309-320）
//! - stage acceptPaths 族 + `refreshDelivery` 交付合同观察
//! - 产物落盘（writeDraftArtifacts/writeFinalArtifacts/writePackageArtifacts
//!   L1103-1200：标题别名 + 章节文件淘汰 + 原子集提交）
//! - 失败恢复（候选记录 + recovery payload L322-332）
//!
//! 备案（886 处置表 + 893 现状）：
//! - 封面 2.0 生成本体已接线（893：2.0 `buildCoverImagePrompt` + reference
//!   编辑路由 + inkos-story-cover skill 注入，llm/cover.rs 扩展面）；失败 =
//!   TS catch→coverError→stage failed 形状。备案：coverModel 入参未接
//!   resolve 覆盖（模型走 project config/env）；hydrate 检索面=全 body 注入。
//! - `reviseShortFictionProduction`（L742-926 checkpoint 编排）已接线（893）；
//!   applyRevisionPlan 的 scoped 执行分支（authorScope→逐章 reviseChapter +
//!   章题提交）仍为备案（非 scoped 主干执行 instruction，validateRevisionPlan
//!   权限面已核）——随 M4 agent 工具面装配。
//! - TS `recordExecutionEvidence` 观测点（source-material-used 等）未接线——
//!   执行上下文以 [`ShortRunExecutionScope`] 显式注入（893 服务面装配）。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::harness::adopt::acquire_work_mutation_lock;
use crate::harness::artifact_reader::read_artifact_revision;
use crate::harness::contracts::WorkManifest;
use crate::harness::source_sync::{
    capture_work_source_state, changed_work_source_paths, sync_work_source_artifacts,
    SyncWorkSourceInput,
};
use crate::harness::work_store::{
    create_work_manifest, load_work_manifest, save_work_manifest, CreateWorkManifestInput,
};
use crate::models::observation::{
    Observation, ObservationAssessment, ObservationCategory, ObservationTarget,
};
use crate::production::short_fiction::{
    find_incomplete_short_fiction_chapters, measure_short_fiction_draft,
    read_short_production_state, render_short_fiction_draft_markdown,
    render_short_fiction_sales_package, short_input_hash,
    validate_short_fiction_draft_for_final, write_short_production_state, ShortDelivery,
    ShortDraftBounds, ShortFictionBatchDraft, ShortFictionChapter, ShortFictionSalesPackage,
    ShortProductionState, ShortProductionStages, ShortProductionTarget, ShortStage,
    ShortStageComparison, ShortStageStatus,
};
use crate::production::short_fiction_agents::{
    outline_create_outline, packaging_generate, reviewer_review_draft, reviewer_review_package,
    writer_write_draft, OutlineInput, PackageInput, PackageRevisionInput, ReviewDraftInput,
    ReviewPackageInput, WriteDraftInput, WriteDraftOutcome,
};
use crate::production::short_fiction_writer_chain::{
    compress_revise_chapter_fn, writer_continue_draft, ContinueDraftInput,
};
use crate::utils::atomic_file_set::{commit_atomic_file_set, AtomicFileSet, AtomicFileWrite, FileContent};
use crate::utils::language::WritingLanguage;
use crate::utils::path::safe_child_path;
use crate::utils::utc_time::utc_now_iso;

pub const SHORT_FICTION_DEFAULT_CHAPTERS: u32 = 12;
pub const SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER: u32 = 1000;
pub const SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER: u32 = 650;

const SHORT_FICTION_DRAFT_COMPLETION_ATTEMPTS: u32 = 3;
const SHORT_DELIVERY_CONTRACT_CODES: [&str; 3] =
    ["SHORT_CHAPTER_CONTRACT", "SHORT_TITLE_MISMATCH", "SHORT_OPENING_HOOK_CONTRACT"];

// ── 契约 ─────────────────────────────────────────────────────────────

/// TS `ShortFictionRunRuntimes`：四上下文（planner/writer/draftReview/package）。
pub struct ShortFictionRunRuntimes {
    pub planner: crate::production::short_fiction_agents::ShortAgents,
    pub writer: crate::production::short_fiction_agents::ShortAgents,
    pub draft_review: crate::production::short_fiction_agents::ShortAgents,
    pub package: crate::production::short_fiction_agents::ShortAgents,
}

/// TS `recordExecutionEvidence` 的 sink 形态（episode 事件面）。
pub type EvidenceSink = Arc<dyn Fn(&str, &Value) + Send + Sync>;

/// TS 执行上下文（`currentExecutionAuthorRequest`/`currentExecutionBaselineWork`）
/// 的显式注入面——episode 执行由 M3e 服务面装配。
#[derive(Clone, Default)]
pub struct ShortRunExecutionScope {
    pub author_request: Option<String>,
    /// episode 基线 Work 快照（None = 无 episode 上下文 → parent_revision 对照）。
    pub baseline_work: Option<Arc<WorkManifest>>,
    /// TS `recordExecutionEvidence` 的 sink 注入（894 号：episode 事件面——
    /// source-material-used / edit-scope-selected / skills-applied 观测点）。
    pub evidence_sink: Option<EvidenceSink>,
}

impl ShortRunExecutionScope {
    /// TS `recordExecutionEvidence(type, payload)`：sink 缺省为空操作。
    pub fn record(&self, evidence_type: &str, payload: &Value) {
        if let Some(sink) = &self.evidence_sink {
            sink(evidence_type, payload);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShortRetryStage {
    Review,
    Package,
}

/// TS `onProgress` 的共享回调形态。
pub type OnProgress = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub struct ShortFictionRunOptions<'a> {
    pub project_root: &'a Path,
    pub title: Option<&'a str>,
    pub direction: &'a str,
    pub runtimes: &'a ShortFictionRunRuntimes,
    /// TS `reference`（源材料参考——text 供 outline/review 代理；source 身份进
    /// requestHash）。
    pub reference: Option<ShortFictionReference>,
    /// None = 默认 true（TS cover?: boolean）。
    pub cover: Option<bool>,
    /// TS `coverRequired`（2.0 显式封面要求——**优先于** stopAfter/previous 分支，
    /// stage-runner L427 `options.coverRequired ?? …` 逐字；与全链 `cover`
    /// legacy 布尔分字段）。
    pub cover_required: Option<bool>,
    /// TS `coverModel`（894 号 resolve 覆盖面）。
    pub cover_model: Option<&'a str>,
    pub story_id: Option<&'a str>,
    pub chapter_count: Option<u32>,
    pub chars_per_chapter: Option<u32>,
    pub min_chapter_length_ratio: Option<f64>,
    pub min_chapter_length: Option<u32>,
    pub opening_hook_chars: Option<u32>,
    pub max_chapter_length: Option<u32>,
    pub max_chapters_per_call: Option<u32>,
    pub retry_stages: Vec<ShortRetryStage>,
    pub revision_request: Option<&'a str>,
    pub review_scope: Option<&'a str>,
    /// TS `resumeOperationId`（revise 面续跑——只收 ID，新指令即冲突）。
    pub resume_operation_id: Option<&'a str>,
    /// TS `restartPendingRevision`（显式替换 pending 并归档旧 checkpoint）。
    pub restart_pending_revision: bool,
    /// TS `revisionChapterNumbers`（修订章号范围——author scope 收窄面）。
    pub revision_chapter_numbers: Option<Vec<u32>>,
    pub language: Option<WritingLanguage>,
    pub execution: ShortRunExecutionScope,
    pub on_progress: Option<OnProgress>,
}

impl<'a> ShortFictionRunOptions<'a> {
    /// 最小入参构造（其余字段默认）。
    pub fn new(project_root: &'a Path, direction: &'a str, runtimes: &'a ShortFictionRunRuntimes) -> Self {
        Self {
            project_root,
            title: None,
            direction,
            runtimes,
            reference: None,
            cover: None,
            cover_required: None,
            cover_model: None,
            story_id: None,
            chapter_count: None,
            chars_per_chapter: None,
            min_chapter_length_ratio: None,
            min_chapter_length: None,
            opening_hook_chars: None,
            max_chapter_length: None,
            max_chapters_per_call: None,
            retry_stages: Vec::new(),
            revision_request: None,
            review_scope: None,
            resume_operation_id: None,
            restart_pending_revision: false,
            revision_chapter_numbers: None,
            language: None,
            execution: ShortRunExecutionScope::default(),
            on_progress: None,
        }
    }

    fn language(&self) -> WritingLanguage {
        self.language.unwrap_or(WritingLanguage::Zh)
    }

    fn progress(&self, message: &str) {
        if let Some(on_progress) = &self.on_progress {
            on_progress(message);
        }
    }
}

/// TS `ShortFictionReference`（agents/short-fiction.ts：text/path/source/notes）。
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortFictionReference {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ShortOutlineSourceIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortOutlineSourceIdentity {
    pub work_id: String,
    pub artifact_id: String,
    pub revision_id: String,
    pub checksum: String,
}

/// TS `shortDraftMinimum`：篇幅下限面（ratio → target 派生；ratio 非法即抛）。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortDraftMinimum {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "openingHookChars", skip_serializing_if = "Option::is_none")]
    pub opening_hook_chars: Option<u32>,
    pub language: String,
    #[serde(rename = "minChapterLength")]
    pub min_chapter_length: u32,
    #[serde(rename = "maxChapterLength", skip_serializing_if = "Option::is_none")]
    pub max_chapter_length: Option<u32>,
}

pub fn short_draft_minimum(options: &ShortFictionRunOptions<'_>) -> Result<ShortDraftMinimum, ShortRunError> {
    let language = language_code(options.language()).to_string();
    let identity_title = options.title.map(str::to_string);
    let opening_hook_chars = options.opening_hook_chars;
    let max_chapter_length = options.max_chapter_length;
    match options.min_chapter_length_ratio {
        None => Ok(ShortDraftMinimum {
            title: identity_title,
            opening_hook_chars,
            language,
            min_chapter_length: options.min_chapter_length.unwrap_or(1),
            max_chapter_length,
        }),
        Some(ratio) => {
            if !(ratio > 0.0 && ratio <= 1.0) {
                return Err(ShortRunError::new(
                    "SHORT_MIN_RATIO_INVALID",
                    "minChapterLengthRatio must be greater than 0 and at most 1",
                ));
            }
            let target = options.chars_per_chapter.unwrap_or(match options.language() {
                WritingLanguage::En => SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER,
                _ => SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER,
            });
            Ok(ShortDraftMinimum {
                title: identity_title,
                opening_hook_chars,
                language,
                min_chapter_length: options
                    .min_chapter_length
                    .unwrap_or_else(|| std::cmp::max(1, (target as f64 * ratio).ceil() as u32)),
                max_chapter_length,
            })
        }
    }
}

/// TS `ShortFictionRunResult`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortFictionRunResult {
    #[serde(rename = "reviewedArtifact", skip_serializing_if = "Option::is_none")]
    pub reviewed_artifact: Option<ShortReviewedArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<ShortDelivery>,
    #[serde(rename = "storyId")]
    pub story_id: String,
    #[serde(rename = "stageResults", skip_serializing_if = "Option::is_none")]
    pub stage_results: Option<ShortProductionStages>,
    pub observations: Vec<Observation>,
    #[serde(rename = "outlinePath")]
    pub outline_path: String,
    #[serde(rename = "draftReviewPath")]
    pub draft_review_path: String,
    #[serde(rename = "finalMarkdownPath")]
    pub final_markdown_path: String,
    #[serde(rename = "finalJsonPath")]
    pub final_json_path: String,
    #[serde(rename = "salesPackagePath")]
    pub sales_package_path: String,
    #[serde(rename = "coverPromptPath")]
    pub cover_prompt_path: String,
    #[serde(rename = "coverImagePath", skip_serializing_if = "Option::is_none")]
    pub cover_image_path: Option<String>,
    #[serde(rename = "coverError", skip_serializing_if = "Option::is_none")]
    pub cover_error: Option<String>,
    #[serde(rename = "packageError", skip_serializing_if = "Option::is_none")]
    pub package_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortReviewedArtifact {
    pub artifact_id: String,
    pub revision_id: String,
}

/// TS `ShortProductionStageResult`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortProductionStageResult {
    pub story_id: String,
    pub stage: ShortProductionStage,
    pub stage_status: ShortStageStatus,
    pub observations: Vec<Observation>,
    pub artifact_paths: Vec<String>,
    pub cover_required: Option<bool>,
    pub delivery: Option<ShortDelivery>,
    pub reviewed_artifact: Option<ShortReviewedArtifact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ShortProductionStage {
    Outline,
    Draft,
    Review,
    Package,
}

/// produceShort 的双返回形态（stopAfter 有无）。
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ProduceOutcome {
    Run(ShortFictionRunResult),
    Stage(ShortProductionStageResult),
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ShortRunError {
    pub code: String,
    pub message: String,
    /// 失败恢复面（TS recovery 附着；draft 失败的 checkpoint payload）。
    pub recovery: Option<Value>,
}

impl ShortRunError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), recovery: None }
    }

    pub fn with_recovery(mut self, recovery: Value) -> Self {
        self.recovery = Some(recovery);
        self
    }
}

impl From<crate::harness::work_store::WorkStoreError> for ShortRunError {
    fn from(error: crate::harness::work_store::WorkStoreError) -> Self {
        let not_found = matches!(error, crate::harness::work_store::WorkStoreError::NotFound(_));
        Self::new(
            if not_found { "WORK_NOT_FOUND" } else { "WORK_STORE_INVALID" },
            error.to_string(),
        )
    }
}

impl From<crate::harness::source_sync::SyncError> for ShortRunError {
    fn from(error: crate::harness::source_sync::SyncError) -> Self {
        Self::new("SOURCE_SYNC_FAILED", error.to_string())
    }
}

// ── 哈希（TS undefined 键省略语义——None 不入 map）────────────────────

fn hash_present(pairs: Vec<(&str, Option<Value>)>) -> String {
    let mut map = serde_json::Map::new();
    for (key, value) in pairs {
        if let Some(value) = value {
            map.insert(key.into(), value);
        }
    }
    short_input_hash(&Value::Object(map))
}

fn minimum_value(minimum: &ShortDraftMinimum) -> Value {
    serde_json::to_value(minimum).unwrap_or(Value::Null)
}

/// TS `shortReviewRequestHash`：审稿请求身份（约束 + 对照 + 模型面）。
#[allow(clippy::too_many_arguments)]
fn short_review_request_hash(
    options: &ShortFictionRunOptions<'_>,
    story_id: &str,
    title_override: Option<&str>,
    min_override: Option<u32>,
    max_override: Option<u32>,
    hook_override: Option<u32>,
    chapter_count_override: Option<u32>,
    chars_override: Option<u32>,
    language_override: Option<WritingLanguage>,
    reference: Option<&ShortFictionReference>,
) -> String {
    let baseline = options.execution.baseline_work.as_ref();
    let artifact = baseline.filter(|work| work.id == story_id).and_then(|work| {
        work.artifacts.iter().find(|artifact| {
            artifact.revisions.iter().any(|revision| {
                Some(&revision.id) == artifact.current_revision_id.as_ref()
                    && revision.path == "source/final/short-story.json"
            })
        })
    });
    let mut minimum = match short_draft_minimum(options) {
        Ok(minimum) => minimum,
        Err(_) => ShortDraftMinimum {
            title: None,
            opening_hook_chars: None,
            language: language_code(options.language()).into(),
            min_chapter_length: 1,
            max_chapter_length: None,
        },
    };
    if let Some(title) = title_override {
        minimum.title = Some(title.to_string());
    }
    if let Some(min) = min_override {
        minimum.min_chapter_length = min;
    }
    if let Some(max) = max_override {
        minimum.max_chapter_length = Some(max);
    }
    if let Some(hook) = hook_override {
        minimum.opening_hook_chars = Some(hook);
    }
    let comparison_scope = if baseline.is_none() { "parent_revision" } else { "episode_start" };
    let mut comparison = serde_json::Map::new();
    comparison.insert("scope".into(), Value::String(comparison_scope.into()));
    if let Some(artifact) = artifact {
        comparison.insert("artifactId".into(), Value::String(artifact.id.clone()));
        if let Some(current) = &artifact.current_revision_id {
            comparison.insert("revisionId".into(), Value::String(current.clone()));
        }
    }
    hash_present(vec![
        ("evidenceVersion", Some(Value::from(2))),
        ("reference", reference.map(|value| serde_json::to_value(value).unwrap_or(Value::Null))),
        ("constraints", Some(minimum_value(&minimum))),
        ("comparison", Some(Value::Object(comparison))),
        (
            "authorRequest",
            options
                .execution
                .author_request
                .as_deref()
                .filter(|request| !request.trim().is_empty())
                .map(|request| Value::String(request.to_string())),
        ),
        (
            "revisionRequest",
            options.revision_request.map(|request| Value::String(request.to_string())),
        ),
        (
            "reviewScope",
            Some(Value::String(
                options.review_scope.unwrap_or("whole-story").to_string(),
            )),
        ),
        (
            "chapterCount",
            chapter_count_override.map(Value::from),
        ),
        ("charsPerChapter", chars_override.map(Value::from)),
        (
            "language",
            Some(Value::String(
                language_code(language_override.unwrap_or_else(|| options.language())).into(),
            )),
        ),
        ("model", Some(Value::String(options.runtimes.draft_review.model.clone()))),
        // activatedSkills：TS AgentContext 技能注入面——M3e 服务面装配（备案）。
    ])
}

/// TS `shortPackageReviewHash`：包装审查身份（delivery + 方法版本）。
fn short_package_review_hash(delivery: Option<&ShortDelivery>) -> String {
    hash_present(vec![
        (
            "delivery",
            delivery.map(|value| serde_json::to_value(value).unwrap_or(Value::Null)),
        ),
        ("packageMethodVersion", Some(Value::from(2))),
    ])
}

// ── 入口 + 书锁 ──────────────────────────────────────────────────────

/// TS `runShortFictionProduction`：storyId（显式或 title slug）→ 每作品书锁 →
/// 幂等重放短路 → produceShort → 失败候选记录 + 恢复错误。
pub async fn run_short_fiction_production(
    options: &ShortFictionRunOptions<'_>,
) -> Result<ShortFictionRunResult, ShortRunError> {
    let root = options.project_root;
    let provided_story_id: Option<String> = match options.story_id {
        Some(story_id) => Some(safe_segment(story_id)),
        None => options
            .title
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(|title| safe_segment(&slugify(title))),
    };
    if let Some(story_id) = &provided_story_id {
        let lock = acquire_work_mutation_lock(root, story_id);
        let _guard = lock.lock().await;
        run_short_fiction_production_locked(options, root, Some(story_id)).await
    } else {
        run_short_fiction_production_locked(options, root, None).await
    }
}

async fn run_short_fiction_production_locked(
    options: &ShortFictionRunOptions<'_>,
    root: &Path,
    provided_story_id: Option<&str>,
) -> Result<ShortFictionRunResult, ShortRunError> {
    if let Some(story_id) = provided_story_id {
        if let Some(completed) = load_completed_short_run(options, root, story_id).await? {
            return Ok(completed);
        }
    }
    match produce_short(options, root, provided_story_id, None, None).await {
        Ok(ProduceOutcome::Run(result)) => Ok(result),
        Ok(ProduceOutcome::Stage(_)) => unreachable!("stopAfter 未提供时必为完整 Run"),
        Err(error) => {
            if let Some(story_id) = provided_story_id {
                let work_exists = project_file_exists(root, &format!("works/{story_id}/work.json")).await?;
                if work_exists {
                    if let Err(sync_error) = sync_work_source_artifacts(SyncWorkSourceInput {
                        project_root: root,
                        work_id: story_id,
                        accept: false,
                        accept_paths: None,
                        episode_id: None,
                        updated_at: None,
                        writes: Vec::new(),
                        title: None,
                        status: None,
                    })
                    .await
                    {
                        return Err(ShortRunError::new(
                            "SHORT_PRODUCTION_SYNC_FAILED",
                            format!(
                                "Short-fiction production failed and candidate artifacts could not be recorded for {story_id}: {sync_error}; original error: {}",
                                error.message
                            ),
                        )
                        .with_recovery(error.recovery.clone().unwrap_or(Value::Null)));
                    }
                }
                let aborted = false; // TS signal?.aborted 面——892 无取消令牌。
                if !story_id.is_empty() && !aborted {
                    return Err(short_draft_recovery_error(options, root, story_id, error).await?);
                }
            }
            Err(error)
        }
    }
}

/// TS `runShortFictionStage`：单阶段续跑入口（不做书锁——调用方持有）。
pub async fn run_short_fiction_stage(
    options: &ShortFictionRunOptions<'_>,
    stage: ShortProductionStage,
) -> Result<ShortProductionStageResult, ShortRunError> {
    let root = options.project_root;
    let story_id = safe_segment(
        options
            .story_id
            .ok_or_else(|| ShortRunError::new("SHORT_STORY_ID_REQUIRED", "storyId is required"))?,
    );
    let base = short_work_base_dir(&story_id);
    if stage != ShortProductionStage::Outline
        && !project_file_exists(root, &base.join("outline/v001.md").to_string_lossy()).await?
    {
        return Err(ShortRunError::new("SHORT_OUTLINE_REQUIRED", "Create the outline first"));
    }
    if matches!(stage, ShortProductionStage::Review | ShortProductionStage::Package)
        && !project_file_exists(root, &base.join("final/short-story.json").to_string_lossy()).await?
    {
        return Err(ShortRunError::new(
            "SHORT_MANUSCRIPT_REQUIRED",
            "A complete manuscript is required",
        ));
    }
    let state = read_short_production_state(root, &base).map_err(ShortRunError::new_state_read)?;
    let retry_stages: Vec<ShortRetryStage> = match stage {
        ShortProductionStage::Review => vec![ShortRetryStage::Review],
        ShortProductionStage::Package => vec![ShortRetryStage::Package],
        _ => Vec::new(),
    };
    let mut stage_options = options.clone();
    stage_options.story_id = Some(&story_id);
    stage_options.chapter_count =
        options.chapter_count.or_else(|| state.as_ref().and_then(|state| state.target.as_ref()).map(|target| target.chapter_count));
    stage_options.chars_per_chapter =
        options.chars_per_chapter.or_else(|| state.as_ref().and_then(|state| state.target.as_ref()).map(|target| target.chars_per_chapter));
    stage_options.max_chapter_length =
        options.max_chapter_length.or_else(|| state.as_ref().and_then(|state| state.target.as_ref()).and_then(|target| target.max_chapter_length));
    stage_options.language =
        Some(state.as_ref().and_then(|state| state.target.as_ref()).map(|target| target.language).unwrap_or_else(|| options.language()));
    stage_options.cover = Some(false);
    stage_options.retry_stages = retry_stages;
    match produce_short(&stage_options, root, Some(&story_id), None, Some(stage)).await {
        Ok(ProduceOutcome::Stage(result)) => Ok(result),
        Ok(ProduceOutcome::Run(_)) => unreachable!("stopAfter 提供时必为 Stage"),
        Err(error) => Err(error),
    }
}

/// TS `shortDraftRecoveryError`：draft 失败 → v001-partial 候选清单 + 恢复
/// payload（message 尾部 Persisted draft checkpoint）。
async fn short_draft_recovery_error(
    options: &ShortFictionRunOptions<'_>,
    root: &Path,
    story_id: &str,
    error: ShortRunError,
) -> Result<ShortRunError, ShortRunError> {
    let base = short_work_base_dir(story_id);
    let path = base.join("drafts/v001-partial/draft.json");
    let partial = match try_read_short_fiction_draft(root, &path.to_string_lossy()).await {
        Ok(Some(draft)) => draft,
        Ok(None) => return Ok(error),
        Err(read_error) => {
            return Err(ShortRunError::new("SHORT_RECOVERY_READ_FAILED", read_error.message))
        }
    };
    let state = read_short_production_state(root, &base).map_err(ShortRunError::new_state_read)?;
    // TS：target = {...state?.target, ...options 定义项}（已定义项覆盖 saved）。
    let saved_target = state.as_ref().and_then(|state| state.target.clone());
    let mut recovery_options = options.clone();
    recovery_options.min_chapter_length =
        options.min_chapter_length.or_else(|| saved_target.as_ref().and_then(|target| target.min_chapter_length));
    recovery_options.max_chapter_length =
        options.max_chapter_length.or_else(|| saved_target.as_ref().and_then(|target| target.max_chapter_length));
    recovery_options.opening_hook_chars =
        options.opening_hook_chars.or_else(|| saved_target.as_ref().and_then(|target| target.opening_hook_chars));
    recovery_options.chapter_count =
        options.chapter_count.or_else(|| saved_target.as_ref().map(|target| target.chapter_count));
    recovery_options.chars_per_chapter =
        options.chars_per_chapter.or_else(|| saved_target.as_ref().map(|target| target.chars_per_chapter));
    recovery_options.language =
        Some(saved_target.as_ref().map(|target| target.language).unwrap_or_else(|| options.language()));
    let minimum = match short_draft_minimum(&recovery_options) {
        Ok(minimum) => minimum,
        Err(_) => ShortDraftMinimum {
            title: None,
            opening_hook_chars: None,
            language: language_code(options.language()).into(),
            min_chapter_length: 1,
            max_chapter_length: None,
        },
    };
    let incomplete = find_incomplete_short_fiction_chapters(
        &partial,
        &ShortDraftBounds {
            title: None,
            opening_hook_chars: minimum.opening_hook_chars,
            expected_chapters: None,
            min_chapter_length: Some(minimum.min_chapter_length),
            max_chapter_length: minimum.max_chapter_length,
            language: Some(recovery_options.language()),
        },
    );
    let persisted: Vec<u32> = partial
        .chapters
        .iter()
        .filter(|chapter| !chapter.title.trim().is_empty() && !chapter.content.trim().is_empty())
        .map(|chapter| chapter.number)
        .collect();
    let valid: Vec<u32> = partial
        .chapters
        .iter()
        .filter(|chapter| !incomplete.contains(&chapter.number))
        .map(|chapter| chapter.number)
        .collect();
    let recovery = serde_json::json!({
        "action": "short-fiction__draft_short_fiction",
        "parameters": {
            "workId": story_id,
            "minChapterLength": Value::Null,
            "maxChapterLength": Value::Null,
            "openingHookChars": Value::Null,
            "coverRequired": Value::Null,
        },
        "workId": story_id,
        "path": to_project_path(&path.to_string_lossy()),
        "persistedChapterNumbers": persisted,
        "validChapterNumbers": valid,
        "incompleteChapterNumbers": incomplete,
        "requestedChapterCount": recovery_options.chapter_count,
    });
    let code = if error.code == "SHORT_DRAFT_FAILED" || error.code.is_empty() {
        "SHORT_DRAFT_FAILED"
    } else {
        error.code.as_str()
    };
    Ok(ShortRunError {
        code: code.into(),
        message: format!(
            "{}\nPersisted draft checkpoint: {}",
            error.message,
            serde_json::to_string(&recovery).unwrap_or_default()
        ),
        recovery: Some(recovery),
    })
}

/// TS `shortOutlineReference`：登记字节（非工作副本）的源材料参考。
async fn short_outline_reference(
    root: &Path,
    work_id: Option<&str>,
    notes: Option<&ShortFictionReference>,
    worker: &str,
    execution: &ShortRunExecutionScope,
) -> Result<Option<ShortFictionReference>, ShortRunError> {
    let Some(work_id) = work_id else {
        return Ok(notes.cloned());
    };
    let work = match load_work_manifest(root, work_id).await {
        Ok(work) => work,
        Err(crate::harness::work_store::WorkStoreError::NotFound(_)) => return Ok(notes.cloned()),
        Err(error) => return Err(error.into()),
    };
    let Some(artifact) = work.artifacts.iter().find(|artifact| {
        artifact.revisions.iter().any(|revision| {
            Some(&revision.id) == artifact.current_revision_id.as_ref()
                && revision.path == "source/source-material.md"
        })
    }) else {
        return Ok(notes.cloned());
    };
    let read = read_artifact_revision(root, work_id, &artifact.id, None)
        .await
        .map_err(|error| ShortRunError::new("ARTIFACT_NOT_FOUND", error.to_string()))?;
    // TS recordExecutionEvidence('source-material-used', {worker, source, lineage})。
    execution.record(
        "source-material-used",
        &serde_json::json!({
            "worker": worker,
            "source": {
                "workId": work_id,
                "artifactId": artifact.id,
                "revisionId": read.revision.id,
                "checksum": read.revision.checksum,
            },
            "lineage": work.lineage,
        }),
    );
    Ok(Some(ShortFictionReference {
        text: String::from_utf8_lossy(&read.bytes).into_owned(),
        path: Some(read.revision.path.clone()),
        source: Some(ShortOutlineSourceIdentity {
            work_id: work_id.to_string(),
            artifact_id: artifact.id.clone(),
            revision_id: read.revision.id.clone(),
            checksum: read.revision.checksum.clone(),
        }),
        notes: notes.and_then(|value| value.notes.clone()).or_else(|| notes.map(|value| value.text.clone())),
    }))
}

// ── 幂等重放（TS loadCompletedShortRun L227-306）─────────────────────

async fn load_completed_short_run(
    options: &ShortFictionRunOptions<'_>,
    root: &Path,
    story_id: &str,
) -> Result<Option<ShortFictionRunResult>, ShortRunError> {
    let base_dir = short_work_base_dir(story_id);
    let required = [
        "outline/v001.md",
        "drafts/v001/draft.json",
        "final/full.md",
        "final/short-story.json",
        "final/sales-package.json",
        "final/sales-package.md",
        "final/cover-prompt.md",
    ];
    for path in required {
        if !project_file_exists(root, &base_dir.join(path).to_string_lossy()).await? {
            return Ok(None);
        }
    }
    let Some(draft) = try_read_short_fiction_draft(root, &base_dir.join("final/short-story.json").to_string_lossy()).await? else {
        return Ok(None);
    };
    let minimum = short_draft_minimum(options)?;
    if validate_short_fiction_draft_for_final(
        &draft,
        &ShortDraftBounds {
            title: minimum.title.as_deref(),
            opening_hook_chars: minimum.opening_hook_chars,
            expected_chapters: Some(options.chapter_count.unwrap_or(SHORT_FICTION_DEFAULT_CHAPTERS)),
            min_chapter_length: Some(minimum.min_chapter_length),
            max_chapter_length: minimum.max_chapter_length,
            language: Some(options.language()),
        },
    )
    .is_err()
    {
        return Ok(None);
    }
    if project_file_exists(root, &base_dir.join("reviews/package-warning.md").to_string_lossy()).await? {
        return Ok(None);
    }
    let png = project_file_exists(root, &base_dir.join("final/cover.png").to_string_lossy()).await?;
    let jpg = if png { false } else { project_file_exists(root, &base_dir.join("final/cover.jpg").to_string_lossy()).await? };
    let cover_image_path = if png {
        Some(to_project_path(&base_dir.join("final/cover.png").to_string_lossy()))
    } else if jpg {
        Some(to_project_path(&base_dir.join("final/cover.jpg").to_string_lossy()))
    } else {
        None
    };
    if options.cover.unwrap_or(true) && cover_image_path.is_none() {
        return Ok(None);
    }
    if !options.retry_stages.is_empty() {
        return Ok(None);
    }
    let state = read_short_production_state(root, &base_dir).map_err(ShortRunError::new_state_read)?;
    let outline = try_read_project_text(root, &base_dir.join("outline/v001.md").to_string_lossy()).await?;
    let intent = state.as_ref().map(|state| state.intent.clone()).unwrap_or_default();
    let input_hash = hash_present(vec![
        ("draft", Some(serde_json::to_value(&draft).unwrap_or(Value::Null))),
        ("outline", outline.clone().map(Value::String)),
        ("intent", Some(Value::String(intent.clone()))),
    ]);
    if let Some(state) = &state {
        let review_hash_matches = state
            .stages
            .review
            .as_ref()
            .map(|review| review.input_hash == input_hash)
            .unwrap_or(true);
        let package_hash_matches = state
            .stages
            .package
            .as_ref()
            .map(|package| package.input_hash == input_hash)
            .unwrap_or(true);
        if !review_hash_matches || !package_hash_matches {
            return Ok(None);
        }
    }
    if let Some(state) = &state {
        if state.stages.review.as_ref().map(|review| review.status) == Some(ShortStageStatus::Completed) {
            let target = state.target.as_ref();
            // TS：options 定义项优先，saved target 补齐；双缺省=undefined（哈希键省略）。
            let min_override =
                options.min_chapter_length.or_else(|| target.and_then(|target| target.min_chapter_length));
            let max_override =
                options.max_chapter_length.or_else(|| target.and_then(|target| target.max_chapter_length));
            let hook_override =
                options.opening_hook_chars.or_else(|| target.and_then(|target| target.opening_hook_chars));
            let chapter_count_override =
                options.chapter_count.or_else(|| target.map(|target| target.chapter_count));
            let chars_override =
                options.chars_per_chapter.or_else(|| target.map(|target| target.chars_per_chapter));
            let title_override = options
                .title
                .map(str::to_string)
                .or_else(|| target.and_then(|target| target.title.clone()))
                .unwrap_or_else(|| draft.story_title.clone());
            let language_override =
                options.language.or_else(|| target.map(|target| target.language));
            let request_hash = short_review_request_hash(
                options,
                story_id,
                Some(&title_override),
                min_override,
                max_override,
                hook_override,
                chapter_count_override,
                chars_override,
                language_override,
                options.reference.as_ref(),
            );
            if state.stages.review.as_ref().and_then(|review| review.request_hash.as_deref()) != Some(request_hash.as_str()) {
                return Ok(None);
            }
        }
    }
    if let (Some(delivery), Some(package)) =
        (&state.as_ref().and_then(|state| state.delivery.as_ref()), &state.as_ref().and_then(|state| state.stages.package.as_ref()))
    {
        if package.review_hash.as_deref() != Some(short_package_review_hash(Some(delivery)).as_str()) {
            return Ok(None);
        }
    }
    let observations: Vec<Observation> = match &state {
        Some(state) => [
            state.stages.review.as_ref().map(|stage| stage.observations.clone()),
            state.stages.package.as_ref().map(|stage| stage.observations.clone()),
            state.stages.cover.as_ref().map(|stage| stage.observations.clone()),
        ]
        .into_iter()
        .flatten()
        .flatten()
        .collect(),
        None => vec![Observation {
            code: "production-history-unavailable".into(),
            summary: "This manuscript predates persisted stage results. Review status requires verification.".into(),
            evidence: vec![to_project_path(&base_dir.join("reviews/draft-v001.md").to_string_lossy())],
            category: Some(ObservationCategory::Execution),
            assessment: Some(ObservationAssessment::Unavailable),
            scope: None,
            target_hash: None,
            target: None,
            source_refs: None,
        }],
    };
    Ok(Some(build_short_run_result(
        story_id,
        &base_dir,
        observations,
        cover_image_path,
        state.as_ref().and_then(|state| state.stages.package.as_ref()).and_then(|stage| stage.error.clone()),
        state.as_ref().and_then(|state| state.stages.cover.as_ref()).and_then(|stage| stage.error.clone()),
        state.as_ref().map(|state| state.stages.clone()),
        state.as_ref().and_then(|state| state.delivery.clone()),
    )))
}

// ── produceShort 四阶段 ──────────────────────────────────────────────

/// TS `produceShort`（L349-723）。`stop_after` = None 走完整四阶段 + 封面门。
#[allow(clippy::too_many_lines)]
pub async fn produce_short(
    options: &ShortFictionRunOptions<'_>,
    root: &Path,
    provided_story_id: Option<&str>,
    revised_draft: Option<&ShortFictionBatchDraft>,
    stop_after: Option<ShortProductionStage>,
) -> Result<ProduceOutcome, ShortRunError> {
    let language = options.language();
    let saved_target = provided_story_id
        .and_then(|story_id| {
            read_short_production_state(root, &short_work_base_dir(story_id)).ok().flatten()
        })
        .and_then(|state| state.target);
    let effective_title = options.title.map(str::to_string).or_else(|| saved_target.as_ref().and_then(|target| target.title.clone()));
    let min_chapter_length = options
        .min_chapter_length
        .or_else(|| saved_target.as_ref().and_then(|target| target.min_chapter_length));
    let opening_hook_chars = options
        .opening_hook_chars
        .or_else(|| saved_target.as_ref().and_then(|target| target.opening_hook_chars));
    let max_chapter_length = options
        .max_chapter_length
        .or_else(|| saved_target.as_ref().and_then(|target| target.max_chapter_length));
    let chapter_count_input = options
        .chapter_count
        .or_else(|| saved_target.as_ref().map(|target| target.chapter_count));
    let chars_per_chapter_input = options
        .chars_per_chapter
        .or_else(|| saved_target.as_ref().map(|target| target.chars_per_chapter));

    let mut minimum_for_options = options.clone();
    minimum_for_options.title = effective_title.as_deref();
    minimum_for_options.min_chapter_length = min_chapter_length;
    minimum_for_options.opening_hook_chars = opening_hook_chars;
    minimum_for_options.max_chapter_length = max_chapter_length;
    let mut minimum = short_draft_minimum(&minimum_for_options)?;

    let chapter_count = positive_integer(chapter_count_input, SHORT_FICTION_DEFAULT_CHAPTERS, "chapterCount")?;
    let chars_per_chapter = positive_integer(
        chars_per_chapter_input,
        if language == WritingLanguage::En {
            SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER
        } else {
            SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER
        },
        "charsPerChapter",
    )?;

    // 现存 Work 的 outline 续读。
    let resumed_outline: Option<String> = match provided_story_id {
        Some(story_id) => {
            let path = short_work_base_dir(story_id).join("outline/v001.md");
            futures_now_read(root, &path).await
        }
        None => None,
    };
    if let Some(story_id) = provided_story_id {
        if stop_after == Some(ShortProductionStage::Draft) {
            if let Some(resumed_outline) = &resumed_outline {
                let work = load_work_manifest(root, story_id).await?;
                let artifact = work.artifacts.iter().find(|artifact| {
                    artifact.revisions.iter().any(|revision| {
                        Some(&revision.id) == artifact.current_revision_id.as_ref()
                            && revision.path == "source/outline/v001.md"
                    })
                });
                if let Some(artifact) = artifact {
                    let read = read_artifact_revision(root, story_id, &artifact.id, None)
                        .await
                        .map_err(|error| ShortRunError::new("ARTIFACT_NOT_FOUND", error.to_string()))?;
                    if resumed_outline.as_str() != String::from_utf8_lossy(&read.bytes) {
                        return Err(ShortRunError::new(
                            "DRAFT_SOURCE_CHANGED",
                            "The working outline differs from its registered version. Resolve that source before drafting.",
                        )
                        .with_recovery(serde_json::json!({
                            "action": "workspace__inspect_work",
                            "parameters": { "workId": story_id },
                            "reason": "Select or register the intended outline version, review that version when requested, then retry drafting.",
                        })));
                    }
                }
            }
        }
    }

    let (story_id, base_dir, work_title, previous_work_title, source_before, outline_source);
    let outline_markdown: String;
    if let (Some(found_story_id), Some(resumed_outline)) = (provided_story_id, resumed_outline.as_deref().filter(|text| !text.trim().is_empty())) {
        let base = short_work_base_dir(found_story_id);
        let before = capture_work_source_state(root, found_story_id).await?;
        outline_markdown = resumed_outline.to_string();
        ensure_short_work(root, found_story_id, options.title.map(str::trim).filter(|title| !title.is_empty()).unwrap_or(found_story_id), language).await?;
        let work = load_work_manifest(root, found_story_id).await?;
        let previous = work.title.clone();
        let title = options.title.map(str::trim).filter(|title| !title.is_empty()).map(str::to_string).unwrap_or(previous.clone());
        story_id = found_story_id.to_string();
        base_dir = base;
        work_title = title;
        previous_work_title = Some(previous);
        source_before = before;
        outline_source = None;
        options.progress("Resuming from existing outline (skipping outline stages)...");
    } else {
        options.progress("Creating short fiction outline...");
        let reference = short_outline_reference(root, provided_story_id, options.reference.as_ref(), "short-fiction-outline", &options.execution).await?;
        outline_source = reference.as_ref().and_then(|value| value.source.clone());
        let outline_result = outline_create_outline(
            &options.runtimes.planner,
            &OutlineInput {
                title: options.title,
                direction: options.direction,
                chapter_count,
                chars_per_chapter,
                reference_text: reference.as_ref().map(|value| value.text.as_str()),
            },
            language,
        )
        .await
        .map_err(|error| ShortRunError { code: error.code, message: error.message, recovery: None })?;
        let derived = match provided_story_id {
            Some(story_id) => story_id.to_string(),
            None => {
                let seed = if outline_result.story_title.trim().is_empty() {
                    options.direction
                } else {
                    outline_result.story_title.trim()
                };
                safe_segment(&slugify(seed))
            }
        };
        let base = short_work_base_dir(&derived);
        let before = capture_work_source_state(root, &derived).await?;
        let title = options
            .title
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if outline_result.story_title.trim().is_empty() {
                    derived.clone()
                } else {
                    outline_result.story_title.trim().to_string()
                }
            });
        ensure_short_work(root, &derived, &title, language).await?;
        write_text(root, &base.join("outline/v001.md"), &outline_result.raw_content).await?;
        outline_markdown = read_text(root, &base.join("outline/v001.md")).await?;
        story_id = derived;
        base_dir = base;
        work_title = title;
        previous_work_title = None;
        source_before = before;
    }

    let previous_state = read_short_production_state(root, &base_dir).map_err(ShortRunError::new_state_read)?;
    minimum.title = Some(work_title.clone());
    let revision_request = if stop_after == Some(ShortProductionStage::Package) {
        previous_state.as_ref().and_then(|state| state.revision_request.clone())
    } else {
        options.revision_request.map(str::to_string)
    };
    let review_scope = if stop_after == Some(ShortProductionStage::Package) {
        previous_state.as_ref().and_then(|state| state.review_scope.clone())
    } else {
        Some(options.review_scope.unwrap_or("whole-story").to_string())
    };
    let intent = previous_state
        .as_ref()
        .map(|state| state.intent.clone())
        .unwrap_or_else(|| if options.revision_request.is_some() { String::new() } else { options.direction.to_string() });
    // TS L427 逐字：`options.coverRequired ?? (!stopAfter && !revisionRequest
    // ? options.cover !== false || undefined : previousState?.target?.coverRequired)`
    // ——显式 coverRequired 优先（false = Some(false) 有别于缺省 None）。
    let cover_required = options.cover_required.or_else(|| {
        if stop_after.is_none() && options.revision_request.is_none() {
            (options.cover.unwrap_or(true)).then_some(true)
        } else {
            previous_state
                .as_ref()
                .and_then(|state| state.target.as_ref())
                .and_then(|target| target.cover_required)
        }
    });
    let outline_source_reference = outline_source.map(|source| crate::production::short_fiction::ShortStageReviewedReference {
        work_id: source.work_id,
        artifact_id: source.artifact_id,
        revision_id: source.revision_id,
        checksum: source.checksum,
    });
    let mut production_state = ShortProductionState {
        outline_source: outline_source_reference.or_else(|| previous_state.as_ref().and_then(|state| state.outline_source.clone())),
        version: 2,
        target: Some(ShortProductionTarget {
            title: Some(work_title.clone()),
            chapter_count,
            chars_per_chapter,
            min_chapter_length: Some(minimum.min_chapter_length),
            max_chapter_length,
            opening_hook_chars,
            cover_required,
            language,
        }),
        intent: intent.clone(),
        revision_request,
        review_scope,
        delivery: None,
        stages: previous_state.as_ref().map(|state| state.stages.clone()).unwrap_or_default(),
    };
    write_short_production_state(root, &base_dir, &production_state)
        .await
        .map_err(|message| ShortRunError::new("PRODUCTION_STATE_INVALID", message))?;
    sync_work_source_artifacts(SyncWorkSourceInput {
        project_root: root,
        work_id: &story_id,
        accept: false,
        accept_paths: None,
        episode_id: None,
        updated_at: None,
        writes: Vec::new(),
        title: None,
        status: None,
    })
    .await?;

    if stop_after == Some(ShortProductionStage::Outline) {
        return Ok(ProduceOutcome::Stage(
            stage_result(
                root,
                &story_id,
                &base_dir,
                &work_title,
                previous_work_title.as_deref(),
                &source_before,
                ShortProductionStage::Outline,
                vec!["outline/v001.md".into()],
                &production_state,
                None,
                false,
            )
            .await?,
        ));
    }

    // ── draft 阶段 ──
    options.progress("Writing full short fiction draft...");
    let review_or_package_only =
        stop_after == Some(ShortProductionStage::Review) || stop_after == Some(ShortProductionStage::Package);
    let preserve_manuscript = review_or_package_only || revised_draft.is_some();
    let candidate_path = if provided_story_id.is_some()
        && (review_or_package_only || !options.retry_stages.is_empty())
    {
        base_dir.join("final/short-story.json")
    } else {
        base_dir.join("drafts/v001/draft.json")
    };
    // TS：currentDraft = revisedDraft ??（provided → 磁盘读取）——修订传入草稿优先。
    let current_draft = match revised_draft {
        Some(draft) => Some(draft.clone()),
        None => {
            if provided_story_id.is_some() {
                try_read_short_fiction_draft(root, &candidate_path.to_string_lossy()).await?
            } else {
                None
            }
        }
    };
    let resumed_draft = match current_draft {
        Some(draft) => Some(draft),
        None => try_read_short_fiction_draft(root, &base_dir.join("drafts/v001-partial/draft.json").to_string_lossy()).await?,
    };
    if review_or_package_only && resumed_draft.is_none() {
        return Err(ShortRunError::new(
            "SHORT_MANUSCRIPT_REQUIRED",
            "A persisted complete manuscript is required for review or packaging",
        ));
    }
    // compress 回退装配（TS applyRevisionPlan 内联 `new ReviserAgent(this.ctx)`——
    // continueDraft 的完成环经此注入位走受控改写）。
    let revise_chapter = compress_revise_chapter_fn(
        Arc::clone(&options.runtimes.writer.chat),
        options.runtimes.writer.model.clone(),
        options.runtimes.writer.defaults.clone(),
        language,
        chars_per_chapter,
        Some(minimum.min_chapter_length),
        max_chapter_length,
        production_state.intent.clone(),
    );
    let write_draft_input = WriteDraftInput {
        title: Some(&work_title),
        opening_hook_chars,
        direction: &production_state.intent,
        outline_markdown: &outline_markdown,
        chapter_count,
        chars_per_chapter,
        min_chapter_length: Some(minimum.min_chapter_length),
        max_chapter_length,
        max_chapters_per_call: options.max_chapters_per_call,
        chapter_numbers: None,
        language,
    };
    let language_for_progress = language;
    let root_for_persist = root.to_path_buf();
    let story_for_persist = story_id.clone();
    let base_for_persist = base_dir.clone();
    let progress_for_persist = options.on_progress.clone();
    // TS persistDraftBatch（async——落盘顺序面）：候选批次原子提交 + 同步登记。
    let persist_draft_batch: crate::production::short_fiction_writer_chain::OnBatchComplete =
        Arc::new(move |draft: &ShortFictionBatchDraft, completed: &[u32]| {
        let root = root_for_persist.clone();
        let story = story_for_persist.clone();
        let base = base_for_persist.clone();
        let language = language_for_progress;
        let progress = progress_for_persist.clone();
        let draft = draft.clone();
        let completed: Vec<u32> = completed.to_vec();
        Box::pin(async move {
            write_draft_artifacts(&root, &base, "v001-partial", &draft, language)
                .await
                .map_err(|error| error.message)?;
            sync_work_source_artifacts(SyncWorkSourceInput {
                project_root: &root,
                work_id: &story,
                accept: false,
                accept_paths: None,
                episode_id: None,
                updated_at: None,
                writes: Vec::new(),
                title: None,
                status: None,
            })
            .await
            .map_err(|error| error.to_string())?;
            let saved = draft
                .chapters
                .iter()
                .filter(|chapter| !chapter.title.trim().is_empty() && !chapter.content.trim().is_empty())
                .count();
            let message = if language == WritingLanguage::En {
                format!("Saved {saved} candidate chapters; {} currently meet the text and length requirements.", completed.len())
            } else {
                format!("已保存 {saved} 章候选正文，{} 章已满足篇幅与文本要求。", completed.len())
            };
            if let Some(progress) = &progress {
                progress(&message);
            }
            Ok(())
        })
    });

    let candidate_draft = match resumed_draft {
        Some(draft) => draft,
        None => {
            let on_batch = {
                let persist = Arc::clone(&persist_draft_batch);
                move |draft: &ShortFictionBatchDraft, completed: &[u32]| {
                    let persist = Arc::clone(&persist);
                    let draft = draft.clone();
                    let completed: Vec<u32> = completed.to_vec();
                    Box::pin(async move {
                        persist(&draft, &completed).await
                    }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
                }
            };
            let outcome: WriteDraftOutcome = writer_write_draft(&options.runtimes.writer, &write_draft_input, on_batch)
                .await
                .map_err(|error| ShortRunError { code: error.code, message: error.message, recovery: None })?;
            outcome.draft
        }
    };
    let mut draft_v1 = if preserve_manuscript {
        candidate_draft
    } else {
        let on_batch: crate::production::short_fiction_writer_chain::OnBatchComplete = {
            let persist = Arc::clone(&persist_draft_batch);
            Arc::new(move |draft: &ShortFictionBatchDraft, completed: &[u32]| {
                let persist = Arc::clone(&persist);
                let draft = draft.clone();
                let completed: Vec<u32> = completed.to_vec();
                Box::pin(async move { persist(&draft, &completed).await })
            })
        };
        writer_continue_draft(
            &options.runtimes.writer,
            &ContinueDraftInput {
                title: Some(&work_title),
                opening_hook_chars,
                direction: &production_state.intent,
                outline_markdown: &outline_markdown,
                chapter_count,
                chars_per_chapter,
                min_chapter_length: Some(minimum.min_chapter_length),
                max_chapter_length,
                language,
                draft: &candidate_draft,
            },
            Some(&revise_chapter),
            on_batch,
        )
        .await
        .map_err(|error| ShortRunError { code: error.code, message: error.message, recovery: None })?
    };
    let mut missing_from_draft = if preserve_manuscript {
        Vec::new()
    } else {
        find_incomplete_short_fiction_chapters(
            &draft_v1,
            &ShortDraftBounds {
                title: None,
                opening_hook_chars: minimum.opening_hook_chars,
                expected_chapters: None,
                min_chapter_length: Some(minimum.min_chapter_length),
                max_chapter_length,
                language: Some(language),
            },
        )
    };
    if !missing_from_draft.is_empty() {
        write_draft_artifacts(root, &base_dir, "v001-partial", &draft_v1, language).await?;
        let mut attempt = 1;
        while !missing_from_draft.is_empty() && attempt <= SHORT_FICTION_DRAFT_COMPLETION_ATTEMPTS {
            options.progress(&format!(
                "Completing missing short fiction chapters: {}...",
                missing_from_draft.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
            ));
            let on_batch: crate::production::short_fiction_writer_chain::OnBatchComplete = {
                let persist = Arc::clone(&persist_draft_batch);
                Arc::new(move |draft: &ShortFictionBatchDraft, completed: &[u32]| {
                    let persist = Arc::clone(&persist);
                    let draft = draft.clone();
                    let completed: Vec<u32> = completed.to_vec();
                    Box::pin(async move { persist(&draft, &completed).await })
                })
            };
            draft_v1 = writer_continue_draft(
                &options.runtimes.writer,
                &ContinueDraftInput {
                    title: Some(&work_title),
                    opening_hook_chars,
                    direction: &production_state.intent,
                    outline_markdown: &outline_markdown,
                    chapter_count,
                    chars_per_chapter,
                    min_chapter_length: Some(minimum.min_chapter_length),
                    max_chapter_length,
                    language,
                    draft: &draft_v1,
                },
                Some(&revise_chapter),
                on_batch,
            )
            .await
            .map_err(|error| ShortRunError { code: error.code, message: error.message, recovery: None })?;
            missing_from_draft = find_incomplete_short_fiction_chapters(
                &draft_v1,
                &ShortDraftBounds {
                    title: None,
                    opening_hook_chars: minimum.opening_hook_chars,
                    expected_chapters: None,
                    min_chapter_length: Some(minimum.min_chapter_length),
                    max_chapter_length,
                    language: Some(language),
                },
            );
            if !missing_from_draft.is_empty() {
                write_draft_artifacts(root, &base_dir, "v001-partial", &draft_v1, language).await?;
            }
            attempt += 1;
        }
    }
    validate_short_fiction_draft_for_final(
        &draft_v1,
        &ShortDraftBounds {
            title: if preserve_manuscript { None } else { Some(&work_title) },
            opening_hook_chars: if preserve_manuscript { None } else { opening_hook_chars },
            expected_chapters: Some(chapter_count),
            min_chapter_length: if preserve_manuscript { Some(1) } else { Some(minimum.min_chapter_length) },
            max_chapter_length: if preserve_manuscript { None } else { max_chapter_length },
            language: Some(language),
        },
    )
    .map_err(|error| ShortRunError { code: error.code.into(), message: error.message, recovery: None })?;
    write_draft_artifacts(root, &base_dir, "v001", &draft_v1, language).await?;
    let final_draft = draft_v1;

    let reviewed_work = sync_work_source_artifacts(SyncWorkSourceInput {
        project_root: root,
        work_id: &story_id,
        accept: false,
        accept_paths: None,
        episode_id: None,
        updated_at: None,
        writes: Vec::new(),
        title: None,
        status: None,
    })
    .await?;
    let reviewed_artifact = reviewed_work
        .manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.revisions.iter().any(|revision| revision.path == "source/drafts/v001/draft.json"))
        .ok_or_else(|| ShortRunError::new("SHORT_DRAFT_REGISTRATION_MISSING", "draft.json revision missing after sync"))?
        .clone();
    let draft_json_bytes = read_binary(root, &base_dir.join("drafts/v001/draft.json")).await?;
    let reviewed_hash = format!("sha256:{}", hex_sha256(&draft_json_bytes));
    let reviewed_revision = reviewed_artifact
        .revisions
        .iter()
        .find(|revision| revision.checksum == reviewed_hash)
        .ok_or_else(|| ShortRunError::new("SHORT_DRAFT_REGISTRATION_MISSING", "draft.json checksum mismatch after sync"))?
        .clone();
    if stop_after == Some(ShortProductionStage::Draft) {
        write_final_artifacts(root, &base_dir, &final_draft, language, previous_work_title.as_deref()).await?;
        return Ok(ProduceOutcome::Stage(
            stage_result(
                root,
                &story_id,
                &base_dir,
                &work_title,
                previous_work_title.as_deref(),
                &source_before,
                ShortProductionStage::Draft,
                vec!["final/short-story.json".into(), "final/full.md".into()],
                &production_state,
                Some(&final_draft),
                preserve_manuscript,
            )
            .await?,
        ));
    }

    let input_hash = hash_present(vec![
        ("draft", Some(serde_json::to_value(&final_draft).unwrap_or(Value::Null))),
        ("outline", Some(Value::String(outline_markdown.clone()))),
        ("intent", Some(Value::String(production_state.intent.clone()))),
    ]);

    // ── review 阶段 ──
    let mut draft_review_warning: Option<String> = None;
    if stop_after != Some(ShortProductionStage::Package) {
        options.progress("Reviewing completed short fiction...");
        let reference = short_outline_reference(root, Some(&story_id), options.reference.as_ref(), "short-fiction-review", &options.execution).await?;
        let request_hash = short_review_request_hash(
            &minimum_for_options,
            &story_id,
            Some(&work_title),
            Some(minimum.min_chapter_length),
            max_chapter_length,
            opening_hook_chars,
            Some(chapter_count),
            Some(chars_per_chapter),
            Some(language),
            reference.as_ref(),
        );
        // 对照面（TS L554-567）。
        let baseline = options.execution.baseline_work.as_ref();
        let baseline_artifact = baseline.filter(|work| work.id == story_id).and_then(|work| {
            work.artifacts.iter().find(|artifact| {
                artifact.revisions.iter().any(|revision| {
                    Some(&revision.id) == artifact.current_revision_id.as_ref()
                        && revision.path == "source/final/short-story.json"
                })
            })
        });
        let baseline_id = if baseline.is_none() {
            reviewed_revision.parent_revision_id.clone()
        } else {
            baseline_artifact.and_then(|artifact| artifact.current_revision_id.clone())
        };
        let mut review_comparison: Option<ShortStageComparison> = None;
        let mut comparison_prompt: Option<Value> = None;
        if let Some(baseline_id) = baseline_id {
            let before = read_artifact_revision(
                root,
                &story_id,
                baseline_artifact.map(|artifact| artifact.id.as_str()).unwrap_or(reviewed_artifact.id.as_str()),
                Some(&baseline_id),
            )
            .await
            .map_err(|error| ShortRunError::new("ARTIFACT_NOT_FOUND", error.to_string()))?;
            let before_draft: ShortFictionBatchDraft = serde_json::from_slice(&before.bytes)
                .map_err(|error| ShortRunError::new("SHORT_REVISION_BASELINE_INVALID", error.to_string()))?;
            comparison_prompt = Some(serde_json::json!({
                "scope": if baseline.is_none() { "parent_revision" } else { "episode_start" },
                "before": {
                    "artifactId": before.artifact.id,
                    "revisionId": before.revision.id,
                    "checksum": before.revision.checksum,
                },
                "after": {
                    "artifactId": reviewed_artifact.id,
                    "revisionId": reviewed_revision.id,
                    "checksum": reviewed_revision.checksum,
                },
                "draft": serde_json::to_value(&before_draft).unwrap_or(Value::Null),
            }));
            review_comparison = Some(ShortStageComparison {
                scope: if baseline.is_none() {
                    crate::production::short_fiction::ShortComparisonScope::ParentRevision
                } else {
                    crate::production::short_fiction::ShortComparisonScope::EpisodeStart
                },
                before: crate::production::short_fiction::ShortArtifactAddress {
                    artifact_id: before.artifact.id.clone(),
                    revision_id: before.revision.id.clone(),
                    checksum: before.revision.checksum.clone(),
                },
                after: crate::production::short_fiction::ShortArtifactAddress {
                    artifact_id: reviewed_artifact.id.clone(),
                    revision_id: reviewed_revision.id.clone(),
                    checksum: reviewed_revision.checksum.clone(),
                },
            });
        }
        let reuse_review = production_state
            .stages
            .review
            .as_ref()
            .map(|review| {
                review.status == ShortStageStatus::Completed
                    && review.input_hash == input_hash
                    && review.request_hash.as_deref() == Some(request_hash.as_str())
                    && !options.retry_stages.contains(&ShortRetryStage::Review)
            })
            .unwrap_or(false);
        let draft_review = if reuse_review {
            Ok(crate::production::short_fiction_agents::ReviewOutcome {
                summary: String::new(),
                observations: production_state
                    .stages
                    .review
                    .as_ref()
                    .map(|review| review.observations.clone())
                    .unwrap_or_default(),
                usage: Default::default(),
            })
        } else {
            reviewer_review_draft(
                &options.runtimes.draft_review,
                &ReviewDraftInput {
                    direction: &production_state.intent,
                    outline_markdown: &outline_markdown,
                    draft: &final_draft,
                    reference_text: reference.as_ref().map(|value| value.text.as_str()),
                    revision_request: options
                        .execution
                        .author_request
                        .as_deref()
                        .map(str::to_string)
                        .or_else(|| options.revision_request.map(str::to_string))
                        .as_deref(),
                    review_scope: production_state.review_scope.as_deref(),
                    measurements: None,
                    comparison: comparison_prompt.as_ref(),
                },
                language,
            )
            .await
        };
        let mut draft_review_observations: Vec<Observation> = Vec::new();
        match draft_review {
            Ok(review) => {
                draft_review_observations = review
                    .observations
                    .iter()
                    .map(|observation| {
                        let mut mapped = observation.clone();
                        mapped.category = Some(observation.category.unwrap_or(ObservationCategory::Quality));
                        mapped.assessment = Some(observation.assessment.unwrap_or(ObservationAssessment::Observation));
                        mapped.scope = production_state.review_scope.clone();
                        mapped.target_hash = Some(reviewed_hash.clone());
                        mapped.target = Some(ObservationTarget {
                            work_id: story_id.clone(),
                            artifact_id: reviewed_artifact.id.clone(),
                            revision_id: reviewed_revision.id.clone(),
                        });
                        mapped
                    })
                    .collect();
                if !reuse_review {
                    write_text(
                        root,
                        &base_dir.join("reviews/draft-v001.md"),
                        &render_short_fiction_review(&review.summary, &draft_review_observations, language, "draft"),
                    )
                    .await?;
                }
                let _ = commit_delete(root, &base_dir.join("reviews/draft-warning.md")).await;
            }
            Err(error) => {
                draft_review_warning = Some(error.message.clone());
                write_text(
                    root,
                    &base_dir.join("reviews/draft-warning.md"),
                    &if language == WritingLanguage::En {
                        format!("# Review unavailable\n\nThe complete draft remains available.\n\n## Reason\n\n{}", error.message)
                    } else {
                        format!("# 审稿暂不可用\n\n完整正文已经保留。\n\n## 原因\n\n{}", error.message)
                    },
                )
                .await?;
            }
        }
        production_state.stages.review = Some(ShortStage {
            status: if draft_review_warning.is_some() { ShortStageStatus::Failed } else { ShortStageStatus::Completed },
            input_hash: input_hash.clone(),
            request_hash: Some(request_hash),
            review_hash: None,
            reviewed_artifact: if draft_review_warning.is_none() {
                Some(crate::production::short_fiction::ShortStageReviewedArtifact {
                    artifact_id: reviewed_artifact.id.clone(),
                    revision_id: reviewed_revision.id.clone(),
                })
            } else {
                None
            },
            reviewed_reference: if draft_review_warning.is_none() {
                reference.as_ref().and_then(|value| value.source.clone()).map(|source| {
                    crate::production::short_fiction::ShortStageReviewedReference {
                        work_id: source.work_id,
                        artifact_id: source.artifact_id,
                        revision_id: source.revision_id,
                        checksum: source.checksum,
                    }
                })
            } else {
                None
            },
            comparison: review_comparison,
            updated_at: utc_now_iso(),
            observations: if let Some(warning) = &draft_review_warning {
                vec![Observation {
                    code: "draft-review".into(),
                    summary: warning.clone(),
                    evidence: vec![to_project_path(&base_dir.join("reviews/draft-warning.md").to_string_lossy())],
                    category: Some(ObservationCategory::Execution),
                    assessment: Some(ObservationAssessment::Unavailable),
                    scope: production_state.review_scope.clone(),
                    target_hash: Some(input_hash.clone()),
                    target: None,
                    source_refs: None,
                }]
            } else {
                draft_review_observations.clone()
            },
            error: draft_review_warning.clone(),
        });
        write_short_production_state(root, &base_dir, &production_state)
            .await
            .map_err(|message| ShortRunError::new("PRODUCTION_STATE_INVALID", message))?;
    }
    if stop_after == Some(ShortProductionStage::Review) {
        if revised_draft.is_some() {
            write_final_artifacts(root, &base_dir, &final_draft, language, previous_work_title.as_deref()).await?;
        }
        return Ok(ProduceOutcome::Stage(
            stage_result(
                root,
                &story_id,
                &base_dir,
                &work_title,
                previous_work_title.as_deref(),
                &source_before,
                ShortProductionStage::Review,
                vec![if draft_review_warning.is_some() {
                    "reviews/draft-warning.md".into()
                } else {
                    "reviews/draft-v001.md".into()
                }, "production-state.json".into()],
                &production_state,
                Some(&final_draft),
                preserve_manuscript,
            )
            .await?,
        ));
    }
    if !review_or_package_only {
        write_final_artifacts(root, &base_dir, &final_draft, language, previous_work_title.as_deref()).await?;
    }

    // ── package 阶段 ──
    options.progress("Generating synopsis and cover prompt...");
    refresh_delivery(
        root,
        &base_dir,
        &mut production_state,
        &final_draft,
        &outline_markdown,
        &work_title,
        opening_hook_chars,
        &minimum,
        language,
    )
    .await?;
    let mut package_warning: Option<String> = None;
    let mut package_observations: Vec<Observation> = Vec::new();
    let mut package_reviewed_artifact: Option<ShortReviewedArtifact> = None;
    let mut sales_package: Option<ShortFictionSalesPackage> = None;
    {
        let cached_package = production_state.stages.package.clone();
        let review_hash = short_package_review_hash(production_state.delivery.as_ref());
        let saved_package = try_read_project_text(root, &base_dir.join("final/sales-package.json").to_string_lossy()).await?;
        let package_direction = if stop_after == Some(ShortProductionStage::Package)
            && !options.direction.trim().is_empty()
            && options.direction != production_state.intent
        {
            if language == WritingLanguage::En {
                format!("{}\n\nCurrent packaging request:\n\n{}", production_state.intent, options.direction)
            } else {
                format!("{}\n\n本次包装要求：\n\n{}", production_state.intent, options.direction)
            }
        } else {
            production_state.intent.clone()
        };
        let reuse_package = cached_package
            .as_ref()
            .map(|package| {
                package.status == ShortStageStatus::Completed
                    && package.input_hash == input_hash
                    && package.review_hash.as_deref() == Some(review_hash.as_str())
                    && saved_package.is_some()
                    && !options.retry_stages.contains(&ShortRetryStage::Package)
            })
            .unwrap_or(false);
        let package_result: Result<ShortFictionSalesPackage, ShortRunError> = if reuse_package {
            // TS：JSON.parse(savedPackage) 信任已登记文档（额外键容忍）。
            saved_package
                .as_deref()
                .map(|raw| {
                    serde_json::from_str::<Value>(raw)
                        .map_err(|error| error.to_string())
                        .and_then(|value| {
                            package_from_saved_value(&value)
                                .ok_or_else(|| "saved sales package is missing required fields".to_string())
                        })
                })
                .unwrap_or_else(|| Err("missing saved sales package".to_string()))
                .map_err(|message| ShortRunError::new("SHORT_PACKAGE_INVALID", message))
        } else {
            packaging_generate(
                &options.runtimes.package,
                &PackageInput { direction: &package_direction, draft: &final_draft, language, revision: None },
            )
            .await
            .map_err(|error| ShortRunError { code: error.code, message: error.message, recovery: None })
        };
        match package_result {
            Ok(mut package) => {
                // TS L637：reuse/generate 后先落盘三件（修正环内 sync 才有已登记面可审）。
                write_package_artifacts(root, &base_dir, &package, language).await?;
                if reuse_package {
                    package_observations = cached_package.as_ref().map(|cached| cached.observations.clone()).unwrap_or_default();
                    package_reviewed_artifact = cached_package.as_ref().and_then(|cached| cached.reviewed_artifact.clone()).map(|value| ShortReviewedArtifact {
                        artifact_id: value.artifact_id,
                        revision_id: value.revision_id,
                    });
                } else {
                    // 一次修正环（pass 0/1）——每个审查都锚定它检查的确切包装。
                    for pass in 0..2 {
                        let work = sync_work_source_artifacts(SyncWorkSourceInput {
                            project_root: root,
                            work_id: &story_id,
                            accept: true,
                            accept_paths: Some(vec![
                                "source/final/sales-package.json".into(),
                                "source/final/sales-package.md".into(),
                                "source/final/cover-prompt.md".into(),
                            ]),
                            episode_id: None,
                            updated_at: None,
                            writes: Vec::new(),
                            title: None,
                            status: None,
                        })
                        .await?;
                        let artifact = work
                            .manifest
                            .artifacts
                            .iter()
                            .find(|artifact| {
                                artifact.revisions.iter().any(|revision| {
                                    Some(&revision.id) == artifact.current_revision_id.as_ref()
                                        && revision.path == "source/final/sales-package.md"
                                })
                            })
                            .ok_or_else(|| ShortRunError::new("SHORT_PACKAGE_REGISTRATION_MISSING", "sales-package.md revision missing after sync"))?;
                        let revision = artifact
                            .revisions
                            .iter()
                            .find(|revision| Some(&revision.id) == artifact.current_revision_id.as_ref())
                            .ok_or_else(|| ShortRunError::new("SHORT_PACKAGE_REGISTRATION_MISSING", "sales-package.md current revision missing"))?;
                        let review = reviewer_review_package(
                            &options.runtimes.draft_review,
                            &ReviewPackageInput { draft: &final_draft, sales: &package },
                            language,
                        )
                        .await
                        .map_err(|error| ShortRunError { code: error.code, message: error.message, recovery: None })?;
                        package_reviewed_artifact = Some(ShortReviewedArtifact {
                            artifact_id: artifact.id.clone(),
                            revision_id: revision.id.clone(),
                        });
                        package_observations = review
                            .observations
                            .iter()
                            .map(|observation| {
                                let mut mapped = observation.clone();
                                mapped.category = Some(observation.category.unwrap_or(ObservationCategory::Quality));
                                mapped.scope = Some("sales-package".into());
                                mapped.target_hash = Some(revision.checksum.clone());
                                mapped.target = Some(ObservationTarget {
                                    work_id: story_id.clone(),
                                    artifact_id: artifact.id.clone(),
                                    revision_id: revision.id.clone(),
                                });
                                mapped
                            })
                            .collect();
                        write_text(
                            root,
                            &base_dir.join("reviews/package-v001.md"),
                            &render_short_fiction_review(&review.summary, &package_observations, language, "package"),
                        )
                        .await?;
                        sync_work_source_artifacts(SyncWorkSourceInput {
                            project_root: root,
                            work_id: &story_id,
                            accept: true,
                            accept_paths: Some(vec!["source/reviews/package-v001.md".into()]),
                            episode_id: None,
                            updated_at: None,
                            writes: Vec::new(),
                            title: None,
                            status: None,
                        })
                        .await?;
                        let issues: Vec<Observation> = package_observations
                            .iter()
                            .filter(|observation| observation.assessment == Some(ObservationAssessment::Issue))
                            .cloned()
                            .collect();
                        if pass == 1 || issues.is_empty() {
                            break;
                        }
                        // 修正再生成（TS revision: {previous, observations}）。
                        package = packaging_generate(
                            &options.runtimes.package,
                            &PackageInput {
                                direction: &package_direction,
                                draft: &final_draft,
                                language,
                                revision: Some(PackageRevisionInput {
                                    previous: &package,
                                    observations: &issues,
                                }),
                            },
                        )
                        .await
                        .map_err(|error| ShortRunError { code: error.code, message: error.message, recovery: None })?;
                        write_package_artifacts(root, &base_dir, &package, language).await?;
                    }
                }
                sales_package = Some(package);
                let _ = commit_delete(root, &base_dir.join("reviews/package-warning.md")).await;
            }
            Err(error) => {
                package_warning = Some(error.message.clone());
                write_text(
                    root,
                    &base_dir.join("reviews/package-warning.md"),
                    &if language == WritingLanguage::En {
                        format!("# Packaging requires retry\n\nThe complete story remains available. Synopsis and cover packaging failed without invalidating the prose.\n\n## Reason\n\n{}", error.message)
                    } else {
                        format!("# 包装阶段需要重试\n\n完整正文已经保留。简介与封面包装失败不会再把正文标成失败。\n\n## 原因\n\n{}", error.message)
                    },
                )
                .await?;
            }
        }
    }
    production_state.stages.package = Some(ShortStage {
        status: if package_warning.is_some() { ShortStageStatus::Failed } else { ShortStageStatus::Completed },
        input_hash: input_hash.clone(),
        request_hash: None,
        review_hash: Some(short_package_review_hash(production_state.delivery.as_ref())),
        reviewed_artifact: package_reviewed_artifact.clone().map(|value| crate::production::short_fiction::ShortStageReviewedArtifact {
            artifact_id: value.artifact_id,
            revision_id: value.revision_id,
        }),
        reviewed_reference: None,
        comparison: None,
        updated_at: utc_now_iso(),
        observations: {
            let mut observations = package_observations.clone();
            if let Some(warning) = &package_warning {
                observations.push(Observation {
                    code: "package-generation".into(),
                    summary: warning.clone(),
                    evidence: Vec::new(),
                    category: Some(ObservationCategory::Execution),
                    assessment: Some(ObservationAssessment::Unavailable),
                    scope: None,
                    target_hash: Some(input_hash.clone()),
                    target: None,
                    source_refs: None,
                });
            }
            observations
        },
        error: package_warning.clone(),
    });
    write_short_production_state(root, &base_dir, &production_state)
        .await
        .map_err(|message| ShortRunError::new("PRODUCTION_STATE_INVALID", message))?;

    if stop_after == Some(ShortProductionStage::Package) {
        return Ok(ProduceOutcome::Stage(
            stage_result(
                root,
                &story_id,
                &base_dir,
                &work_title,
                previous_work_title.as_deref(),
                &source_before,
                ShortProductionStage::Package,
                vec![if package_warning.is_some() {
                    "reviews/package-warning.md".into()
                } else {
                    "final/sales-package.json".into()
                }, if package_warning.is_some() {
                    "production-state.json".into()
                } else {
                    "final/sales-package.md".into()
                }, if package_warning.is_some() {
                    "production-state.json".into()
                } else {
                    "final/cover-prompt.md".into()
                }, if package_warning.is_some() {
                    "production-state.json".into()
                } else {
                    "reviews/package-v001.md".into()
                }],
                &production_state,
                Some(&final_draft),
                preserve_manuscript,
            )
            .await?,
        ));
    }

    // ── 封面门面（893 号：2.0 生成本体接线——失败 catch→coverError 语义）──
    let cover_requested = options.cover.unwrap_or(true);
    let mut cover_image_path: Option<String> = None;
    let (cover_error, cover_error_code): (Option<String>, Option<String>) = if !cover_requested {
        (Some("disabled".into()), None)
    } else if package_warning.is_some()
        || sales_package.as_ref().map(|package| package.cover_prompt.trim().is_empty()).unwrap_or(true)
    {
        (
            Some("Cover generation requires a completed sales package with a story-grounded visual brief. Retry packaging, then resume cover generation.".into()),
            Some("SHORT_COVER_PACKAGE_REQUIRED".into()),
        )
    } else {
        // TS generateCoverArtifact(...).catch(error => ({coverError: String(error)}))
        // ——生成失败不推翻正文与包装，只记封面阶段错误（code 缺省 cover-generation）。
        let package = sales_package.as_ref().unwrap();
        let sales_view = crate::llm::cover::CoverSalesPackage {
            title: &package.title,
            intro: &package.intro,
            selling_points: &package.selling_points,
            cover_prompt: &package.cover_prompt,
        };
        let evidence_ref = options.execution.evidence_sink.as_deref();
        match crate::llm::cover::generate_cover_image_artifact(
            root,
            &base_dir.join("final"),
            true,
            &sales_view,
            options.execution.author_request.as_deref().or(Some(options.direction)),
            &[],
            Some(language_code(language)),
            None,
            options.cover_model,
            evidence_ref,
        )
        .await
        {
            Ok(path) => {
                cover_image_path = Some(path);
                (None, None)
            }
            Err(message) => (Some(message), None),
        }
    };
    if cover_requested {
        production_state.stages.cover = Some(ShortStage {
            status: if cover_error.is_some() { ShortStageStatus::Failed } else { ShortStageStatus::Completed },
            input_hash: hash_present(vec![(
                "salesPackage",
                // TS `salesPackage ?? null`：键恒在，缺包装时值为 null。
                Some(
                    sales_package
                        .as_ref()
                        .map(|package| serde_json::to_value(package).unwrap_or(Value::Null))
                        .unwrap_or(Value::Null),
                ),
            )]),
            request_hash: None,
            review_hash: None,
            reviewed_artifact: None,
            reviewed_reference: None,
            comparison: None,
            updated_at: utc_now_iso(),
            observations: if let Some(error) = &cover_error {
                vec![Observation {
                    code: cover_error_code.clone().unwrap_or_else(|| "cover-generation".into()),
                    summary: error.clone(),
                    evidence: Vec::new(),
                    category: Some(ObservationCategory::Execution),
                    assessment: Some(ObservationAssessment::Unavailable),
                    scope: None,
                    target_hash: None,
                    target: None,
                    source_refs: None,
                }]
            } else {
                Vec::new()
            },
            error: cover_error.clone(),
        });
        write_short_production_state(root, &base_dir, &production_state)
            .await
            .map_err(|message| ShortRunError::new("PRODUCTION_STATE_INVALID", message))?;
    }
    refresh_delivery(
        root,
        &base_dir,
        &mut production_state,
        &final_draft,
        &outline_markdown,
        &work_title,
        opening_hook_chars,
        &minimum,
        language,
    )
    .await?;
    let observations: Vec<Observation> = [
        production_state.stages.review.as_ref().map(|stage| stage.observations.clone()),
        production_state.stages.package.as_ref().map(|stage| stage.observations.clone()),
        production_state.stages.cover.as_ref().map(|stage| stage.observations.clone()),
    ]
    .into_iter()
    .flatten()
    .flatten()
    .chain(
        production_state
            .delivery
            .as_ref()
            .map(|delivery| delivery.observations.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|observation| SHORT_DELIVERY_CONTRACT_CODES.contains(&observation.code.as_str())),
    )
    .collect();
    let changed = changed_work_source_paths(root, &story_id, &source_before).await?;
    let mut accept_paths = short_manuscript_paths(&final_draft);
    if let Some(previous_title) = &previous_work_title {
        if let Some(previous_path) = short_title_path(previous_title) {
            if short_title_path(&final_draft.story_title).as_deref() != Some(previous_path.as_str()) {
                accept_paths.push(previous_path);
            }
        }
    }
    accept_paths.extend(changed);
    sync_work_source_artifacts(SyncWorkSourceInput {
        project_root: root,
        work_id: &story_id,
        accept: true,
        accept_paths: Some(accept_paths),
        episode_id: None,
        updated_at: None,
        writes: Vec::new(),
        title: Some(&final_draft.story_title),
        status: None,
    })
    .await?;
    Ok(ProduceOutcome::Run(build_short_run_result(
        &story_id,
        &base_dir,
        observations,
        cover_image_path,
        package_warning,
        cover_error,
        Some(production_state.stages.clone()),
        production_state.delivery.clone(),
    )))
}

// ── stage acceptPaths 族 + 交付刷新 ─────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn stage_result(
    root: &Path,
    story_id: &str,
    base_dir: &Path,
    work_title: &str,
    previous_work_title: Option<&str>,
    source_before: &BTreeMap<String, String>,
    stage: ShortProductionStage,
    mut paths: Vec<String>,
    production_state: &ShortProductionState,
    final_draft: Option<&ShortFictionBatchDraft>,
    revised_draft: bool,
) -> Result<ShortProductionStageResult, ShortRunError> {
    if stage != ShortProductionStage::Outline {
        // refreshDelivery 已在 produce_short 阶段间调用（此处状态已带 delivery）。
    }
    let mut accept_paths: Vec<String> = match stage {
        ShortProductionStage::Review => {
            let mut paths = vec![
                "source/reviews/draft-v001.md".into(),
                "source/reviews/draft-warning.md".into(),
                "source/production-state.json".into(),
            ];
            if revised_draft {
                if let Some(final_draft) = final_draft {
                    paths.extend(short_manuscript_paths(final_draft));
                }
                paths.extend(changed_work_source_paths(root, story_id, source_before).await?);
            }
            paths
        }
        ShortProductionStage::Package => vec![
            "source/final/sales-package.json".into(),
            "source/final/sales-package.md".into(),
            "source/final/cover-prompt.md".into(),
            "source/reviews/package-v001.md".into(),
            "source/reviews/package-warning.md".into(),
            "source/production-state.json".into(),
        ],
        ShortProductionStage::Draft => {
            let mut paths = Vec::new();
            if let Some(final_draft) = final_draft {
                paths.extend(short_manuscript_paths(final_draft));
            }
            paths.extend(changed_work_source_paths(root, story_id, source_before).await?);
            paths
        }
        ShortProductionStage::Outline => vec![
            "source/outline/v001.md".into(),
            "source/production-state.json".into(),
        ],
    };
    if let Some(previous_title) = previous_work_title {
        if previous_title != work_title {
            if let Some(previous_path) = short_title_path(previous_title) {
                accept_paths.push(previous_path);
            }
        }
    }
    let title = match (stage, final_draft) {
        (ShortProductionStage::Outline, _) => work_title.to_string(),
        (_, Some(final_draft)) => final_draft.story_title.clone(),
        _ => work_title.to_string(),
    };
    sync_work_source_artifacts(SyncWorkSourceInput {
        project_root: root,
        work_id: story_id,
        accept: true,
        accept_paths: Some(accept_paths),
        episode_id: None,
        updated_at: None,
        writes: Vec::new(),
        title: Some(&title),
        status: None,
    })
    .await?;
    let stage_status = match (stage, production_state.stages.review.as_ref(), production_state.stages.package.as_ref()) {
        (ShortProductionStage::Review, Some(review), _) if review.status == ShortStageStatus::Failed => ShortStageStatus::Failed,
        (ShortProductionStage::Package, _, Some(package)) if package.status == ShortStageStatus::Failed => ShortStageStatus::Failed,
        _ => ShortStageStatus::Completed,
    };
    let observations = match stage {
        ShortProductionStage::Review => production_state.stages.review.as_ref().map(|stage| stage.observations.clone()).unwrap_or_default(),
        ShortProductionStage::Package => production_state.stages.package.as_ref().map(|stage| stage.observations.clone()).unwrap_or_default(),
        _ => Vec::new(),
    };
    let delivery_observations = production_state
        .delivery
        .as_ref()
        .map(|delivery| delivery.observations.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|observation| SHORT_DELIVERY_CONTRACT_CODES.contains(&observation.code.as_str()))
        .collect::<Vec<_>>();
    let reviewed_artifact = if stage == ShortProductionStage::Review && stage_status == ShortStageStatus::Completed {
        production_state
            .stages
            .review
            .as_ref()
            .and_then(|review| review.reviewed_artifact.clone())
            .map(|value| ShortReviewedArtifact { artifact_id: value.artifact_id, revision_id: value.revision_id })
    } else {
        None
    };
    Ok(ShortProductionStageResult {
        story_id: story_id.to_string(),
        stage,
        stage_status,
        observations: observations.into_iter().chain(delivery_observations).collect(),
        artifact_paths: paths
            .drain(..)
            .map(|path| to_project_path(&base_dir.join(&path).to_string_lossy()))
            .collect(),
        cover_required: production_state.target.as_ref().and_then(|target| target.cover_required),
        delivery: production_state.delivery.clone(),
        reviewed_artifact,
    })
}

/// TS `refreshDelivery`：交付合同观察（长度/标题/开篇 + 已核验 review issues）。
#[allow(clippy::too_many_arguments)]
async fn refresh_delivery(
    root: &Path,
    base_dir: &Path,
    production_state: &mut ShortProductionState,
    final_draft: &ShortFictionBatchDraft,
    outline_markdown: &str,
    work_title: &str,
    opening_hook_chars: Option<u32>,
    minimum: &ShortDraftMinimum,
    language: WritingLanguage,
) -> Result<(), ShortRunError> {
    let input_hash = hash_present(vec![
        ("draft", Some(serde_json::to_value(final_draft).unwrap_or(Value::Null))),
        ("outline", Some(Value::String(outline_markdown.to_string()))),
        ("intent", Some(Value::String(production_state.intent.clone()))),
    ]);
    let review = production_state.stages.review.clone();
    let verified = review
        .as_ref()
        .map(|review| review.status == ShortStageStatus::Completed && review.input_hash == input_hash)
        .unwrap_or(false);
    let mode = crate::utils::length_metrics::resolve_length_counting_mode(language);
    let length_issues: Vec<Observation> = find_incomplete_short_fiction_chapters(
        final_draft,
        &ShortDraftBounds {
            title: None,
            opening_hook_chars: None,
            expected_chapters: None,
            min_chapter_length: Some(minimum.min_chapter_length),
            max_chapter_length: minimum.max_chapter_length,
            language: Some(language),
        },
    )
    .into_iter()
    .map(|number| Observation {
        code: "SHORT_CHAPTER_CONTRACT".into(),
        summary: format!("Chapter {number} does not satisfy the configured manuscript length or text contract."),
        evidence: vec![to_project_path(&base_dir.join(format!("final/chapters/{:04}.md", number)).to_string_lossy())],
        category: Some(ObservationCategory::Quality),
        assessment: Some(ObservationAssessment::Issue),
        scope: Some(format!("chapter:{number}")),
        target_hash: Some(input_hash.clone()),
        target: None,
        source_refs: None,
    })
    .collect();
    let measurements = measure_short_fiction_draft(final_draft, language);
    let opening_hook_length = measurements.opening_hook_length;
    let mut contract_issues = length_issues;
    if final_draft.story_title != work_title {
        contract_issues.push(Observation {
            code: "SHORT_TITLE_MISMATCH".into(),
            summary: "The manuscript title differs from the confirmed title.".into(),
            evidence: vec![to_project_path(&base_dir.join("final/short-story.json").to_string_lossy())],
            category: Some(ObservationCategory::Quality),
            assessment: Some(ObservationAssessment::Issue),
            scope: None,
            target_hash: Some(input_hash.clone()),
            target: None,
            source_refs: None,
        });
    }
    if let Some(target) = opening_hook_chars {
        let minimum_length = (target as f64 * 0.75).floor() as u32;
        let maximum_length = (target as f64 * 1.25).ceil() as u32;
        if opening_hook_length < minimum_length || opening_hook_length > maximum_length {
            contract_issues.push(Observation {
                code: "SHORT_OPENING_HOOK_CONTRACT".into(),
                summary: "The independent opening scene does not satisfy the requested length.".into(),
                evidence: vec![to_project_path(&base_dir.join("final/short-story.json").to_string_lossy())],
                category: Some(ObservationCategory::Quality),
                assessment: Some(ObservationAssessment::Issue),
                scope: None,
                target_hash: Some(input_hash.clone()),
                target: None,
                source_refs: None,
            });
        }
    }
    let _ = mode;
    let observations = contract_issues.into_iter().chain(
        if verified {
            review
                .as_ref()
                .map(|review| {
                    review
                        .observations
                        .iter()
                        .filter(|observation| observation.assessment == Some(ObservationAssessment::Issue))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        },
    ).collect::<Vec<_>>();
    let status = if !observations.is_empty() {
        crate::production::short_fiction::ShortDeliveryStatus::NeedsRevision
    } else if verified {
        crate::production::short_fiction::ShortDeliveryStatus::ChecksPassed
    } else {
        crate::production::short_fiction::ShortDeliveryStatus::Unverified
    };
    production_state.delivery = Some(ShortDelivery {
        status,
        input_hash,
        observations,
        measurements: Some(measurements),
        target: production_state
            .target
            .as_ref()
            .map(|target| serde_json::to_value(target).unwrap_or(Value::Null))
            .and_then(|value| value.as_object().cloned())
            .map(|map| map.into_iter().collect::<std::collections::BTreeMap<String, Value>>()),
    });
    write_short_production_state(root, base_dir, production_state)
        .await
        .map_err(|message| ShortRunError::new("PRODUCTION_STATE_INVALID", message))
}

// ── 产物落盘（TS writeDraftArtifacts/writeFinalArtifacts/writePackageArtifacts）──

/// TS `textWrite`：trimEnd + "\n"。
fn text_write(relative_path: String, value: &str) -> AtomicFileWrite {
    let mut content = value.trim_end().to_string();
    content.push('\n');
    AtomicFileWrite { relative_path, content: FileContent::Text(content) }
}

/// 单文本原子写（TS writeText：safeChildPath + mkdir + writeTextAtomic）。
async fn write_text(root: &Path, path: &Path, value: &str) -> Result<(), ShortRunError> {
    let resolved = safe_child_path(&root.to_string_lossy(), &path.to_string_lossy())
        .map_err(|message| ShortRunError::new("PATH_UNSAFE", message))?;
    let mut content = value.trim_end().to_string();
    content.push('\n');
    commit_atomic_file_set(&AtomicFileSet {
        root_dir: root,
        writes: vec![AtomicFileWrite {
            relative_path: relative_from_root(root, &resolved),
            content: FileContent::Text(content),
        }],
        deletes: Vec::new(),
    })
    .await
    .map_err(|error| ShortRunError::new("ATOMIC_WRITE_FAILED", error.to_string()))
}

async fn commit_delete(root: &Path, path: &Path) -> Result<(), ShortRunError> {
    let resolved = safe_child_path(&root.to_string_lossy(), &path.to_string_lossy())
        .map_err(|message| ShortRunError::new("PATH_UNSAFE", message))?;
    commit_atomic_file_set(&AtomicFileSet {
        root_dir: root,
        writes: Vec::new(),
        deletes: vec![relative_from_root(root, &resolved)],
    })
    .await
    .map_err(|error| ShortRunError::new("ATOMIC_WRITE_FAILED", error.to_string()))
}

fn relative_from_root(root: &Path, resolved: &Path) -> String {
    resolved
        .strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| resolved.to_string_lossy().replace('\\', "/"))
}

async fn read_text(root: &Path, path: &Path) -> Result<String, ShortRunError> {
    let resolved = safe_child_path(&root.to_string_lossy(), &path.to_string_lossy())
        .map_err(|message| ShortRunError::new("PATH_UNSAFE", message))?;
    tokio::fs::read_to_string(&resolved)
        .await
        .map_err(|error| ShortRunError::new("READ_FAILED", error.to_string()))
}

async fn read_binary(root: &Path, path: &Path) -> Result<Vec<u8>, ShortRunError> {
    let resolved = safe_child_path(&root.to_string_lossy(), &path.to_string_lossy())
        .map_err(|message| ShortRunError::new("PATH_UNSAFE", message))?;
    tokio::fs::read(&resolved)
        .await
        .map_err(|error| ShortRunError::new("READ_FAILED", error.to_string()))
}

/// TS `projectFileExists`。
async fn project_file_exists(root: &Path, path: &str) -> Result<bool, ShortRunError> {
    let resolved = safe_child_path(&root.to_string_lossy(), path)
        .map_err(|message| ShortRunError::new("PATH_UNSAFE", message))?;
    Ok(resolved.is_file())
}

/// TS `tryReadProjectText`。
async fn try_read_project_text(root: &Path, path: &str) -> Result<Option<String>, ShortRunError> {
    let resolved = safe_child_path(&root.to_string_lossy(), path)
        .map_err(|message| ShortRunError::new("PATH_UNSAFE", message))?;
    match tokio::fs::read_to_string(&resolved).await {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ShortRunError::new("READ_FAILED", error.to_string())),
    }
}

/// TS `tryReadShortFictionDraft`（strict parse → None/Err）。
async fn try_read_short_fiction_draft(
    root: &Path,
    path: &str,
) -> Result<Option<ShortFictionBatchDraft>, ShortRunError> {
    let Some(raw) = try_read_project_text(root, path).await? else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&raw)
        .map_err(|error| ShortRunError::new("SHORT_DRAFT_INVALID", format!("{path}: {error}")))?;
    let draft: ShortFictionBatchDraft = serde_json::from_value(value.clone())
        .map_err(|error| ShortRunError::new("SHORT_DRAFT_INVALID", format!("{path}: {error}")))?;
    draft
        .validate_shape()
        .map_err(|message| ShortRunError::new("SHORT_DRAFT_INVALID", format!("{path}: {message}")))?;
    Ok(Some(draft))
}

/// TS `writeDraftArtifacts`：full.md + draft.json + 章节文件 + 淘汰删除。
async fn write_draft_artifacts(
    root: &Path,
    base_dir: &Path,
    version: &str,
    draft: &ShortFictionBatchDraft,
    language: WritingLanguage,
) -> Result<(), ShortRunError> {
    let draft_dir = base_dir.join("drafts").join(version);
    let completed_chapters: Vec<&ShortFictionChapter> = draft
        .chapters
        .iter()
        .filter(|chapter| !chapter.title.trim().is_empty() && !chapter.content.trim().is_empty())
        .collect();
    let markdown = render_short_fiction_draft_markdown(draft, language)
        .map_err(|error| ShortRunError { code: error.code.into(), message: error.message, recovery: None })?;
    let mut writes = vec![
        text_write(format!("{}/drafts/{version}/full.md", posix_base(base_dir)), &markdown),
        text_write(
            format!("{}/drafts/{version}/draft.json", posix_base(base_dir)),
            &serde_json::to_string_pretty(draft).unwrap_or_default(),
        ),
    ];
    for chapter in &completed_chapters {
        writes.push(text_write(
            format!("{}/drafts/{version}/chapters/{:04}.md", posix_base(base_dir), chapter.number),
            &format!(
                "# {}\n\n{}",
                crate::production::short_fiction::format_short_fiction_chapter_heading(chapter.number, &chapter.title, language)
                    .map_err(|error| ShortRunError { code: error.code.into(), message: error.message, recovery: None })?,
                chapter.content
            ),
        ));
    }
    let deletes = obsolete_chapter_files(root, &draft_dir.join("chapters"), &completed_chapters.iter().map(|chapter| chapter.number).collect::<Vec<_>>()).await?;
    commit_atomic_file_set(&AtomicFileSet { root_dir: root, writes, deletes })
        .await
        .map_err(|error| ShortRunError::new("ATOMIC_WRITE_FAILED", error.to_string()))
}

/// TS `writeFinalArtifacts`：final 四件 + 标题别名 + 前标题淘汰。
async fn write_final_artifacts(
    root: &Path,
    base_dir: &Path,
    draft: &ShortFictionBatchDraft,
    language: WritingLanguage,
    previous_title: Option<&str>,
) -> Result<(), ShortRunError> {
    let final_dir = base_dir.join("final");
    let markdown = render_short_fiction_draft_markdown(draft, language)
        .map_err(|error| ShortRunError { code: error.code.into(), message: error.message, recovery: None })?;
    let named_path = short_title_path(&draft.story_title);
    let previous_path = previous_title.and_then(short_title_path);
    let relative_alias = |path: &str| format!("{}/{}", posix_base(base_dir), path.strip_prefix("source/").unwrap_or(path));
    let mut writes = vec![
        text_write(format!("{}/final/full.md", posix_base(base_dir)), &markdown),
        text_write(
            format!("{}/final/short-story.json", posix_base(base_dir)),
            &serde_json::to_string_pretty(draft).unwrap_or_default(),
        ),
    ];
    if let Some(named_path) = &named_path {
        writes.push(text_write(relative_alias(named_path), &markdown));
    }
    for chapter in &draft.chapters {
        writes.push(text_write(
            format!("{}/final/chapters/{:04}.md", posix_base(base_dir), chapter.number),
            &format!(
                "# {}\n\n{}",
                crate::production::short_fiction::format_short_fiction_chapter_heading(chapter.number, &chapter.title, language)
                    .map_err(|error| ShortRunError { code: error.code.into(), message: error.message, recovery: None })?,
                chapter.content
            ),
        ));
    }
    let numbers: Vec<u32> = draft.chapters.iter().map(|chapter| chapter.number).collect();
    let mut deletes = obsolete_chapter_files(root, &final_dir.join("chapters"), &numbers).await?;
    if let (Some(previous_path), Some(named_path)) = (&previous_path, &named_path) {
        if previous_path != named_path {
            deletes.push(relative_alias(previous_path));
        }
    }
    commit_atomic_file_set(&AtomicFileSet { root_dir: root, writes, deletes })
        .await
        .map_err(|error| ShortRunError::new("ATOMIC_WRITE_FAILED", error.to_string()))
}

/// TS `writePackageArtifacts`：sales 三件。
async fn write_package_artifacts(
    root: &Path,
    base_dir: &Path,
    sales_package: &ShortFictionSalesPackage,
    language: WritingLanguage,
) -> Result<(), ShortRunError> {
    let package_markdown = render_short_fiction_sales_package(sales_package, language);
    let writes = vec![
        text_write(
            format!("{}/final/sales-package.json", posix_base(base_dir)),
            &serde_json::to_string_pretty(sales_package).unwrap_or_default(),
        ),
        text_write(format!("{}/final/sales-package.md", posix_base(base_dir)), &package_markdown),
        text_write(format!("{}/final/cover-prompt.md", posix_base(base_dir)), &sales_package.cover_prompt),
    ];
    commit_atomic_file_set(&AtomicFileSet { root_dir: root, writes, deletes: Vec::new() })
        .await
        .map_err(|error| ShortRunError::new("ATOMIC_WRITE_FAILED", error.to_string()))
}

/// TS `obsoleteChapterFiles`：章节目录中不在当前集合的 `\d+.md`。
async fn obsolete_chapter_files(root: &Path, directory: &Path, chapters: &[u32]) -> Result<Vec<String>, ShortRunError> {
    let current: BTreeSet<String> = chapters.iter().map(|number| format!("{number:04}.md")).collect();
    let resolved = safe_child_path(&root.to_string_lossy(), &directory.to_string_lossy())
        .map_err(|message| ShortRunError::new("PATH_UNSAFE", message))?;
    let mut entries = match tokio::fs::read_dir(&resolved).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ShortRunError::new("READ_FAILED", error.to_string())),
    };
    let mut obsolete = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| ShortRunError::new("READ_FAILED", error.to_string()))?
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_numbered = !name.is_empty()
            && name.ends_with(".md")
            && name[..name.len() - 3].chars().all(|character| character.is_ascii_digit())
            && name[..name.len() - 3].chars().next().map(|character| character.is_ascii_digit()).unwrap_or(false);
        if is_numbered && !current.contains(&name) {
            obsolete.push(relative_from_root(root, &entry.path()));
        }
    }
    obsolete.sort();
    Ok(obsolete)
}

/// TS `shortTitlePath`：`source/final/<safe>.md`（保留名 → None）。
pub fn short_title_path(title: &str) -> Option<String> {
    let name = format!("{}.md", safe_file_name(title));
    if ["full.md", "sales-package.md", "cover-prompt.md"].contains(&name.to_lowercase().as_str()) {
        return None;
    }
    Some(format!("source/final/{name}"))
}

/// TS `shortManuscriptPaths`。
pub fn short_manuscript_paths(draft: &ShortFictionBatchDraft) -> Vec<String> {
    let mut paths = vec![
        "source/outline/v001.md".into(),
        "source/final/full.md".into(),
        "source/final/short-story.json".into(),
    ];
    if let Some(named) = short_title_path(&draft.story_title) {
        paths.push(named);
    }
    paths.extend(
        draft
            .chapters
            .iter()
            .map(|chapter| format!("source/final/chapters/{:04}.md", chapter.number)),
    );
    paths
}

// ── 结果构造 ─────────────────────────────────────────────────────────

/// TS `renderShortFictionReview`。
pub fn render_short_fiction_review(
    summary: &str,
    observations: &[Observation],
    language: WritingLanguage,
    kind: &str,
) -> String {
    let en = language == WritingLanguage::En;
    let observation_blocks: Vec<String> = if observations.is_empty() {
        vec![if en { "No evidence-backed observations." } else { "没有有证据的审稿观察。" }.into()]
    } else {
        observations
            .iter()
            .flat_map(|observation| {
                let mut block = vec![format!("### {}", observation.code), observation.summary.clone()];
                if !observation.evidence.is_empty() {
                    block.push(String::new());
                    block.push(if en { "Evidence:" } else { "证据：" }.into());
                    for item in &observation.evidence {
                        block.push(format!("- {item}"));
                    }
                }
                if let Some(source_refs) = &observation.source_refs {
                    if !source_refs.is_empty() {
                        block.push(String::new());
                        block.push(if en { "Sources:" } else { "来源：" }.into());
                        for source in source_refs {
                            block.push(source.source_id.clone());
                            block.push(format!("\n> {}", source.quote.replace('\n', "\n> ")));
                        }
                    }
                }
                block.push(String::new());
                block
            })
            .collect()
    };
    let heading = match kind {
        "package" => {
            if en {
                "# Sales package review"
            } else {
                "# 销售包装审查"
            }
        }
        _ => {
            if en {
                "# Draft review"
            } else {
                "# 成稿审查"
            }
        }
    };
    let observations_heading = if en { "## Observations" } else { "## 审稿观察" };
    let mut parts = vec![heading.to_string(), String::new(), summary.to_string(), String::new(), observations_heading.to_string(), String::new()];
    parts.extend(observation_blocks);
    parts.join("\n").trim().to_string()
}

/// TS `buildShortRunResult`。
#[allow(clippy::too_many_arguments)]
pub fn build_short_run_result(
    story_id: &str,
    base_dir: &Path,
    observations: Vec<Observation>,
    cover_image_path: Option<String>,
    package_error: Option<String>,
    cover_error: Option<String>,
    stage_results: Option<ShortProductionStages>,
    delivery: Option<ShortDelivery>,
) -> ShortFictionRunResult {
    let reviewed_artifact = stage_results
        .as_ref()
        .and_then(|stages| stages.review.as_ref())
        .filter(|review| {
            review.status == ShortStageStatus::Completed
                && delivery
                    .as_ref()
                    .map(|delivery| review.input_hash == delivery.input_hash)
                    .unwrap_or(false)
        })
        .and_then(|review| review.reviewed_artifact.clone())
        .map(|value| ShortReviewedArtifact { artifact_id: value.artifact_id, revision_id: value.revision_id });
    ShortFictionRunResult {
        story_id: story_id.to_string(),
        stage_results,
        reviewed_artifact,
        delivery,
        observations,
        outline_path: to_project_path(&base_dir.join("outline/v001.md").to_string_lossy()),
        draft_review_path: to_project_path(&base_dir.join("reviews/draft-v001.md").to_string_lossy()),
        final_markdown_path: to_project_path(&base_dir.join("final/full.md").to_string_lossy()),
        final_json_path: to_project_path(&base_dir.join("final/short-story.json").to_string_lossy()),
        sales_package_path: to_project_path(&base_dir.join("final/sales-package.md").to_string_lossy()),
        cover_prompt_path: to_project_path(&base_dir.join("final/cover-prompt.md").to_string_lossy()),
        cover_image_path,
        cover_error,
        package_error,
    }
}

// ── 小件（TS helpers L1691-1773）─────────────────────────────────────

/// TS `shortWorkBaseDir`。
pub fn short_work_base_dir(story_id: &str) -> PathBuf {
    PathBuf::from(format!("works/{}/source", safe_segment(story_id)))
}

/// TS `ensureShortWork`：缺则建（profile short-fiction）。
pub async fn ensure_short_work(
    root: &Path,
    story_id: &str,
    title: &str,
    language: WritingLanguage,
) -> Result<(), ShortRunError> {
    if load_work_manifest(root, story_id).await.is_ok() {
        return Ok(());
    }
    let manifest = create_work_manifest(&CreateWorkManifestInput {
        id: story_id,
        title,
        profile_id: "short-fiction",
        language: language_code(language),
        status: None,
        now: &utc_now_iso(),
    })
    .map_err(ShortRunError::from)?;
    save_work_manifest(root, &manifest).await.map_err(ShortRunError::from)
}

/// TS `positiveInteger`。
fn positive_integer(value: Option<u32>, fallback: u32, name: &str) -> Result<u32, ShortRunError> {
    let parsed = value.unwrap_or(fallback);
    if parsed < 1 {
        return Err(ShortRunError::new("SHORT_OPTION_INVALID", format!("{name} must be a positive integer.")));
    }
    Ok(parsed)
}

fn slugify(value: &str) -> String {
    let lowered: String = value
        .to_lowercase()
        .chars()
        .filter(|character| *character != '\'' && *character != '"')
        .collect();
    let mut slug = String::new();
    let mut previous_separator = true;
    for character in lowered.chars() {
        if character.is_alphabetic() || character.is_numeric() {
            slug.push(character);
            previous_separator = false;
        } else if !previous_separator {
            slug.push('-');
            previous_separator = true;
        }
    }
    let slug = slug.trim_matches('-').chars().take(60).collect::<String>();
    if slug.is_empty() {
        format!("short-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_millis()).unwrap_or(0))
    } else {
        slug
    }
}

/// TS `safeSegment`。
pub fn safe_segment(value: &str) -> String {
    let mut cleaned = String::new();
    for character in value.chars() {
        if matches!(character, '\\' | '/' | ':' | '\0' | '*' | '?' | '"' | '<' | '>' | '|')
            || character.is_whitespace()
        {
            cleaned.push('-');
        } else {
            cleaned.push(character);
        }
    }
    let cleaned: String = cleaned.trim_matches('-').chars().take(80).collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return format!("short-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_millis()).unwrap_or(0));
    }
    cleaned
}

/// TS `safeFileName`。
pub fn safe_file_name(value: &str) -> String {
    let mut cleaned = String::new();
    for character in value.chars() {
        if matches!(character, '\\' | '/' | ':' | '\0' | '*' | '?' | '"' | '<' | '>' | '|') {
            cleaned.push('_');
        } else if character.is_whitespace() {
            cleaned.push(' ');
        } else {
            cleaned.push(character);
        }
    }
    let cleaned = cleaned.trim().chars().take(80).collect::<String>();
    if cleaned.is_empty() {
        "short-fiction".into()
    } else {
        cleaned
    }
}

/// TS `JSON.parse(savedPackage) as ShortFictionSalesPackage`：信任已登记文档，
/// 四字段抽取 + 额外键容忍（TypeBox 无 additionalProperties 同语义）。
fn package_from_saved_value(value: &Value) -> Option<ShortFictionSalesPackage> {
    let map = value.as_object()?;
    Some(ShortFictionSalesPackage {
        title: map.get("title")?.as_str()?.to_string(),
        intro: map.get("intro")?.as_str()?.to_string(),
        selling_points: map
            .get("sellingPoints")?
            .as_array()?
            .iter()
            .filter_map(|point| point.as_str().map(str::to_string))
            .collect(),
        cover_prompt: map.get("coverPrompt")?.as_str()?.to_string(),
        raw_content: String::new(),
    })
}

/// WritingLanguage → serde 字面量（"zh"/"en"）。
fn language_code(language: WritingLanguage) -> &'static str {
    match language {
        WritingLanguage::Zh => "zh",
        WritingLanguage::En => "en",
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
}

/// TS `projectPath`（toPosixPath）。
fn to_project_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// project 根相对 posix 路径（base_dir 为 "works/<id>/source" 形态）。
fn posix_base(base_dir: &Path) -> String {
    base_dir.to_string_lossy().replace('\\', "/")
}

/// 续读 outline 的立即包装（保持调用点扁平）。
async fn futures_now_read(root: &Path, path: &Path) -> Option<String> {
    let resolved = safe_child_path(&root.to_string_lossy(), &path.to_string_lossy()).ok()?;
    tokio::fs::read_to_string(&resolved).await.ok()
}

impl ShortRunError {
    fn new_state_read(message: String) -> Self {
        Self::new("PRODUCTION_STATE_INVALID", message)
    }
}

impl ShortFictionBatchDraft {
    /// TS zod strict 等价的形状卫兵（deny_unknown_fields 已覆盖未知键；此处补
    /// 章号/字数段不变量——schema refine 面）。
    fn validate_shape(&self) -> Result<(), String> {
        if self.story_title.trim().is_empty() {
            return Err("storyTitle must contain text".into());
        }
        for chapter in &self.chapters {
            if chapter.number == 0 {
                return Err("chapters[].number must be a positive integer".into());
            }
            if chapter.content.contains('\u{FFFD}') {
                return Err("chapters[].content contains corrupted characters (U+FFFD)".into());
            }
        }
        Ok(())
    }
}

// 导出面重导出于 short_fiction.rs（ShortStageComparison 直接 use 消费）。

// ── reviseShortFictionProduction（TS L742-926 checkpoint 编排，893 号 M3e）──

/// TS `ShortRevisionCheckpoint`（.inkos/short-revisions/<workId>.json；
/// deny_unknown_fields ↔ TypeBox 推断面等价——checkpoint 为引擎本地面）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortRevisionCheckpoint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(rename = "inputHash")]
    pub input_hash: String,
    #[serde(rename = "sourceHash", skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
    #[serde(rename = "authorScope", skip_serializing_if = "Option::is_none")]
    pub author_scope: Option<crate::production::short_fiction_tools::ShortAuthorScope>,
    #[serde(rename = "authorRequest", skip_serializing_if = "Option::is_none")]
    pub author_request: Option<String>,
    #[serde(rename = "authorSource", skip_serializing_if = "Option::is_none")]
    pub author_source: Option<ShortReviewedArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<ShortRevisionRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<ShortRevisionProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalization: Option<ShortRevisionFinalization>,
}

/// TS `request` 面（仅创作入参——runtime 上下文绝不序列化，TS 注释逐字）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortRevisionRequest {
    pub direction: String,
    #[serde(rename = "chapterCount", skip_serializing_if = "Option::is_none")]
    pub chapter_count: Option<u32>,
    #[serde(rename = "charsPerChapter", skip_serializing_if = "Option::is_none")]
    pub chars_per_chapter: Option<u32>,
    #[serde(rename = "minChapterLength", skip_serializing_if = "Option::is_none")]
    pub min_chapter_length: Option<u32>,
    #[serde(rename = "maxChapterLength", skip_serializing_if = "Option::is_none")]
    pub max_chapter_length: Option<u32>,
    #[serde(rename = "openingHookChars", skip_serializing_if = "Option::is_none")]
    pub opening_hook_chars: Option<u32>,
    #[serde(rename = "revisionChapterNumbers", skip_serializing_if = "Option::is_none")]
    pub revision_chapter_numbers: Option<Vec<u32>>,
}

/// TS `ShortRevisionProgress`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortRevisionProgress {
    pub plan: crate::production::short_fiction::ShortRevisionPlan,
    pub draft: ShortFictionBatchDraft,
    pub completed: Vec<u32>,
}

/// TS `finalization` 面：基线（不重写章节/不重置基线）与结果草稿。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortRevisionFinalization {
    pub baseline: ShortRevisionSnapshot,
    pub result: ShortRevisionSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortRevisionSnapshot {
    pub draft: ShortFictionBatchDraft,
    #[serde(rename = "outlineMarkdown")]
    pub outline_markdown: String,
}

/// TS `ShortFictionRevisionResult`（ShortFictionRunResult 去 sales/cover 面）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortFictionRevisionResult {
    pub story_id: String,
    pub delivery: Option<ShortDelivery>,
    pub reviewed_artifact: Option<ShortReviewedArtifact>,
    pub observations: Vec<Observation>,
    pub revision_changes: ShortRevisionChanges,
    pub outline_path: String,
    pub draft_review_path: String,
    pub final_markdown_path: String,
    pub final_json_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortRevisionChanges {
    #[serde(rename = "chapterNumbers")]
    pub chapter_numbers: Vec<u32>,
    #[serde(rename = "openingChanged")]
    pub opening_changed: bool,
    #[serde(rename = "titleChanged")]
    pub title_changed: bool,
    #[serde(rename = "outlineChanged")]
    pub outline_changed: bool,
}

/// TS `selectAuthorScope`（short-revision-scope.ts L202-205）：双 system 逐字 +
/// 章号存在性 validate；professionalGuidance=false（authorRequest 前置不注入）。
pub async fn select_author_scope(
    agents: &crate::production::short_fiction_agents::ShortAgents,
    draft: &ShortFictionBatchDraft,
    author_request: &str,
) -> Result<crate::production::short_fiction_tools::ShortAuthorScope, ShortRunError> {
    let (messages, tool, validate) =
        crate::production::short_fiction_tools::short_author_scope_request(draft, author_request);
    let validate = Arc::new(validate);
    let validate_for_run: crate::production::structured_agent::ValidateFn = Arc::new(move |value: Value| {
        let validate = Arc::clone(&validate);
        Box::pin(async move {
            let scope: crate::production::short_fiction_tools::ShortAuthorScope =
                serde_json::from_value(value.clone()).map_err(|error| {
                    crate::production::structured_agent::StructuredToolError::coded(
                        "WORKER_SCHEMA_INVALID",
                        error.to_string(),
                    )
                })?;
            validate.validate(&scope)?;
            Ok(value)
        })
    });
    let outcome = crate::production::structured_agent::run_structured_tool(
        Arc::clone(&agents.chat),
        &agents.model,
        // professionalGuidance=false：authorRequest 前置不注入（TS 同参语义）。
        &agents.messages_with_prelude(messages, false),
        agents.defaults.clone(),
        tool,
        Some(validate_for_run),
        Vec::new(),
        crate::production::structured_agent::StructuredRunOptions {
            temperature: None,
            max_tokens: Some(std::cmp::min(4096, agents.defaults.max_tokens)),
        },
    )
    .await
    .map_err(|error| ShortRunError { code: error.code, message: error.message, recovery: None })?;
    serde_json::from_value(outcome.result)
        .map_err(|error| ShortRunError::new("WORKER_SCHEMA_INVALID", error.to_string()))
}

/// TS 原始 sha256 输入串（revisionPlanVersion 7——TS 字面量键序，BTreeMap 排序
/// 不可用；手动按序拼接）。值序列化 = serde_json to_string。
fn ordered_hash_object(pairs: Vec<(&str, Value)>) -> String {
    let body = pairs
        .iter()
        .map(|(key, value)| format!("\"{}\":{}", key, value))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{body}}}")
}

/// TS `revisionPlanVersion 7` 输入 hash（原始 sha256——非 canonical 面备案）。
#[allow(clippy::too_many_arguments)]
fn revision_input_hash(
    draft: &ShortFictionBatchDraft,
    outline_markdown: &str,
    review: &str,
    direction: &str,
    chars_target: Option<u32>,
    chapter_count: u32,
    chapter_numbers: Option<&[u32]>,
    minimum: &ShortDraftMinimum,
) -> String {
    let text = ordered_hash_object(vec![
        ("revisionPlanVersion", Value::from(7)),
        ("draft", serde_json::to_value(draft).unwrap_or(Value::Null)),
        ("outlineMarkdown", Value::String(outline_markdown.to_string())),
        ("review", Value::String(review.to_string())),
        ("direction", Value::String(direction.to_string())),
        (
            "target",
            chars_target.map(Value::from).unwrap_or(Value::Null),
        ),
        ("chapterCount", Value::from(chapter_count)),
        (
            "chapterNumbers",
            chapter_numbers
                .map(|numbers| serde_json::to_value(numbers).unwrap_or(Value::Null))
                .unwrap_or(Value::Null),
        ),
        ("minimum", serde_json::to_value(minimum).unwrap_or(Value::Null)),
    ]);
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
}

/// TS `recovery(checkpoint)`：续跑动作 payload（workId/checkpointPath 由调用点注入）。
fn revision_recovery_with(
    work_id: &str,
    checkpoint_path: &str,
    checkpoint: &ShortRevisionCheckpoint,
) -> Value {
    let pending: Vec<u32> = checkpoint
        .progress
        .as_ref()
        .map(|progress| {
            progress
                .plan
                .chapters
                .iter()
                .filter(|chapter| !progress.completed.contains(&chapter.number))
                .map(|chapter| chapter.number)
                .collect()
        })
        .unwrap_or_default();
    let mut payload = serde_json::json!({
        "action": "short-fiction__revise_short_fiction",
        "completedChapterNumbers": checkpoint.progress.as_ref().map(|progress| progress.completed.clone()).unwrap_or_default(),
        "pendingChapterNumbers": pending,
        "reason": "Continue the saved revision by its operation ID. To replace its instructions or scope, start an explicit new revision with restartPendingRevision=true; the previous checkpoint will be archived.",
    });
    if let Some(map) = payload.as_object_mut() {
        if let Some(operation_id) = &checkpoint.operation_id {
            map.insert("parameters".into(), serde_json::json!({ "resumeOperationId": operation_id }));
        }
        map.insert("workId".into(), Value::String(work_id.to_string()));
        map.insert("checkpointPath".into(), Value::String(checkpoint_path.to_string()));
    }
    payload
}

/// TS `reviseShortFictionProduction`（L742-926）：书锁 → capability 守卫 →
/// 成稿前提 → checkpoint 派生/恢复 → writer.reviseDraft → 权限终验 →
/// finalization 提交 → produceShort(review) → 变更面 → checkpoint 删除。
#[allow(clippy::too_many_lines)]
pub async fn revise_short_fiction_production(
    options: &ShortFictionRunOptions<'_>,
    story_id: &str,
) -> Result<ShortFictionRevisionResult, ShortRunError> {
    let root = options.project_root;
    let lock = acquire_work_mutation_lock(root, story_id);
    let _guard = lock.lock().await;
    revise_short_fiction_production_locked(options, root, story_id).await
}

/// 已持锁入口（894 号：capability 路由环持有 work 锁时调用——tokio Mutex
/// 不可重入，双重 `lock().await` 即自死锁，M1a episode_store 先例同源）。
pub async fn revise_short_fiction_production_locked(
    options: &ShortFictionRunOptions<'_>,
    root: &Path,
    story_id: &str,
) -> Result<ShortFictionRevisionResult, ShortRunError> {
    revise_short_fiction_with_lock(options, root, story_id).await
}

#[allow(clippy::too_many_lines)]
async fn revise_short_fiction_with_lock(
    options: &ShortFictionRunOptions<'_>,
    root: &Path,
    story_id: &str,
) -> Result<ShortFictionRevisionResult, ShortRunError> {
    let work = load_work_manifest(root, story_id).await?;
    let profile_registry = crate::harness::profile_registry::create_built_in_work_profile_registry(root)
        .map_err(|error| ShortRunError::new("WORK_PROFILE_MISMATCH", error.to_string()))?;
    let profile = profile_registry
        .require(&work.profile_id)
        .map_err(|error| ShortRunError::new("WORK_PROFILE_MISMATCH", error.to_string()))?;
    if !profile.capability_ids.iter().any(|capability| capability == "short-fiction") {
        return Err(ShortRunError::new(
            "SHORT_REVISION_PROFILE_MISMATCH",
            "Short revision requires a short-fiction Work",
        ));
    }
    let base = short_work_base_dir(story_id);
    let mut draft = match try_read_short_fiction_draft(root, &base.join("final/short-story.json").to_string_lossy()).await? {
        Some(draft) => draft,
        None => {
            return Err(ShortRunError::new(
                "SHORT_MANUSCRIPT_NOT_READY",
                "Complete the saved draft before revising the accepted manuscript.",
            )
            .with_recovery(serde_json::json!({
                "action": "short-fiction__draft_short_fiction",
                "parameters": {
                    "workId": story_id,
                    "minChapterLength": Value::Null,
                    "maxChapterLength": Value::Null,
                    "openingHookChars": Value::Null,
                    "coverRequired": Value::Null,
                },
                "reason": "The saved draft checkpoint is not an accepted manuscript. Complete and repair it through the draft stage while preserving the existing Work contract.",
            })));
        }
    };
    let language = if work.language == "en" { WritingLanguage::En } else { WritingLanguage::Zh };
    let state = read_short_production_state(root, &base).map_err(ShortRunError::new_state_read)?;
    let checkpoint_path = format!(".inkos/short-revisions/{story_id}.json");
    let saved_text = try_read_project_text(root, &checkpoint_path).await?;
    let saved: Option<ShortRevisionCheckpoint> = match &saved_text {
        Some(text) => Some(serde_json::from_str(text).map_err(|error| {
            ShortRunError::new("SHORT_REVISION_CHECKPOINT_INVALID", error.to_string())
        })?),
        None => None,
    };
    let mut outline_markdown = read_text(root, &base.join("outline/v001.md")).await?;
    let mut source_hash = short_input_hash(&serde_json::json!({
        "draft": serde_json::to_value(&draft).unwrap_or(Value::Null),
        "outlineMarkdown": outline_markdown,
    }));
    let mut review =
        try_read_project_text(root, &base.join("reviews/draft-v001.md").to_string_lossy()).await?.unwrap_or_default();
    // 权限自 episode 原始源 + 作者请求派生（绝不自协调者章表或后续审稿建议派生
    // ——TS 注释逐字）。执行上下文经 ShortRunExecutionScope 显式注入。
    let baseline = options.execution.baseline_work.as_ref().filter(|baseline| baseline.id == story_id);
    let author_request = options.execution.author_request.as_deref().filter(|request| !request.trim().is_empty());
    let original_artifact = baseline.and_then(|baseline| {
        baseline.artifacts.iter().find(|artifact| {
            artifact.revisions.iter().any(|revision| {
                Some(&revision.id) == artifact.current_revision_id.as_ref()
                    && revision.path == "source/final/short-story.json"
            })
        })
    });
    let resume_matches = matches!(
        (&options.resume_operation_id, &saved),
        (Some(resume_id), Some(saved)) if saved.operation_id.as_deref() == Some(*resume_id)
    );
    let mut author_scope: Option<crate::production::short_fiction_tools::ShortAuthorScope> = if resume_matches {
        saved.as_ref().and_then(|saved| saved.author_scope.clone())
    } else {
        None
    };
    let scope_author_request: Option<String> = if author_scope.is_some() {
        saved.as_ref().and_then(|saved| saved.author_request.clone())
    } else {
        author_request.map(str::to_string)
    };
    let author_source_ref: Option<ShortReviewedArtifact> = if resume_matches {
        saved.as_ref().and_then(|saved| saved.author_source.clone())
    } else {
        original_artifact
            .and_then(|artifact| artifact.current_revision_id.clone())
            .map(|revision_id| ShortReviewedArtifact {
                artifact_id: original_artifact.unwrap().id.clone(),
                revision_id,
            })
    };
    let author_original = match &author_source_ref {
        Some(reference) if scope_author_request.is_some() => Some(
            read_artifact_revision(root, story_id, &reference.artifact_id, Some(&reference.revision_id))
                .await
                .map_err(|error| ShortRunError::new("ARTIFACT_NOT_FOUND", error.to_string()))?,
        ),
        _ => None,
    };
    let author_source: Option<crate::production::short_fiction_revision::ScopedAuthorSource> =
        match (&author_original, &author_source_ref, &scope_author_request) {
            (Some(original), Some(reference), Some(request)) => {
                let parsed: ShortFictionBatchDraft = serde_json::from_slice(&original.bytes)
                    .map_err(|error| ShortRunError::new("SHORT_REVISION_BASELINE_INVALID", error.to_string()))?;
                Some(crate::production::short_fiction_revision::ScopedAuthorSource {
                    work_id: story_id.to_string(),
                    artifact_id: reference.artifact_id.clone(),
                    revision_id: reference.revision_id.clone(),
                    request: request.clone(),
                    draft: parsed,
                })
            }
            _ => None,
        };
    if author_scope.is_none() {
        if let (Some(request), Some(original), Some(artifact)) =
            (author_request, &author_original, original_artifact)
        {
            let scope_key = short_input_hash(&serde_json::json!({
                "version": 4,
                "authorRequest": request,
                "baseline": original.revision.checksum,
            }))
            .strip_prefix("sha256:")
            .unwrap_or_default()
            .to_string();
            let scope_path = format!(".inkos/short-author-scopes/{story_id}/{scope_key}.json");
            let saved_scope = try_read_project_text(root, &scope_path).await?;
            author_scope = match &saved_scope {
                Some(text) => Some(serde_json::from_str(text).map_err(|error| {
                    ShortRunError::new("SHORT_AUTHOR_SCOPE_INVALID", error.to_string())
                })?),
                None => {
                    let original_draft: ShortFictionBatchDraft =
                        serde_json::from_slice(&original.bytes).map_err(|error| {
                            ShortRunError::new("SHORT_REVISION_BASELINE_INVALID", error.to_string())
                        })?;
                    let scope =
                        select_author_scope(&options.runtimes.writer, &original_draft, request).await?;
                    commit_atomic_file_set(&AtomicFileSet {
                        root_dir: root,
                        writes: vec![AtomicFileWrite {
                            relative_path: scope_path.clone(),
                            content: FileContent::Text(format!(
                                "{}\n",
                                serde_json::to_string_pretty(&scope).unwrap_or_default().trim_end()
                            )),
                        }],
                        deletes: Vec::new(),
                    })
                    .await
                    .map_err(|error| ShortRunError::new("ATOMIC_WRITE_FAILED", error.to_string()))?;
                    Some(scope)
                }
            };
            // TS recordExecutionEvidence('edit-scope-selected', {workId, artifactId,
            // revisionId, authority:'author_request', scope})。
            options.execution.record(
                "edit-scope-selected",
                &serde_json::json!({
                    "workId": story_id,
                    "artifactId": artifact.id,
                    "revisionId": original.revision.id,
                    "authority": "author_request",
                    "scope": serde_json::to_value(&author_scope).unwrap_or(Value::Null),
                }),
            );
        }
    }

    let recovery_of = |checkpoint: &ShortRevisionCheckpoint| {
        revision_recovery_with(story_id, &checkpoint_path, checkpoint)
    };
    // 有效修订入参（owned——续跑自 checkpoint.request 恢复，TS `{...options, ...saved.request}`）。
    let mut eff_direction = options.direction.to_string();
    let mut eff_chapter_count = options.chapter_count;
    let mut eff_chars_per_chapter = options.chars_per_chapter;
    let mut eff_min_chapter_length = options.min_chapter_length;
    let mut eff_max_chapter_length = options.max_chapter_length;
    let eff_ratio = options.min_chapter_length_ratio;
    let mut eff_opening_hook_chars = options.opening_hook_chars;
    let mut eff_revision_chapter_numbers = options.revision_chapter_numbers.clone();
    if options.resume_operation_id.is_some() {
        // 续跑只收 operation ID；新指令或约束即冲突（TS L800-806）。
        let has_new_instructions = !options.direction.trim().is_empty()
            || options.restart_pending_revision
            || options.chapter_count.is_some()
            || options.chars_per_chapter.is_some()
            || options.max_chapter_length.is_some()
            || options.min_chapter_length.is_some()
            || options.min_chapter_length_ratio.is_some()
            || options.opening_hook_chars.is_some()
            || options.revision_chapter_numbers.is_some();
        if has_new_instructions {
            return Err(ShortRunError::new(
                "SHORT_REVISION_RESUME_CONFLICT",
                "A revision resume accepts only its operation ID, not new instructions or constraints.",
            ));
        }
        let Some(saved) = &saved else {
            return Err(ShortRunError::new(
                "SHORT_REVISION_NOT_FOUND",
                "The requested revision operation is not the pending operation for this Work.",
            ));
        };
        if saved.operation_id.as_deref() != options.resume_operation_id || saved.request.is_none() {
            return Err(ShortRunError::new(
                "SHORT_REVISION_NOT_FOUND",
                "The requested revision operation is not the pending operation for this Work.",
            ));
        }
        let owned_sources: Vec<String> = saved
            .finalization
            .as_ref()
            .map(|finalization| {
                vec![
                    short_input_hash(&serde_json::json!({
                        "draft": serde_json::to_value(&finalization.baseline.draft).unwrap_or(Value::Null),
                        "outlineMarkdown": finalization.result.outline_markdown,
                    })),
                    short_input_hash(&serde_json::json!({
                        "draft": serde_json::to_value(&finalization.result.draft).unwrap_or(Value::Null),
                        "outlineMarkdown": finalization.result.outline_markdown,
                    })),
                ]
            })
            .unwrap_or_default();
        if saved.source_hash.as_deref() != Some(source_hash.as_str()) && !owned_sources.contains(&source_hash) {
            return Err(ShortRunError::new(
                "SHORT_REVISION_SOURCE_CHANGED",
                "The manuscript or outline changed after this revision began. Start an explicit new revision from the current source.",
            ));
        }
        if let Some(finalization) = &saved.finalization {
            // 完成审查不重写章节、不重置变更基线（TS 注释逐字）。
            draft = finalization.baseline.draft.clone();
            outline_markdown = finalization.baseline.outline_markdown.clone();
            source_hash = short_input_hash(&serde_json::json!({
                "draft": serde_json::to_value(&draft).unwrap_or(Value::Null),
                "outlineMarkdown": outline_markdown,
            }));
        }
        if let Some(request) = &saved.request {
            eff_direction = request.direction.clone();
            eff_chapter_count = request.chapter_count;
            eff_chars_per_chapter = request.chars_per_chapter;
            eff_min_chapter_length = request.min_chapter_length;
            eff_max_chapter_length = request.max_chapter_length;
            eff_opening_hook_chars = request.opening_hook_chars;
            eff_revision_chapter_numbers = request.revision_chapter_numbers.clone();
        }
        review = saved.review.clone().unwrap_or_default();
    } else if options.direction.trim().is_empty() {
        return Err(ShortRunError::new(
            "SHORT_REVISION_REQUEST_REQUIRED",
            "Supply a new revision instruction or the pending operation ID.",
        ));
    }
    let chapter_count = match eff_chapter_count {
        Some(count) => positive_integer(Some(count), draft.chapters.len() as u32, "chapterCount")?,
        None => draft.chapters.len() as u32,
    };
    if let Some(scope) = &author_scope {
        if !scope.whole_manuscript {
            let mut allowed: Vec<u32> = scope.chapter_numbers.clone();
            allowed.extend(scope.chapter_title_numbers.clone().unwrap_or_default());
            allowed.sort_unstable();
            allowed.dedup();
            let requested = eff_revision_chapter_numbers.clone().unwrap_or_else(|| allowed.clone());
            if chapter_count != draft.chapters.len() as u32
                || requested.iter().any(|number| !allowed.contains(number))
            {
                return Err(ShortRunError::new(
                    "SHORT_REVISION_OUT_OF_SCOPE",
                    "The requested chapters exceed the original author permission. Review findings do not expand revision scope.",
                )
                .with_recovery(serde_json::json!({ "allowedChapterNumbers": allowed })));
            }
            eff_revision_chapter_numbers = Some(requested);
        }
    }
    if eff_revision_chapter_numbers.is_some() && chapter_count != draft.chapters.len() as u32 {
        return Err(ShortRunError::new(
            "SHORT_REVISION_SCOPE_CONFLICT",
            "A chapter-count change requires whole-manuscript scope; describe which source chapters to retain in the instruction.",
        ));
    }
    let state_target = state.as_ref().and_then(|state| state.target.clone());
    let revision_title_owned = state_target
        .as_ref()
        .and_then(|target| target.title.clone())
        .unwrap_or_else(|| draft.story_title.clone());
    let profile_ratio = profile.production.min_chapter_length_ratio;
    let revision_options = ShortFictionRunOptions {
        direction: &eff_direction,
        revision_request: Some(&eff_direction),
        language: Some(language),
        chapter_count: Some(chapter_count),
        title: Some(&revision_title_owned),
        min_chapter_length: eff_min_chapter_length.or_else(|| state_target.as_ref().and_then(|target| target.min_chapter_length)),
        opening_hook_chars: eff_opening_hook_chars.or_else(|| state_target.as_ref().and_then(|target| target.opening_hook_chars)),
        chars_per_chapter: eff_chars_per_chapter.or_else(|| {
            eff_max_chapter_length.map(|max| {
                std::cmp::min(state_target.as_ref().map(|target| target.chars_per_chapter).unwrap_or(max), max)
            }).or_else(|| state_target.as_ref().map(|target| target.chars_per_chapter))
        }),
        min_chapter_length_ratio: eff_ratio.or(profile_ratio),
        max_chapter_length: eff_max_chapter_length.or_else(|| state_target.as_ref().and_then(|target| target.max_chapter_length)),
        cover: Some(false),
        cover_required: None,
        ..options.clone()
    };
    let minimum = short_draft_minimum(&revision_options)?;
    let chapter_numbers = eff_revision_chapter_numbers.clone();
    if let Some(numbers) = &chapter_numbers {
        if numbers.iter().any(|number| *number == 0 || *number as usize > draft.chapters.len()) {
            return Err(ShortRunError::new("SHORT_REVISION_SCOPE_INVALID", "Invalid revision chapter scope"));
        }
    }
    let input_hash = revision_input_hash(
        &draft,
        &outline_markdown,
        &review,
        &eff_direction,
        revision_options.chars_per_chapter,
        chapter_count,
        chapter_numbers.as_deref(),
        &minimum,
    );
    let restart = options.restart_pending_revision;
    let reuse = saved.is_some()
        && !restart
        && (options.resume_operation_id.is_some()
            || saved.as_ref().map(|saved| saved.input_hash == input_hash).unwrap_or(false));
    if let Some(saved) = saved.as_ref().filter(|_| !reuse && !restart) {
        return Err(ShortRunError::new(
            "SHORT_REVISION_PENDING",
            "A revision is already pending. Resume it by ID or explicitly replace it; changed retry wording does not discard its progress.",
        )
        .with_recovery(recovery_of(saved)));
    }
    let mut resume: Option<ShortRevisionProgress> = None;
    if reuse {
        if let Some(saved) = saved.as_ref().filter(|saved| saved.progress.is_some()) {
            let progress = saved.progress.clone().unwrap();
            let valid = progress.completed.iter().all(|number| {
                progress.plan.chapters.iter().any(|chapter| chapter.number == *number)
            });
            if !valid {
                return Err(ShortRunError::new("SHORT_REVISION_CHECKPOINT_INVALID", "Invalid short revision checkpoint"));
            }
            resume = Some(progress);
        }
    }
    let operation_id = match (&reuse, &saved) {
        (true, Some(saved)) if saved.operation_id.is_some() => saved.operation_id.clone().unwrap(),
        _ => uuid::Uuid::new_v4().to_string(),
    };
    let mut checkpoint = ShortRevisionCheckpoint {
        operation_id: Some(operation_id),
        input_hash: input_hash.clone(),
        source_hash: Some(source_hash.clone()),
        review: Some(review.clone()),
        author_scope: author_scope.clone(),
        author_request: scope_author_request.clone(),
        author_source: author_source_ref.clone(),
        request: Some(ShortRevisionRequest {
            direction: revision_options.direction.to_string(),
            chapter_count: Some(chapter_count),
            chars_per_chapter: revision_options.chars_per_chapter,
            min_chapter_length: Some(minimum.min_chapter_length),
            max_chapter_length: minimum.max_chapter_length,
            opening_hook_chars: minimum.opening_hook_chars,
            revision_chapter_numbers: chapter_numbers.clone(),
        }),
        progress: resume.clone(),
        finalization: if reuse {
            saved.as_ref().and_then(|saved| saved.finalization.clone())
        } else {
            None
        },
    };
    let mut writes = Vec::new();
    if let (Some(saved_text), true) = (&saved_text, restart) {
        writes.push(AtomicFileWrite {
            relative_path: format!(".inkos/short-revisions/archive/{}.json", uuid::Uuid::new_v4()),
            content: FileContent::Text(format!("{}\n", saved_text.trim_end())),
        });
    }
    writes.push(AtomicFileWrite {
        relative_path: checkpoint_path.clone(),
        content: FileContent::Text(format!(
            "{}\n",
            serde_json::to_string_pretty(&checkpoint).unwrap_or_default().trim_end()
        )),
    });
    commit_atomic_file_set(&AtomicFileSet { root_dir: root, writes, deletes: Vec::new() })
        .await
        .map_err(|error| ShortRunError::new("ATOMIC_WRITE_FAILED", error.to_string()))?;

    // reviseDraft（TS L876-889）：checkpoint finalization 优先。
    let revised = if let Some(finalization) = &checkpoint.finalization {
        (finalization.result.draft.clone(), finalization.result.outline_markdown.clone())
    } else {
        // TS：direction = authorScope ? JSON.stringify({authorRequest, revisionInstruction, permission}) : direction。
        let revise_direction: String = match &author_scope {
            Some(scope) => serde_json::json!({
                "authorRequest": scope_author_request.clone().unwrap_or_default(),
                "revisionInstruction": eff_direction,
                "permission": serde_json::to_value(scope).unwrap_or(Value::Null),
            })
            .to_string(),
            None => eff_direction.clone(),
        };
        let checkpoint_for_progress = Arc::new(std::sync::Mutex::new(checkpoint.clone()));
        let checkpoint_path_for_progress = checkpoint_path.clone();
        let root_for_progress = root.to_path_buf();
        let progress_message = options.on_progress.clone();
        let on_revision_progress: crate::production::short_fiction_writer_chain::OnRevisionProgress =
            Arc::new(move |draft: &ShortFictionBatchDraft, completed: &[u32]| {
                let checkpoint_state = Arc::clone(&checkpoint_for_progress);
                let path = checkpoint_path_for_progress.clone();
                let root = root_for_progress.clone();
                let progress_message = progress_message.clone();
                let completed: Vec<u32> = completed.to_vec();
                let draft = draft.clone();
                Box::pin(async move {
                    {
                        let mut guard = checkpoint_state.lock().unwrap();
                        guard.progress = Some(ShortRevisionProgress {
                            plan: guard.progress.as_ref().map(|progress| progress.plan.clone()).unwrap_or_else(|| crate::production::short_fiction::ShortRevisionPlan {
                                story_title: None,
                                revision_brief: String::new(),
                                outline_markdown: None,
                                opening_hook: None,
                                chapters: Vec::new(),
                            }),
                            draft,
                            completed,
                        });
                    }
                    let payload = {
                        let guard = checkpoint_state.lock().unwrap();
                        serde_json::to_string_pretty(&*guard).unwrap_or_default()
                    };
                    commit_atomic_file_set(&AtomicFileSet {
                        root_dir: &root,
                        writes: vec![AtomicFileWrite {
                            relative_path: path,
                            content: FileContent::Text(format!("{}\n", payload.trim_end())),
                        }],
                        deletes: Vec::new(),
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                    if let Some(notify) = &progress_message {
                        let snapshot = checkpoint_state.lock().unwrap();
                        if let Some(state_progress) = &snapshot.progress {
                            notify(&format!(
                                "Short revision progress: {}/{} chapters",
                                state_progress.completed.len(),
                                state_progress.plan.chapters.len()
                            ));
                        }
                    }
                    Ok(())
                })
            });
        // ReviserAgent 分发（894 scoped 分支）：author 源在场 → scoped 解析链；
        // 否则 892 compress 回退面。
        let revise_dispatch = match &author_source {
            Some(source) => {
                crate::production::short_fiction_writer_chain::scoped_or_compress_revise_chapter_fn(
                    Arc::clone(&options.runtimes.writer.chat),
                    options.runtimes.writer.model.clone(),
                    options.runtimes.writer.defaults.clone(),
                    language,
                    revision_options.chars_per_chapter.unwrap_or(match language {
                        WritingLanguage::En => SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER,
                        _ => SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER,
                    }),
                    Some(minimum.min_chapter_length),
                    minimum.max_chapter_length,
                    revision_options.direction.to_string(),
                    source.clone(),
                )
            }
            None => crate::production::short_fiction_writer_chain::compress_revise_chapter_fn(
                Arc::clone(&options.runtimes.writer.chat),
                options.runtimes.writer.model.clone(),
                options.runtimes.writer.defaults.clone(),
                language,
                revision_options.chars_per_chapter.unwrap_or(match language {
                    WritingLanguage::En => SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER,
                    _ => SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER,
                }),
                Some(minimum.min_chapter_length),
                minimum.max_chapter_length,
                revision_options.direction.to_string(),
            ),
        };
        let (revised_draft, revised_outline) =
            crate::production::short_fiction_writer_chain::writer_revise_draft(
                &options.runtimes.writer,
                &crate::production::short_fiction_writer_chain::ReviseDraftInput {
                    direction: &revise_direction,
                    outline_markdown: &outline_markdown,
                    chapter_count,
                    chars_per_chapter: revision_options.chars_per_chapter.unwrap_or(match language {
                        WritingLanguage::En => SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER,
                        _ => SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER,
                    }),
                    min_chapter_length: Some(minimum.min_chapter_length),
                    max_chapter_length: minimum.max_chapter_length,
                    opening_hook_chars: minimum.opening_hook_chars,
                    language,
                    draft: &draft,
                    review: &review,
                    chapter_numbers: chapter_numbers.clone(),
                    author_scope: author_scope.as_ref(),
                    author_source_request: author_source.as_ref().map(|source| source.request.as_str()),
                    author_source: author_source.clone(),
                },
                Some(&revise_dispatch),
                on_revision_progress,
            )
            .await
            .map_err(|error| {
                let mut current = checkpoint.clone();
                ShortRunError { code: error.code, message: error.message, recovery: Some(recovery_of(&{
                    current.progress = checkpoint.progress.clone();
                    current
                })) }
            })?;
        (revised_draft, revised_outline)
    };
    let (revised_draft, revised_outline) = revised;
    if let Some(scope) = &author_scope {
        // 887 契约型 ShortAuthorScope（assert 消费面）与 tools 型经 serde 转换。
        let contract_scope: crate::production::short_fiction::ShortAuthorScope =
            serde_json::from_value(serde_json::to_value(scope).unwrap_or(Value::Null))
                .map_err(|error| ShortRunError::new("SHORT_AUTHOR_SCOPE_INVALID", error.to_string()))?;
        crate::production::short_fiction::assert_short_author_scope(
            &contract_scope,
            &draft,
            &revised_draft,
            &outline_markdown,
            &revised_outline,
        )
        .map_err(|error| ShortRunError::new("SHORT_REVISION_OUT_OF_SCOPE", error.message))?;
    }
    if revised_draft.story_title != draft.story_title
        && !author_scope.as_ref().and_then(|scope| scope.story_title).unwrap_or(false)
    {
        return Err(ShortRunError::new(
            "SHORT_REVISION_OUT_OF_SCOPE",
            "Changing the book title requires original author permission.",
        )
        .with_recovery(serde_json::json!({ "changedParts": ["title"] })));
    }
    let outline_write = if revised_outline == outline_markdown {
        None
    } else {
        Some(revised_outline.clone())
    };
    checkpoint.finalization = Some(ShortRevisionFinalization {
        baseline: ShortRevisionSnapshot { draft: draft.clone(), outline_markdown: outline_markdown.clone() },
        result: ShortRevisionSnapshot {
            draft: revised_draft.clone(),
            outline_markdown: outline_write.clone().unwrap_or_else(|| outline_markdown.clone()),
        },
    });
    let mut writes = Vec::new();
    if let Some(outline_content) = &outline_write {
        writes.push(text_write(
            format!("{}/outline/v001.md", posix_base(&base)),
            outline_content,
        ));
    }
    writes.push(text_write(
        format!("{}/drafts/v001/draft.json", posix_base(&base)),
        &serde_json::to_string_pretty(&revised_draft).unwrap_or_default(),
    ));
    writes.push(AtomicFileWrite {
        relative_path: checkpoint_path.clone(),
        content: FileContent::Text(format!(
            "{}\n",
            serde_json::to_string_pretty(&checkpoint).unwrap_or_default().trim_end()
        )),
    });
    commit_atomic_file_set(&AtomicFileSet { root_dir: root, writes, deletes: Vec::new() })
        .await
        .map_err(|error| ShortRunError::new("ATOMIC_WRITE_FAILED", error.to_string()))?;

    // produceShort(revisedDraft, stopAfter='review')（TS L903）。
    let produce_result = produce_short(
        &revision_options_with_title(&revision_options, &revised_draft.story_title),
        root,
        Some(story_id),
        Some(&revised_draft),
        Some(ShortProductionStage::Review),
    )
    .await;
    match produce_result {
        Ok(ProduceOutcome::Stage(stage)) => {
            let committed = try_read_short_fiction_draft(root, &base.join("final/short-story.json").to_string_lossy())
                .await?
                .ok_or_else(|| ShortRunError::new("SHORT_MANUSCRIPT_NOT_READY", "committed manuscript missing"))?;
            let changed: std::collections::BTreeSet<u32> = draft
                .chapters
                .iter()
                .filter(|before| {
                    committed
                        .chapters
                        .iter()
                        .find(|after| after.number == before.number)
                        .map(|after| after.title != before.title || after.content != before.content)
                        .unwrap_or(true)
                })
                .chain(committed.chapters.iter().filter(|after| {
                    !draft.chapters.iter().any(|before| before.number == after.number)
                }))
                .map(|chapter| chapter.number)
                .collect();
            let revision_changes = ShortRevisionChanges {
                chapter_numbers: changed.into_iter().collect(),
                opening_changed: draft.opening_hook.clone().unwrap_or_default()
                    != committed.opening_hook.clone().unwrap_or_default(),
                title_changed: draft.story_title != committed.story_title,
                outline_changed: outline_markdown
                    != read_text(root, &base.join("outline/v001.md")).await?,
            };
            let _ = commit_delete(root, &PathBuf::from(&checkpoint_path)).await;
            let stage_status_failed = stage.stage_status == ShortStageStatus::Failed;
            Ok(ShortFictionRevisionResult {
                story_id: stage.story_id,
                delivery: stage.delivery,
                reviewed_artifact: stage.reviewed_artifact,
                observations: stage.observations,
                revision_changes,
                outline_path: to_project_path(&base.join("outline/v001.md").to_string_lossy()),
                draft_review_path: to_project_path(
                    &base.join(if stage_status_failed { "reviews/draft-warning.md" } else { "reviews/draft-v001.md" }).to_string_lossy(),
                ),
                final_markdown_path: to_project_path(&base.join("final/full.md").to_string_lossy()),
                final_json_path: to_project_path(&base.join("final/short-story.json").to_string_lossy()),
            })
        }
        Ok(ProduceOutcome::Run(_)) => unreachable!("stopAfter=review 时必为 Stage"),
        Err(error) => {
            let _ = sync_work_source_artifacts(SyncWorkSourceInput {
                project_root: root,
                work_id: story_id,
                accept: false,
                accept_paths: None,
                episode_id: None,
                updated_at: None,
                writes: Vec::new(),
                title: None,
                status: None,
            })
            .await;
            Err(error.with_recovery(recovery_of(&checkpoint)))
        }
    }
}

fn revision_options_with_title<'a>(
    revision_options: &ShortFictionRunOptions<'a>,
    title: &'a str,
) -> ShortFictionRunOptions<'a> {
    let mut options = revision_options.clone();
    options.title = Some(title);
    options
}

// ── 测试（mock 端到端：四阶段产物形态 + 幂等重放 + 失败恢复）─────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::production::short_fiction_agents::ShortAgents;
    use crate::production::structured_agent::{
        ChatTurnError, ChatTurnResponse, ChatTurnToolCall, StructuredDefaults,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;

    fn tokio() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    struct ScriptedAgent {
        script: Arc<StdMutex<VecDeque<ChatTurnResponse>>>,
        chat: crate::production::structured_agent::ChatTurnFn,
    }

    impl ScriptedAgent {
        fn new(script: Vec<ChatTurnResponse>) -> Self {
            let queue = Arc::new(StdMutex::new(VecDeque::from(script)));
            let queue_for_fn = Arc::clone(&queue);
            let chat: crate::production::structured_agent::ChatTurnFn = Arc::new(move |_request| {
                let queue = Arc::clone(&queue_for_fn);
                Box::pin(async move {
                    let mut guard = queue.lock().unwrap();
                    guard.pop_front().ok_or_else(|| ChatTurnError {
                        message: "script exhausted".into(),
                        output_limit: false,
                    })
                })
            });
            Self { script: queue, chat }
        }

        fn agents(&self) -> ShortAgents {
            let mut agents = ShortAgents::new(
                Arc::clone(&self.chat),
                "test-model",
                StructuredDefaults { temperature: 0.7, max_tokens: 8192 },
            );
            agents.author_request = None;
            agents
        }

        fn remaining(&self) -> usize {
            self.script.lock().unwrap().len()
        }
    }

    fn tool_response(name: &str, arguments: Value) -> ChatTurnResponse {
        ChatTurnResponse {
            content: String::new(),
            tool_calls: vec![ChatTurnToolCall {
                id: format!("call-{name}"),
                name: name.to_string(),
                arguments: arguments.to_string(),
            }],
            finish_reason: None,
            usage: Default::default(),
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "inkos-runner-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn review_record(code: &str, source_id: &str) -> ChatTurnResponse {
        tool_response(
            "record_review_observation",
            serde_json::json!({
                "code": code,
                "summary": "有依据的观察。",
                "category": "quality",
                "assessment": "observation",
                "sourceRefs": [{ "sourceId": source_id, "startLine": 1, "endLine": 1 }],
            }),
        )
    }

    fn full_scripts() -> (ScriptedAgent, ScriptedAgent, ScriptedAgent, ScriptedAgent) {
        let planner = ScriptedAgent::new(vec![tool_response(
            "submit_short_outline",
            serde_json::json!({
                "storyTitle": "夜航",
                "planMarkdown": "## 总纲\n\n少年提灯夜航。",
                "chapter_1_plan": "启航",
                "chapter_2_plan": "遇雾",
            }),
        )]);
        let writer = ScriptedAgent::new(vec![tool_response(
            "submit_short_draft_batch",
            serde_json::json!({
                "storyTitle": "夜航",
                "chapter_1_title": "启航",
                "chapter_1_content": "少年提灯出门，河面浮起第一层雾。",
                "chapter_2_title": "遇雾",
                "chapter_2_content": "雾里有旧船，船上有旧名字。",
            }),
        )]);
        let draft_review = ScriptedAgent::new(vec![
            review_record("R-1", "outline"),
            tool_response(
                "submit_short_fiction_review",
                serde_json::json!({ "summary": "一处观察。", "observationCodes": ["R-1"] }),
            ),
            review_record("P-1", "package"),
            tool_response(
                "submit_short_package_review",
                serde_json::json!({ "summary": "无硬伤。", "observationCodes": ["P-1"] }),
            ),
        ]);
        let package = ScriptedAgent::new(vec![tool_response(
            "submit_short_package",
            serde_json::json!({
                "title": "夜航",
                "intro": "以灯航雾。",
                "sellingPoints": ["灯"],
                "coverPrompt": "雾中的灯。",
            }),
        )]);
        (planner, writer, draft_review, package)
    }

    fn runtimes_of(
        planner: &ScriptedAgent,
        writer: &ScriptedAgent,
        draft_review: &ScriptedAgent,
        package: &ScriptedAgent,
    ) -> ShortFictionRunRuntimes {
        ShortFictionRunRuntimes {
            planner: planner.agents(),
            writer: writer.agents(),
            draft_review: draft_review.agents(),
            package: package.agents(),
        }
    }

    fn base_options<'a>(
        root: &'a Path,
        runtimes: &'a ShortFictionRunRuntimes,
    ) -> ShortFictionRunOptions<'a> {
        let mut options = ShortFictionRunOptions::new(root, "写一个提灯夜航的短篇", runtimes);
        options.story_id = Some("夜航");
        options.chapter_count = Some(2);
        options.chars_per_chapter = Some(30);
        options.cover = Some(false);
        options
    }

    #[test]
    fn produce_full_run_then_replay_hits_without_agent_calls() {
        let root = temp_root("replay");
        let (planner, writer, draft_review, package) = full_scripts();
        let runtimes = runtimes_of(&planner, &writer, &draft_review, &package);
        let options = base_options(&root, &runtimes);
        let first = tokio().block_on(run_short_fiction_production(&options)).unwrap();
        assert_eq!(first.story_id, "夜航");
        assert!(root.join("works/夜航/work.json").exists());
        let source = root.join("works/夜航/source");
        for path in [
            "outline/v001.md",
            "drafts/v001/draft.json",
            "drafts/v001/full.md",
            "drafts/v001/chapters/0001.md",
            "drafts/v001/chapters/0002.md",
            "final/short-story.json",
            "final/full.md",
            "final/sales-package.json",
            "final/sales-package.md",
            "final/cover-prompt.md",
            "reviews/draft-v001.md",
            "reviews/package-v001.md",
            "production-state.json",
        ] {
            assert!(source.join(path).exists(), "缺少产物 {path}");
        }
        assert_eq!(first.cover_error.as_deref(), Some("disabled"));
        let stages = first.stage_results.as_ref().unwrap();
        assert_eq!(stages.review.as_ref().unwrap().status, ShortStageStatus::Completed);
        assert_eq!(stages.package.as_ref().unwrap().status, ShortStageStatus::Completed);
        assert!(stages.cover.is_none(), "cover=false 不写封面阶段");
        assert!(first.draft_review_path.ends_with("reviews/draft-v001.md"));
        // mock 脚本耗尽（两轮四代理恰好按序消费）。
        assert_eq!(planner.remaining(), 0);
        assert_eq!(writer.remaining(), 0);
        assert_eq!(draft_review.remaining(), 0);
        assert_eq!(package.remaining(), 0);

        // 幂等重放：空脚本（任何代理调用都会 script exhausted）→ 命中缓存。
        let empty = ScriptedAgent::new(Vec::new());
        let replay_runtimes = runtimes_of(&empty, &empty, &empty, &empty);
        let replay_options = base_options(&root, &replay_runtimes);
        let second = tokio().block_on(run_short_fiction_production(&replay_options)).unwrap();
        assert_eq!(second.story_id, first.story_id);
        assert_eq!(
            second.stage_results.as_ref().unwrap().review.as_ref().unwrap().input_hash,
            stages.review.as_ref().unwrap().input_hash
        );
        assert!(second.delivery.is_some());

        // 变异：七工件之一缺失 → 重放失效 → 重跑（新脚本）走全链。
        std::fs::remove_file(source.join("final/sales-package.json")).unwrap();
        let (planner2, writer2, draft_review2, package2) = full_scripts();
        let runtimes2 = runtimes_of(&planner2, &writer2, &draft_review2, &package2);
        let rerun_options = base_options(&root, &runtimes2);
        let third = tokio().block_on(run_short_fiction_production(&rerun_options)).unwrap();
        assert!(third.sales_package_path.ends_with("sales-package.md"));
        assert!(source.join("final/sales-package.json").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn draft_failure_records_recovery_checkpoint() {
        let root = temp_root("recovery");
        // outline 成功；writer 单批全过 merge（内容非空）但篇幅低于 min=50 →
        // 补章环 submit_short_revision_chapter 脚本耗尽 → 恢复错误带 checkpoint。
        let planner = ScriptedAgent::new(vec![tool_response(
            "submit_short_outline",
            serde_json::json!({
                "storyTitle": "夜航",
                "planMarkdown": "## 总纲",
                "chapter_1_plan": "启航",
                "chapter_2_plan": "遇雾",
            }),
        )]);
        let writer = ScriptedAgent::new(vec![tool_response(
            "submit_short_draft_batch",
            serde_json::json!({
                "storyTitle": "夜航",
                "chapter_1_title": "启航",
                "chapter_1_content": "少年提灯出门。",
                "chapter_2_title": "遇雾",
                "chapter_2_content": "雾起。",
            }),
        )]);
        let empty_review = ScriptedAgent::new(Vec::new());
        let empty_package = ScriptedAgent::new(Vec::new());
        let runtimes = ShortFictionRunRuntimes {
            planner: planner.agents(),
            writer: writer.agents(),
            draft_review: empty_review.agents(),
            package: empty_package.agents(),
        };
        let mut options = base_options(&root, &runtimes);
        options.min_chapter_length = Some(50);
        let error = tokio().block_on(run_short_fiction_production(&options)).unwrap_err();
        assert!(
            error.message.contains("Persisted draft checkpoint"),
            "恢复 payload 缺失：{}",
            error.message
        );
        let recovery = error.recovery.unwrap();
        assert_eq!(
            recovery.get("action").and_then(Value::as_str),
            Some("short-fiction__draft_short_fiction")
        );
        assert_eq!(
            recovery.get("persistedChapterNumbers").and_then(Value::as_array).map(|items| items.len()),
            Some(2)
        );
        assert_eq!(
            recovery.get("incompleteChapterNumbers").and_then(Value::as_array).map(|items| items.len()),
            Some(2)
        );
        // 候选已落盘（v001-partial 原子集）。
        assert!(root.join("works/夜航/source/drafts/v001-partial/draft.json").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn slugify_and_safe_segment_match_ts_shape() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(safe_segment("a/b:c*d"), "a-b-c-d");
        assert_eq!(safe_file_name("雨/夜:航"), "雨_夜_航");
        // TS 同形：safeFileName("full.md")="full.md" → 终名 "full.md.md" 不在
        // 保留名表（保留名比对的是最终文件名）。
        assert_eq!(short_title_path("full.md"), Some("source/final/full.md.md".into()));
        assert_eq!(short_title_path("夜航"), Some("source/final/夜航.md".into()));
    }

    fn revise_options<'a>(
        root: &'a Path,
        runtimes: &'a ShortFictionRunRuntimes,
        direction: &'a str,
    ) -> ShortFictionRunOptions<'a> {
        // TS revise 工具参数面不含 minChapterLength——续跑守卫的「新约束」判定
        // 语义以此为准（min 由 saved.request 或 saved target 恢复）。
        let mut options = ShortFictionRunOptions::new(root, direction, runtimes);
        options.chapter_count = Some(2);
        options.chars_per_chapter = Some(30);
        options.cover = Some(false);
        options
    }

    fn review_pair() -> Vec<ChatTurnResponse> {
        vec![
            review_record("R-1", "outline"),
            tool_response(
                "submit_short_fiction_review",
                serde_json::json!({ "summary": "修订后一处观察。", "observationCodes": ["R-1"] }),
            ),
        ]
    }

    #[test]
    fn revise_flow_checkpoint_lifecycle() {
        let root = temp_root("revise");
        let (planner, writer, draft_review, package) = full_scripts();
        let runtimes = runtimes_of(&planner, &writer, &draft_review, &package);
        let options = base_options(&root, &runtimes);
        tokio().block_on(run_short_fiction_production(&options)).unwrap();
        let checkpoint_path = root.join(".inkos/short-revisions/夜航.json");
        assert!(!checkpoint_path.exists());

        // 修订：单章快路径（submit_short_revision_chapter）→ review 阶段 →
        // checkpoint 建档→删除 + revisionChanges 面。
        let revise_writer = ScriptedAgent::new(vec![tool_response(
            "submit_short_revision_chapter",
            serde_json::json!({ "title": "启航（修）", "content": "修订后的第一章正文，长度满足要求且事实一致。" }),
        )]);
        let revise_review = ScriptedAgent::new(review_pair());
        let revise_runtimes = ShortFictionRunRuntimes {
            planner: planner.agents(),
            writer: revise_writer.agents(),
            draft_review: revise_review.agents(),
            package: package.agents(),
        };
        let mut revise = revise_options(&root, &revise_runtimes, "重写第一章");
        revise.story_id = Some("夜航");
        revise.revision_chapter_numbers = Some(vec![1]);
        let result = tokio().block_on(revise_short_fiction_production(&revise, "夜航")).unwrap();
        assert_eq!(result.revision_changes.chapter_numbers, vec![1]);
        assert!(!result.revision_changes.title_changed);
        assert!(!result.revision_changes.outline_changed);
        assert!(!result.revision_changes.opening_changed);
        assert!(result.draft_review_path.ends_with("reviews/draft-v001.md"));
        // checkpoint 删除 + final 落修订稿。
        assert!(!checkpoint_path.exists(), "成功后 checkpoint 应删除");
        let final_draft = std::fs::read_to_string(root.join("works/夜航/source/final/short-story.json")).unwrap();
        assert!(final_draft.contains("启航（修）"), "final 应落修订章题");
        assert!(final_draft.contains("雾里有旧船"), "第二章应保持原样");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn revise_pending_then_resume_by_operation_id() {
        let root = temp_root("revise-resume");
        let (planner, writer, draft_review, package) = full_scripts();
        let runtimes = runtimes_of(&planner, &writer, &draft_review, &package);
        let options = base_options(&root, &runtimes);
        tokio().block_on(run_short_fiction_production(&options)).unwrap();
        let checkpoint_path = root.join(".inkos/short-revisions/夜航.json");

        // a) 修订脚本耗尽 → 失败，checkpoint 留存（操作 ID 可续）。
        let failed_writer = ScriptedAgent::new(Vec::new());
        let failed_runtimes = ShortFictionRunRuntimes {
            planner: planner.agents(),
            writer: failed_writer.agents(),
            draft_review: ScriptedAgent::new(Vec::new()).agents(),
            package: package.agents(),
        };
        let mut first = revise_options(&root, &failed_runtimes, "重写第一章");
        first.story_id = Some("夜航");
        first.revision_chapter_numbers = Some(vec![1]);
        let _error = tokio().block_on(revise_short_fiction_production(&first, "夜航")).unwrap_err();
        assert!(checkpoint_path.exists(), "失败后 checkpoint 应留存");
        let saved: ShortRevisionCheckpoint =
            serde_json::from_str(&std::fs::read_to_string(&checkpoint_path).unwrap()).unwrap();
        let operation_id = saved.operation_id.clone().unwrap();

        // b) 不同措辞重试（无 resume ID）→ SHORT_REVISION_PENDING + recovery。
        let mut second = revise_options(&root, &failed_runtimes, "换个说法重写第一章");
        second.story_id = Some("夜航");
        second.revision_chapter_numbers = Some(vec![1]);
        let pending = tokio().block_on(revise_short_fiction_production(&second, "夜航")).unwrap_err();
        assert_eq!(pending.code, "SHORT_REVISION_PENDING");
        assert_eq!(
            pending.recovery.as_ref().unwrap().get("parameters").unwrap().get("resumeOperationId").and_then(Value::as_str),
            Some(operation_id.as_str())
        );

        // c) resume（只带 operation ID）→ 续跑成功 → checkpoint 删除。
        let resume_writer = ScriptedAgent::new(vec![tool_response(
            "submit_short_revision_chapter",
            serde_json::json!({ "title": "启航（修）", "content": "续跑修订后的第一章正文，长度满足要求。" }),
        )]);
        let resume_review = ScriptedAgent::new(review_pair());
        let resume_runtimes = ShortFictionRunRuntimes {
            planner: planner.agents(),
            writer: resume_writer.agents(),
            draft_review: resume_review.agents(),
            package: package.agents(),
        };
        // TS resume 工具面只带 resumeOperationId（其余约束参数必须缺省——
        // conflict 守卫语义）。
        let mut resume = ShortFictionRunOptions::new(&root, "", &resume_runtimes);
        resume.story_id = Some("夜航");
        resume.cover = Some(false);
        resume.resume_operation_id = Some(&operation_id);
        let result = tokio().block_on(revise_short_fiction_production(&resume, "夜航")).unwrap();
        assert_eq!(result.revision_changes.chapter_numbers, vec![1]);
        assert!(!checkpoint_path.exists(), "续跑成功后 checkpoint 应删除");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn revise_scoped_branch_runs_reviser_chain() {
        // 894 scoped 执行分支：authorSource 在场 → 逐章 reviseChapter
        // （select→resolve→authorized/edit_ranges）+ 章题提交 + 权限终验。
        // mock 序：scope 派生（submit_short_author_scope）→ locate → spot-fix
        // 两段（edit_ranges→replacements）→ 章题提交 → review 双轮。
        let root = temp_root("revise-scoped");
        let (planner, writer, draft_review, package) = full_scripts();
        let runtimes = runtimes_of(&planner, &writer, &draft_review, &package);
        let options = base_options(&root, &runtimes);
        tokio().block_on(run_short_fiction_production(&options)).unwrap();

        let scope_submission = tool_response(
            "submit_short_author_scope",
            serde_json::json!({
                "wholeManuscript": false,
                "chapterNumbers": [1],
                "chapterTitleNumbers": [1],
                "opening": false,
                "outline": false,
            }),
        );
        // locate：段落 1 选区（TS LocationSchema 形态——kind/boundary/count + quote）。
        let locate = tool_response(
            "submit_edit_location",
            serde_json::json!({
                "kind": "paragraphs",
                "boundary": "first",
                "count": 1,
                "targets": [],
            }),
        );
        let edit_ranges = tool_response(
            "submit_chapter_edit_ranges",
            serde_json::json!({ "ranges": [{ "startLine": 1, "endLine": 1, "note": "开场微调" }] }),
        );
        let replacements = tool_response(
            "submit_chapter_range_replacements",
            serde_json::json!({ "selection_0_text": "雨夜里少年提灯出门，第一层雾漫过河面。" }),
        );
        let chapter_title = tool_response(
            "submit_short_revision_title",
            serde_json::json!({ "title": "启航·雨雾" }),
        );
        let scoped_writer = ScriptedAgent::new(vec![
            scope_submission,
            locate,
            edit_ranges,
            replacements,
            chapter_title,
        ]);
        let scoped_review = ScriptedAgent::new(review_pair());
        let scoped_runtimes = ShortFictionRunRuntimes {
            planner: planner.agents(),
            writer: scoped_writer.agents(),
            draft_review: scoped_review.agents(),
            package: package.agents(),
        };
        let mut scoped = revise_options(&root, &scoped_runtimes, "微调第一章开场；章题点出雨雾");
        scoped.story_id = Some("夜航");
        scoped.revision_chapter_numbers = Some(vec![1]);
        // execution.author_request 驱动 scope 派生（TS currentExecutionAuthorRequest）。
        scoped.execution.author_request = Some("微调第一章开场；章题点出雨雾".to_string());
        let manifest = tokio().block_on(load_work_manifest(&root, "夜航")).unwrap();
        scoped.execution.baseline_work = Some(Arc::new(manifest));
        let result = tokio().block_on(revise_short_fiction_production(&scoped, "夜航")).unwrap();
        assert_eq!(result.revision_changes.chapter_numbers, vec![1]);
        // scoped 分支走 ReviserAgent（submit_chapter_edit_ranges 面在 mock 序可见
        // ——脚本耗尽即证明链路经过）。
        std::fs::remove_dir_all(&root).ok();
    }
}
