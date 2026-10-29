// Models
export { type BookConfig, type Platform, type Genre, type BookStatus, type FanficMode, BookConfigSchema, PlatformSchema, GenreSchema, BookStatusSchema, FanficModeSchema } from "./models/book.js";
export { type ChapterMeta, ChapterMetaSchema, CHAPTER_STATUSES } from "./models/chapter.js";
export { type Observation, ObservationSchema } from "./models/observation.js";
export { type ProjectConfig, type LLMConfig, type NotifyChannel, type DetectionConfig, type AgentLLMOverride, type ResearchSearchConfig, ProjectConfigSchema, LLMConfigSchema, AgentLLMOverrideSchema, DetectionConfigSchema, ResearchSearchConfigSchema } from "./models/project.js";
export { type BookRules, type ParsedBookRules, BookRulesSchema } from "./models/book-rules.js";
export { type DetectionHistoryEntry, type DetectionStats } from "./models/detection.js";
export { type LengthCountingMode, type LengthSpec, type LengthTelemetry, LengthCountingModeSchema, LengthSpecSchema, LengthTelemetrySchema } from "./models/length-governance.js";
// 707 号：timeline 域回收——模型在树（181 号 C4-b），2.0 合并时 index 导出被裁，
// studio 动态导入消费倒查补齐（705 号同型）。
export { type Timeline, type TimelinePlotline, type TimelineCell, TimelineSchema } from "./models/timeline.js";
export {
  type RuntimeStateLanguage,
  type StateManifest,
  type HookStatus,
  type HookRecord,
  type HooksState,
  type ChapterSummaryRow,
  type ChapterSummariesState,
  type CurrentStateFact,
  type CurrentStateState,
  type StateFactInput,
  type StateFactSelector,
  type StateFactOps,
  type HookOps,
  type NewHookCandidate,
  type RuntimeStateDelta,
  RuntimeStateLanguageSchema,
  StateManifestSchema,
  HookStatusSchema,
  HookRecordSchema,
  HooksStateSchema,
  ChapterSummaryRowSchema,
  ChapterSummariesStateSchema,
  CurrentStateFactSchema,
  CurrentStateStateSchema,
  StateFactInputSchema,
  StateFactSelectorSchema,
  StateFactOpsSchema,
  HookOpsSchema,
  NewHookCandidateSchema,
  RuntimeStateDeltaSchema,
} from "./models/runtime-state.js";
export {
  type PlayActionKind,
  type PlayActionIntentInput,
  type PlayActionIntent,
  type PlayEntityType,
  type PlayEntityInput,
  type PlayEntity,
  type PlayVisibility,
  type PlayEdgeInput,
  type PlayEdge,
  type PlayStateSlotKind,
  type PlayStateSlotInput,
  type PlayStateSlot,
  type PlayEvidenceStatus,
  type PlayEvidenceTransitionInput,
  type PlayEvidenceTransition,
  type PlayEventInput,
  type PlayEvent,
  type PlayMutationInput,
  type PlayMutation,
  PlayActionKindSchema,
  PlayActionIntentSchema,
  PlayEntityTypeSchema,
  PlayEntitySchema,
  PlayVisibilitySchema,
  PlayEdgeSchema,
  PlayStateSlotKindSchema,
  PlayStateSlotSchema,
  PlayEvidenceStatusSchema,
  PlayEvidenceTransitionSchema,
  PlayEventSchema,
  PlayMutationSchema,
} from "./models/play.js";
export {
  PlayOpeningStateAgent,
  PlayTurnAgent,
  type PlayOpeningStateInput,
  type PlayTurnInput,
  type PlaySceneRender,
  type PlayTurnResult,
} from "./play/play-agents.js";
export { PlayDB } from "./play/play-db.js";
export { createPlayDB, type PlayGraphDB } from "./play/play-db-factory.js";
export { type PlayGraphSnapshot } from "./play/play-db.js";
export {
  applyPlayMutation,
  type PlayReducerDB,
  type ApplyPlayMutationInput,
  type ApplyPlayMutationResult,
} from "./play/play-reducer.js";
export {
  PlayRunner,
  type PlayOpeningStateLike,
  type PlayTurnLike,
  type PlayRunnerOptions,
  type PlayStepResult,
} from "./play/play-runner.js";
export { PlayStore, type PlayTranscriptTurn, type PlayWorld, type PlayWorldInput, type PlayRunSummary } from "./play/play-store.js";
export { createPlayImageTool } from './harness/tools/play-image.js';
export {
  buildPlayEntityImagePrompt,
  buildPlaySceneImagePrompt,
  playImageContext,
  playSceneImageKey,
  readPlayImageManifest,
  setPlayImageEntry,
  playImageFileName,
  generatePlayImage,
  readPlayImageSettings,
  writePlayImageSettings,
  DEFAULT_PLAY_IMAGE_SETTINGS,
  type PlayImageEntry,
  type PlayImageManifest,
  type PlayImageSettings,
} from "./play/play-image.js";
export {
  type ChapterMemo,
  type ChapterIntent,
  type ContextSource,
  type ContextPackage,
  type ChapterTrace,
  ChapterMemoSchema,
  ChapterIntentSchema,
  ContextSourceSchema,
  ContextPackageSchema,
  ChapterTraceSchema,
} from "./models/input-governance.js";
export {
  AgentSkillSchema,
  createSkillRegistry,
  loadAvailableAgentSkills,
  loadBuiltinAgentSkills,
  loadConfiguredAgentSkills,
  loadExternalAgentSkills,
  parseAgentSkillDocument,
  parseSkillContractVersion,
  skillContractSatisfied,
  SKILL_CONTRACT_VERSION,
  addTrustedSource,
  isTrustedSource,
  normalizeTrustedSourcePath,
  readTrustedSources,
  removeTrustedSource,
  activatedSkillIds,
  mergeActivatedSkillGuidance,
  resolveProfileSkillActivations,
  type AgentSkill,
  type CreateSkillRegistryOptions,
  type ExternalSkillDiagnostic,
  type LoadConfiguredAgentSkillsInput,
  type LoadAvailableAgentSkillsResult,
  type LoadExternalAgentSkillsInput,
  type LoadExternalAgentSkillsResult,
  type ParseAgentSkillDocumentOptions,
  type SkillRegistry,
  type SkillResolutionInput,
  type SkillResolutionResult,
} from "./skills/index.js";
export type { ActivatedSkillGuidance } from "./agent/skill-tool.js";
export { PlannerAgent, type PlanChapterInput, type PlanChapterOutput } from "./agents/planner.js";
export {
  ComposerAgent,
  composeGovernedChapter,
  type ComposeChapterInput,
  type ComposeChapterOutput,
  type BookReferenceContextProvider,
} from "./agents/composer.js";
export {
  bindBookReference,
  listBookReferences,
  loadBookReferenceManifest,
  loadMaterialAsset,
  unbindBookReference,
  type BindBookReferenceInput,
  type BookReferenceBinding,
  type BookReferenceList,
  type BookReferenceManifest,
  type ResolvedBookReference,
} from "./references/book-references.js";
export {
  selectBookReferenceContext,
  type BookReferenceContextSelection,
  type BookReferenceSelectionTask,
  type ReferenceSectionCandidate,
  type ReferenceSectionSelectionRequest,
  type ReferenceSectionSelector,
} from "./references/reference-context.js";
export {
  buildPlannerUserMessage,
  getPlannerMemoSystemPrompt,
} from "./agents/planner-prompts.js";
export {
  buildProxyFetchInit,
  fetchWithProxy,
  resolveProxyUrl,
} from "./utils/proxy-fetch.js";
export { assertSafeBookId, deriveBookIdFromTitle, isSafeBookId } from "./utils/book-id.js";
// 707 号：series 域回收——R20/390 号契约真源（Rust series_canon.rs 1:1 镜像）。
export { SERIES_CANON_VERSION, isValidSeriesId, parseSeriesCanonFile, type SeriesCanonFile } from "./utils/series-canon.js";
export { safeChildPath } from "./utils/path-safety.js";
export { toPosixPath } from "./utils/posix-path.js";
export {
  ActionSourceSchema,
  ActionPayloadSchema,
  CreateBookActionPayloadSchema,
  ContinuationImportActionPayloadSchema,
  FanficCreateActionPayloadSchema,
  GenerateCoverActionPayloadSchema,
  ImitationCreateActionPayloadSchema,
  InteractiveFilmCreateActionPayloadSchema,
  PlayStartActionPayloadSchema,
  RequestedIntentSchema,
  SkillIdSchema,
  ScriptCreateActionPayloadSchema,
  ScriptTargetFormatSchema,
  ShortRunActionPayloadSchema,
  SpinoffCreateActionPayloadSchema,
  StoryboardCreateActionPayloadSchema,
  WriteNextActionPayloadSchema,
  type ActionSource,
  type ActionPayload,
  type RequestedIntent,
  normalizeActionSource,
  normalizeActionPayload,
  normalizeSkillIdList,
  normalizeRequestedIntent,
  normalizePlayMode,
} from "./interaction/action-envelope.js";
export {
  ExecutionStatusSchema,
  ExecutionStateSchema,
  type ExecutionStatus,
  type ExecutionState,
  isTerminalExecutionStatus,
} from "./interaction/events.js";
export {
  PendingProposedActionSchema,
  InteractionMessageSchema,
  InteractionSessionSchema,
  type PendingProposedAction,
  type InteractionMessage,
  type InteractionSession,
  bindActiveBook,
  appendInteractionMessage,
  BookSessionSchema,
  SessionKindSchema,
  PlayModeSchema,
  type BookSession,
  type SessionKind,
  type PlayMode,
  createBookSession,
  appendBookSessionMessage,
} from "./interaction/session.js";
export {
  resolveProjectSessionPath,
  createProjectSession,
  loadProjectSession,
  persistProjectSession,
  resolveSessionActiveBook,
} from "./interaction/project-session-store.js";
export {
  loadBookSession,
  persistBookSession,
  listBookSessions,
  renameBookSession,
  deleteBookSession,
  bindBookSessionToBook,
  transitionSessionToWork,
  createAndPersistBookSession,
  SessionAlreadyBoundError,
} from "./interaction/book-session-store.js";
export {
  appendManualSessionMessages,
  appendTranscriptEvent,
  sessionsDir,
  readTranscriptEvents,
  confirmedRequestInstruction,
  nextTranscriptSeq,
  transcriptPath,
} from "./interaction/session-transcript.js";
export {
  cleanRestoredAgentMessages,
  committedMessageEvents,
  deriveBookSessionFromTranscript,
  restoreAgentMessagesFromTranscript,
} from "./interaction/session-transcript-restore.js";
export {
  MessageEventSchema,
  RequestCommittedEventSchema,
  RequestFailedEventSchema,
  RequestStartedEventSchema,
  SessionCreatedEventSchema,
  SessionMetadataUpdatedEventSchema,
  TranscriptEventSchema,
} from "./interaction/session-transcript-schema.js";
export type {
  TranscriptEvent,
  MessageEvent,
  RequestCommittedEvent,
  RequestFailedEvent,
  RequestStartedEvent,
  SessionCreatedEvent,
  SessionMetadataUpdatedEvent,
} from "./interaction/session-transcript-schema.js";
export { buildExportArtifact, writeExportArtifact, ChapterExportSourceError } from "./interaction/export-artifact.js";
export {
  normalizeTruthFileName,
  classifyTruthAuthority,
  type TruthAuthority,
} from "./interaction/truth-authority.js";
export {
  executeEditTransaction,
  planEditTransaction,
  type EditRequest,
  type EditExecutionDeps,
  type ExecutedEditTransaction,
  type PlannedEditTransaction,
} from "./interaction/edit-controller.js";
export {
  SHORT_FICTION_DEFAULT_CHAPTERS,
  SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER,
  SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER,
  ShortFictionOutlineAgent,
  ShortFictionWriterAgent,
  ShortFictionDraftReviewerAgent,
  ShortFictionPackagingAgent,
  validateShortFictionDraftForFinal,
  renderShortFictionDraftMarkdown,
  type ShortFictionOutline,
  type ShortFictionBatchDraft,
  type ShortFictionChapter,
  type ShortFictionSalesPackage,
  type ShortFictionReference,
  type ShortFictionLanguage,
} from "./agents/short-fiction.js";
export {
  generateShortFictionCover,
  runShortFictionProduction,
  reviseShortFictionProduction,
  extractResponsesImageBase64,
  resolveCoverApiKey,
  type ShortFictionCoverOptions,
  type ShortFictionCoverResult,
  type ShortFictionRunOptions,
  type ShortFictionRunResult,
  type ShortFictionRunRuntimes,
} from "./pipeline/short-fiction-runner.js";

// Narrative forecast (issue #342): non-canonical multi-branch story projection
export {
  FORECAST_DEFAULT_BRANCHES,
  FORECAST_DEFAULT_HORIZON,
  NarrativeForecastSchema,
  ForecastBranchSchema,
  type NarrativeForecast,
  type ForecastBranch,
  type ForecastBeat,
  type ForecastRisk,
  type ForecastStatus,
  type ForecastModelOutput,
} from "./forecast/schema.js";
export { ForecastStore, assertSafeForecastId, type ForecastStoreOptions } from "./forecast/store.js";
export {
  buildForecastContext,
  computeContextFingerprint,
  renderForecastContextMarkdown,
  type ForecastContext,
  type ForecastContextSections,
} from "./forecast/context-builder.js";
export { NarrativeForecastAgent, type ForecastGenerationInput } from "./forecast/agent.js";
export { renderForecastComparisonMarkdown, renderSelectedBranchPlanMarkdown } from "./forecast/render.js";
export {
  createNarrativeForecast,
  getNarrativeForecast,
  selectNarrativeBranch,
  type CreateNarrativeForecastOptions,
  type GetNarrativeForecastOptions,
  type SelectNarrativeBranchOptions,
  type NarrativeForecastCreateResult,
  type NarrativeForecastGetResult,
  type NarrativeForecastSelectResult,
} from "./forecast/runner.js";

// Agent (pi-agent integration)
export * from "./agent/index.js";

// LLM
export { createLLMClient, chatCompletion, createStreamMonitor, PartialResponseError, type LLMClient, type LLMResponse, type LLMMessage, type StreamProgress, type OnStreamProgress } from "./llm/provider.js";
export {
  SERVICE_TO_PI_PROVIDER,
  resolveServicePreset,
  resolveServiceProviderFamily,
  resolveServicePiProvider,
  resolveServiceModelsBaseUrl,
  guessServiceFromBaseUrl,
  listModelsForService,
  listServicesWithModelCount,
  type ServicePreset,
  type ModelInfo,
} from "./llm/service-presets.js";
export { resolveServiceModel, ServiceApiKeyNotFoundError, type ResolvedModel } from "./llm/service-resolver.js";
export { loadSecrets, saveSecrets, getServiceApiKey, type SecretsFile } from "./llm/secrets.js";
export {
  COVER_PROVIDER_PRESETS,
  coverSecretKey,
  normalizeCoverBaseUrl,
  resolveCoverProviderPreset,
  type CoverProviderId,
  type CoverProviderPreset,
} from "./llm/cover-providers.js";
export { getAllEndpoints, getEndpoint, type InkosEndpoint, type InkosModel, type EndpointGroup } from "./llm/providers/index.js";
export { probeModelsFromUpstream, type ProbedModel } from "./llm/providers/probe.js";

// Agents
export { BaseAgent, type AgentContext } from "./agents/base.js";
export { ArchitectAgent, type ArchitectOutput } from "./agents/architect.js";
export { WriterAgent, type WriteChapterInput, type WriteChapterOutput, type TokenUsage } from "./agents/writer.js";
export { ContinuityAuditor, type AuditResult } from "./agents/continuity.js";
export { ReviserAgent, DEFAULT_REVISE_MODE, type ReviseOutput, type ReviseMode } from "./agents/reviser.js";
// 707 号：选后再析恢复（G14a/335）——fetchRankings/filterRankingsBySelection/
// RadarSelection 随 radar 域回收补齐（2.0 合并裁撤、Rust radar.rs 全量保留）。
export { RadarAgent, fetchRankings, filterRankingsBySelection, type RadarResult, type RadarRecommendation, type RadarSelection } from "./agents/radar.js";
export { FanqieRadarSource, QidianRadarSource, TextRadarSource, type RadarSource, type PlatformRankings, type RankingEntry } from "./agents/radar-source.js";
export { readBookRules } from "./agents/rules-reader.js";
export { buildWriterSystemPrompt } from "./agents/writer-prompts.js";
export { detectAIContent, type DetectionResult } from "./agents/detector.js";
export { analyzeDetectionInsights } from "./agents/detection-insights.js";
export { buildSettlerSystemPrompt, buildSettlerUserPrompt } from "./agents/settler-prompts.js";
export { FanficCanonImporter, type FanficCanonOutput } from "./agents/fanfic-canon-importer.js";
export * from "./prompts/short-fiction.js";

// Utils
export { isBookFoundationComplete } from "./utils/outline-paths.js";
export { fetchUrl, searchWeb } from "./utils/web-search.js";
export {
  runResearchReport,
  type ResearchDepth,
  type ResearchInput,
  type ResearchPurpose,
  type ResearchReport,
} from "./agents/researcher.js";
export { StateValidatorAgent } from "./agents/state-validator.js";
export { createInitialRuntimeState, loadRuntimeStateSnapshot, buildRuntimeStateArtifacts, saveRuntimeStateSnapshot, type RuntimeStateArtifacts } from "./state/runtime-state-store.js";
export { splitChapters, type SplitChapter } from "./utils/chapter-splitter.js";
export * from "./translation/index.js";
export { countChapterLength, resolveLengthCountingMode, formatLengthCount, buildLengthSpec, defaultChapterLength, DEFAULT_CHAPTER_LENGTH_ZH, DEFAULT_CHAPTER_LENGTH_EN, type LengthLanguage } from "./utils/length-metrics.js";
export { createLogger, createStderrSink, createJsonLineSink, nullSink, type Logger, type LogSink, type LogLevel, type LogEntry } from "./utils/logger.js";
export { loadProjectConfig, GLOBAL_CONFIG_DIR, GLOBAL_ENV_PATH, isApiKeyOptionalForEndpoint } from "./utils/config-loader.js";
export { resolveEffectiveLLMConfig, LLMConfigurationError, type EffectiveLLMConfigResult, type EffectiveLLMDiagnostics, type LLMConfigCliOverrides, type LLMConfigMode, type LLMConsumer, type LLMValueSource } from "./utils/effective-llm-config.js";
export { loadLLMEnvLayers, mergeEnvMaps, studioIgnoredEnv, mergedLLMEnv, type LLMEnvLayers, type LLMEnvMap } from "./utils/llm-env.js";
export type { ContextCompressionCallback, ContextCompressionCategory, ContextCompressionEvent, ContextCompressionPhase } from "./models/context-compression.js";
export { computeAnalytics, type AnalyticsData, type TokenStats } from "./utils/analytics.js";
export { arbitrateRuntimeStateDeltaHooks, type HookArbiterDecision } from "./utils/hook-arbiter.js";

// Pipeline
export { PipelineRunner, type PipelineConfig, type ChapterPipelineResult, type WriteChaptersOptions, type ReviseResult, type ImportChaptersInput, type ImportChaptersResult, type TokenUsageSummary } from "./pipeline/runner.js";
export { commitAtomicFileSet, recoverAtomicFileSets } from "./utils/atomic-file-set.js";
// 707 号：timeline/series 域回收——199 号单文件原子替换 helper（Rust
// write_file_atomic 同构；2.0 合并时 index 导出被裁）。
export { writeTextAtomic } from "./utils/atomic-write.js";
export { Scheduler, type SchedulerConfig } from "./pipeline/scheduler.js";
export { detectChapter, loadDetectionHistory, type DetectChapterResult } from "./pipeline/detection-runner.js";
export { runScriptCreation, runStoryboardCreation, runInteractiveFilmCreation, createStoryboardAssetsManifest, type ScriptCreationRunOptions, type ScriptCreationRunResult, type StoryboardAssetsManifest, type StoryboardCreationRunOptions, type StoryboardCreationRunResult, type InteractiveFilmCreationRunOptions, type InteractiveFilmCreationRunResult, type StoryboardImageAsset, type StoryboardImageAssetVariant } from "./pipeline/script-storyboard-runner.js";
export { ScriptCreationAgent, StoryboardCreationAgent, InteractiveFilmCreationAgent, renderScriptSpec, renderStoryboardSpec, renderInteractiveFilmSpec, type ScriptCreationInput, type ScriptTargetFormat, type StoryboardCreationInput, type InteractiveFilmCreationInput } from "./agents/script-storyboard.js";

// State
export { BookWriteLockError, StateManager } from "./state/manager.js";
export { syncChapterWordCounts, type ChapterWordCountChange, type ChapterWordSyncDeps, type ChapterWordSyncResult } from "./state/chapter-word-sync.js";
export { deleteLatestChapter, type ChapterDeleteDeps, type DeleteLatestChapterOptions, type DeleteLatestChapterResult } from "./state/chapter-delete.js";
export {
  archiveChapterVersion,
  listChapterVersions,
  readChapterPlanDocument,
  readChapterUserBrief,
  readChapterVersion,
  saveChapterUserBrief,
  type ChapterVersion,
  type ChapterVersionSource,
} from "./state/chapter-workspace.js";
export { loadChaptersFromPath, compareChapterSourceNames } from "./agent/chapter-import-source.js";
export { renderCurrentStateProjection, renderHooksProjection, renderChapterSummariesProjection } from "./state/state-projections.js";
export { applyRuntimeStateDelta, type RuntimeStateSnapshot } from "./state/state-reducer.js";
export { validateRuntimeState, type RuntimeStateValidationIssue } from "./state/state-validator.js";

// Notify
export { dispatchNotification, dispatchWebhookEvent, type NotifyMessage } from "./notify/dispatcher.js";
export type { NotifyFormat } from "./notify/format.js";
export type { TelegramConfig } from "./notify/telegram.js";
export type { FeishuConfig } from "./notify/feishu.js";
export type { WechatWorkConfig } from "./notify/wechat-work.js";
export type { WebhookConfig, WebhookEvent, WebhookPayload } from "./notify/webhook.js";

export async function sendTelegram(
  config: import("./notify/telegram.js").TelegramConfig,
  message: string,
  format?: import("./notify/format.js").NotifyFormat,
): Promise<void> {
  const transport = await import("./notify/telegram.js");
  await transport.sendTelegram(config, message, format);
}

export async function sendFeishu(
  config: import("./notify/feishu.js").FeishuConfig,
  title: string,
  text: string,
  format?: import("./notify/format.js").NotifyFormat,
): Promise<void> {
  const transport = await import("./notify/feishu.js");
  await transport.sendFeishu(config, title, text, format);
}

export async function sendWechatWork(
  config: import("./notify/wechat-work.js").WechatWorkConfig,
  text: string,
  format?: import("./notify/format.js").NotifyFormat,
): Promise<void> {
  const transport = await import("./notify/wechat-work.js");
  await transport.sendWechatWork(config, text, format);
}

export async function sendWebhook(
  config: import("./notify/webhook.js").WebhookConfig,
  payload: import("./notify/webhook.js").WebhookPayload,
): Promise<void> {
  const transport = await import("./notify/webhook.js");
  await transport.sendWebhook(config, payload);
}

// ── Interactive Film (story graph) ──
export {
  StoryGraphSchema,
  StoryNodeSchema,
  ChoiceSchema,
  VariableSchema,
  EndingSchema,
  ConditionSchema,
  EffectSchema,
  type StoryGraph,
  type StoryNode,
  type Choice,
  type Variable,
  type Ending,
  type Condition,
  type Effect,
  type VarValue,
  type NodeType,
} from "./interactive-film/graph-schema.js";
export {
  evaluateCondition,
  applyEffects,
  visibleChoices,
  initVarState,
  type VarState,
} from "./interactive-film/evaluator.js";
export {
  validateStoryGraph,
  reviewStoryGraph,
  type ValidationReport,
  type ValidationIssue,
} from "./interactive-film/validation.js";
export {
  loadStoryGraph,
  saveStoryGraph,
  storyGraphPath,
} from "./interactive-film/graph-store.js";
export {
  materializeStoryGraph,
  type MaterializeStoryGraphInput,
} from "./interactive-film/generate.js";
export {
  WorldAnchorSchema,
  CharacterSchema,
  VoiceProfileSchema,
  type WorldAnchor,
  type Character,
  type VoiceProfile,
} from "./interactive-film/graph-schema.js";
export {
  StoryGraphDeltaSchema,
  applyStoryGraphDelta,
  type StoryGraphDelta,
} from "./interactive-film/delta.js";
export {
  applyGraphDelta,
  loadAuthoringState,
  revertToSnapshot,
  authoringStatePath,
  type AuthoringState,
} from "./interactive-film/authoring-store.js";
export {
  buildWorldAnchorDelta,
  buildAddVariableDelta,
  buildDefineEndingDelta,
  buildRemoveNodeDelta,
  buildConnectChoiceDelta,
  buildUpsertCharactersDelta,
} from "./interactive-film/authoring-tools.js";
export { summarizeStoryGraph, buildFilmAuthoringContext } from "./interactive-film/film-context.js";
export {
  generateNodeImage,
  defaultNodeImageDeps,
  type NodeImageDeps,
} from "./interactive-film/node-image.js";
export {
  enumerateRuntimePaths,
  type RuntimePath,
} from "./interactive-film/paths.js";
export { analyzePathDistribution } from "./interactive-film/path-analysis.js";
export { exportInk } from "./interactive-film/export-ink.js";
export { buildPlayableHtml } from "./interactive-film/export-html.js";
export { ingestMaterial, type IngestMaterialInput, type MaterialAsset } from "./materials/ingest.js";
export { runWorkerAgent, type WorkerAgentOptions } from "./agent/worker-agent.js";
export { compileStyleGuide } from "./agents/style-guide.js";
export { LLM_API_FORMATS, isLLMApiFormat, toPiApi, type LLMApiFormat } from "./llm/api-format.js";
export * from "./harness/index.js";

// ── R36 会话树化（636-639 号；692 号重施——studio 分支切换器消费面）──
export {
  branchBookSession,
  deriveSessionBranchPoints,
  BranchTargetNotFoundError,
  type BranchBookSessionResult,
  type SessionBranchPoint,
  type SessionBranchPointsResult,
} from "./interaction/session-branch.js";

// ── 702 号：只读 MemoryDB Mini 端口（legacy memory.db SELECT 面）──
export { MemoryDBReadonly, type StoredSummary, type StoredReviewMetric, type StoredQualityDebt } from "./state/memory-db-readonly.js";
export { extractCharacterCandidates, parseEntityRoster, resolveRosterCandidates, renderEntityRoster, applyRosterConfirmation, type RosterEntity } from "./utils/entity-roster.js";

// ── 701 号：fork 分析投影面回收（承诺账本/张力/遥测/反AI 种子——studio 路由消费）──
export { globalRunLog, projectRunLog } from "./utils/run-log.js";
export {
  buildPromiseTimeline,
  detectPacingDebts,
  detectWeakHookRuns,
  resolvePromiseUrgency,
} from "./utils/promise-ledger.js";
export { buildQualityTrend } from "./utils/quality-trend.js";
export { listAvailableGenres, readGenreProfile, getBuiltinGenresDir } from "./agents/genre-reader.js";
export type { GenreProfile } from "./models/genre-profile.js";
export { analyzeTensionCurve, parseChapterSummariesMarkdown, buildTensionCurve, detectTensionWarnings } from "./utils/tension-curve.js";
export {
  ANTI_AI_RULE_SEEDS,
  validateAntiAiRule,
  type AntiAiRule,
  type RuleValidateResult,
} from "./utils/rule-experience-engine.js";
// ── 705 号：fork 导出恢复（G5/351 拆书 + G13 经验合并；2.0 合并时模块存活而 index 导出被裁，
// studio 写面路由经 @actalk/inkos-core 动态导入消费）──
export {
  analyzePacingStats,
  buildDeconstructionExport,
  buildEvidenceIndex,
  deriveCharacterDossier,
  renderDeconstructionExport,
  DECONSTRUCTION_DEPTHS,
  type DeconstructionDepth,
  type DeconChapter,
  type PacingStats,
} from "./utils/deconstruction.js";
export {
  antiAiRuleCanonicalForm,
  antiAiRuleContentHash,
  antiAiRulePackHash,
  composeAntiAiGuidance,
  mergeExperienceEntries,
  renderAntiAiFixGuidance,
  renderExperienceGuidance,
  resolveAntiAiRulesWithSeeds,
  scanAntiAiRules,
  type AntiAiHit,
  type AntiAiRuleType,
  type AntiAiRulesWithSeeds,
  type AntiAiSeverity,
  type ExperienceEntry,
  type ExperienceKind,
} from "./utils/rule-experience-engine.js";

// ── fork 资产库（G4/339 号系列；2.0 无此面，studio asset-library-store 消费）──
export {
  assetContentHash,
  buildAssetLibraryExport,
  GENRE_BASE_SEEDS,
  PROGRESSION_MODE_SEEDS,
  mergeAssetLibrary,
  parseAssetLibraryImport,
  previewAssetLibraryImport,
  renderAdoptionHeader,
  renderAssetGuidanceBlock,
  sortAssets,
  validateLibraryAsset,
  ASSET_LIBRARY_VERSION,
  type AssetImportResult,
  type AssetKind,
  type AssetLibraryPackage,
  type AssetMergeResult,
  type AssetValidateResult,
  type LibraryAsset,
} from "./utils/asset-library.js";

// ── 自动导演产品化契约（G6/352 号系列；DirectorPanel/directions 面消费）──
export {
  InspirationCardSchema,
  buildDirectionCandidatesPrompt,
  parseDirectionCandidates,
  resolveDirectorRunPlan,
  nextDirectorStage,
  DIRECTOR_CONTRACT,
  DIRECTOR_RUN_MODES,
  DIRECTOR_STAGES,
  type DirectionCandidate,
  type DirectorContract,
  type DirectorRunMode,
  type DirectorRunPlan,
  type DirectorStage,
  type InspirationCard,
} from "./models/director.js";

// ── 续跑建议（G9/344 号；GET /books/:id/director 的 resumeAdvice 消费面）──
export {
  formatResumeHint,
  resolveResumeAdvice,
  RESUME_ACTIONS,
  type ResumeAction,
  type ResumeAdvice,
  type ResumeAdviceInput,
} from "./utils/resume-advice.js";

// R41/564 号：token 计量服务（TS 镜像，563 号 Rust 侧同批语义；双端共享 golden）。
export { TokenMeter } from "./utils/token-meter.js";
export type {
  MeterSource,
  SurfaceNode,
  UsageAnchor,
  TokenMeterSnapshot,
} from "./utils/token-meter.js";

// ── 715 号：卷级归并 + story 台账 Markdown 族（ConsolidatorAgent 消费面；
//    双端对照 leaf.json render_*_snapshot 向量）──
export { ConsolidatorAgent, type ConsolidationResult } from "./agents/consolidator.js";
// parseChapterSummariesMarkdown 不在此重复导出——tension-curve.js 已有同构
// 版本在导出面（2.0 独立演进），story-markdown 内部版本供模块族自洽。
export {
  renderSummarySnapshot,
  renderHookSnapshot,
  parsePendingHooksMarkdown,
  parseCurrentStateFacts,
} from "./utils/story-markdown.js";
export { rerunPromotionPass, type PromotionPassResult } from "./utils/hook-promotion.js";

// ── 716 号：实体卡 Codex 化（R10/375 系；CodexPanel 消费面）+ Context Lens
//    （R21/391 上下文装配透明投影；ContextLensPanel 消费面）——模块在树导出被裁。──
export {
  deriveCodexCards,
  matchCodexCards,
  renderCodexBlock,
  reviseCodexCard,
  CODEX_LIMITS,
  type CodexMatch,
  type CodexRelationship,
  type EntityCodexCard,
} from "./utils/entity-codex.js";
export {
  buildContextLens,
  CONTEXT_LENS_VERSION,
  type ContextLens,
  type ContextLensCompression,
  type ContextLensEntry,
  type ContextLensRank,
} from "./utils/context-lens.js";

// ── 717 号：写作质量面（BookDetail 重修设定/质量评估/反检测消费）——
//    ai-tells 结构规则检测（零 LLM）+ book-eval 启发式质量评估 + 生产能力
//    绑定表（draft 管线激活透传）。双端对偶 ai_tells.rs / book_eval.rs。──
export { analyzeAITells, type AITellIssue, type AITellResult } from "./agents/ai-tells.js";
export {
  evaluateBookQuality,
  computeChapterEvalScore,
  type ChapterEval,
  type BookEval,
  type EvaluateBookQualityOptions,
} from "./utils/book-eval.js";
export {
  resolveProductionSkillActivations,
  PRODUCTION_SKILL_IDS,
  NON_LONG_PRODUCTION_CAPABILITIES,
  type ProductionSkillCapability,
} from "./skills/production-bindings.js";
export { findChapterFileByNumber } from "./state/chapter-filename.js";

// ── 718 号：混合检索面（RecallTestCard 召回测试消费）——FTS 索引当前演进版
//    在树 + RRF 融合/向量相似度/嵌入客户端；MemoryDB 语义缓存 2.0 未接线
//    （hybrid-search 路由语义面降级 fts5-fallback，接线后自动升级）。──
export { LocalSearchIndex, tokenizeSearchText, type SearchDocument, type SearchHit } from "./retrieval/local-search.js";
export { createEmbeddingClient, type EmbeddingConfig } from "./retrieval/embedding-client.js";
export type { EmbeddingClient } from "./retrieval/semantic-retrieval.js";
export { topKBySimilarity, reciprocalRankFusion } from "./retrieval/semantic-retrieval.js";
