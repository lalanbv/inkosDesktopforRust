// 聊天会话工具装配统一出口（544 号施工图 §2.2 / 555 号）——R38b Rust 侧注册表
// 收拢的 TS 对应面。此前装配逻辑内联在 agent-session.ts（createModeTools 会话
// 种类分支），无独立出口、无目录投影；本模块将其收拢为 buildChatToolSet 单点，
// 并提供 buildChatToolCatalog 供 debug/tools 端点做双端目录机械对照。
//
// 与 Rust 侧 interaction/registry.rs 的对照框架（555 号定案）：
// - 目录 = "书会话有效工具目录"——文件三件取书作用域变体（Rust ToolScope::
//   BookSession 遮蔽 Project 层的同一裁决），项目作用域三件（202 号回环内面）
//   不入聊天目录；
// - Rust 注册表 31 唯一名全部在本目录（差分器 rust-only=空集硬要求）；
// - node-only 件（四建书件 + 六一次性生产件 + play_start + film 三一次性件）
//   为设计内不对称（Rust 走 propose→confirm + 端点，agent_route.rs 备案），
//   差分器按豁免表放行。
// 工厂本体零改动（施工图"不强改 32 工厂"条款）：目录元数据经桩依赖实例化
// 工厂提取——工厂仍是 schema/描述单一事实源，零漂移可能。
import { createHash } from "node:crypto";

import type { AgentTool } from "@earendil-works/pi-agent-core";

import type { PipelineRunner } from "../pipeline/runner.js";
import type { PlayMode, SessionKind } from "../interaction/session.js";
import type { ActionPayload, ActionSource, RequestedIntent } from "../interaction/action-envelope.js";
import type { ActivatedSkillGuidance } from "./skill-tool.js";
import { createUseSkillTool } from "./skill-tool.js";
import { mergeActivatedSkillGuidance } from "../skills/index.js";
import type { ProductionSkillCapability, SkillRegistry } from "../skills/index.js";
import {
  createPatchChapterTextTool,
  createReplaceChapterTextTool,
  createResyncChapterStateTool,
  createDeleteLatestChapterTool,
  createRenameEntityTool,
  createSubAgentTool,
  createReadTool,
  createGrepTool,
  createLsTool,
  createWriteTruthFileTool,
  createShortFictionRunTool,
  createGenerateCoverTool,
  createPlayEditTool,
  createPlayReviseTool,
  createPlayStartTool,
  createPlayStepTool,
  createProposeActionTool,
  createScriptCreationTool,
  createStoryboardCreationTool,
  createInteractiveFilmCreationTool,
  createTranslationCreateTool,
  createFanficBookTool,
  createContinuationImportTool,
  createSpinoffBookTool,
  createImitationBookTool,
  createResearchWebTool,
  createIngestMaterialTool,
  createRetrieveMaterialTool,
  createManageBookReferenceTool,
  createImportChaptersTool,
} from "./agent-tools.js";
import { createFilmAuthoringTools, filmLLMDepsFromClient, type FilmLLMDeps } from "./film-authoring-tools.js";
import {
  createNarrativeForecastCreateTool,
  createNarrativeForecastGetTool,
  createNarrativeForecastSelectTool,
} from "./forecast-tools.js";

// ---------------------------------------------------------------------------
// 生产变更面剔除名单（agent-session.ts 原位搬移；Rust 侧 R38b 已由注册表
// MutationKind::ProductionMutation 承接，TS 侧维持名单形态——工厂不强改条款，
// MutationKind 化备案顺延）
// ---------------------------------------------------------------------------

/**
 * 会创建/修改书籍与产物的生产工具。suppressProductionTools 为 true（同会话
 * 有后台生产任务在运行）时从工具表剔除；read/grep/ls、research/material 与
 * propose_action 保留——propose_action 引发的确认任务在 host 侧另有单任务闸门。
 */
const PRODUCTION_MUTATION_TOOL_NAMES = new Set([
  "sub_agent",
  "generate_cover",
  "write_truth_file",
  "rename_entity",
  "patch_chapter_text",
  "replace_chapter_text",
  "resync_chapter_state",
  "delete_latest_chapter",
  "import_chapters",
  "fanfic_create",
  "continuation_import",
  "spinoff_create",
  "imitation_create",
]);

/** 生产变更面名单单点判定（suppression 消费面；与名单同源零漂移）。 */
export function isProductionMutationToolName(name: string): boolean {
  return PRODUCTION_MUTATION_TOOL_NAMES.has(name);
}

// ---------------------------------------------------------------------------
// 会话工具装配（agent-session.ts createAgentToolsForMode/createModeTools 原位
// 搬移，逻辑零改动）
// ---------------------------------------------------------------------------

export type ChatToolSetParams = {
  readonly pipeline: PipelineRunner;
  readonly bookId: string | null;
  readonly sessionId: string;
  readonly sessionKind: SessionKind;
  readonly actionSource: ActionSource;
  readonly requestedIntent: RequestedIntent | undefined;
  readonly actionPayload: ActionPayload | undefined;
  readonly projectRoot: string;
  readonly allowSystemFileRead: boolean;
  readonly language: string;
  readonly playMode?: PlayMode;
  readonly playWorldExists: boolean;
  readonly intentSkillTool?: ReturnType<typeof createUseSkillTool>;
  readonly requestedSkillIds?: () => ReadonlyArray<string>;
  readonly attachmentPaths?: () => ReadonlyArray<string>;
  readonly activeSkills?: () => ReadonlyArray<ActivatedSkillGuidance>;
  readonly workerSkills?: (agent: string) => ReadonlyArray<ActivatedSkillGuidance>;
  readonly productionSkills?: (capability: ProductionSkillCapability) => ReadonlyArray<ActivatedSkillGuidance>;
};

/**
 * 按会话参数装配本轮 agent 工具表（聊天工具统一出口）。
 */
export function buildChatToolSet(params: ChatToolSetParams): AgentTool<any>[] {
  const tools = createModeTools(params);
  return params.intentSkillTool ? [...tools, params.intentSkillTool] : tools;
}

function createModeTools(params: ChatToolSetParams) {
  const lang = params.language === "en" ? "en" : "zh";
  const subAgentTool = createSubAgentTool(params.pipeline, params.bookId, params.projectRoot, {
    actionPayload: params.actionPayload,
    language: lang,
    activeSkills: params.activeSkills,
    workerSkills: params.workerSkills,
  });
  const proposalTool = createProposeActionTool(lang, {
    sameSession: params.sessionKind !== "chat",
    requestedSkillIds: params.requestedSkillIds,
    attachmentPaths: params.attachmentPaths,
  });
  const researchTool = createResearchWebTool(params.projectRoot);
  const materialTool = createIngestMaterialTool(params.projectRoot);
  const materialRetrievalTool = createRetrieveMaterialTool(params.projectRoot);
  const projectReadTool = createReadTool(params.projectRoot, { scope: "project" });
  const importChaptersTool = createImportChaptersTool(params.pipeline, params.bookId, params.projectRoot);
  const isConfirmed = (intent: RequestedIntent): boolean => {
    return (params.actionSource === "button" || params.actionSource === "slash")
      && params.requestedIntent === intent;
  };

  if (params.sessionKind === "chat") {
    if (isConfirmed("translation_create")) {
      return [createTranslationCreateTool(params.projectRoot, { actionPayload: params.actionPayload })];
    }
    if (isConfirmed("fanfic_init")) {
      return [createFanficBookTool(params.pipeline, params.projectRoot, {
        defaultSkills: params.productionSkills?.("longWriting"),
        activeSkills: params.activeSkills,
      })];
    }
    if (isConfirmed("continuation_import")) {
      return [createContinuationImportTool(params.pipeline, params.bookId, params.projectRoot, {
        defaultSkills: params.productionSkills?.("longWriting"),
        activeSkills: params.activeSkills,
      })];
    }
    if (isConfirmed("spinoff_create")) {
      return [createSpinoffBookTool(params.pipeline, params.projectRoot, {
        defaultSkills: params.productionSkills?.("longWriting"),
        activeSkills: params.activeSkills,
      })];
    }
    if (isConfirmed("style_imitation")) {
      return [createImitationBookTool(params.pipeline, params.projectRoot, {
        defaultSkills: params.productionSkills?.("longWriting"),
        activeSkills: params.activeSkills,
      })];
    }
    return [proposalTool, researchTool, materialTool, materialRetrievalTool, importChaptersTool];
  }

  if (params.sessionKind === "short") {
    if (isConfirmed("short_run")) {
      return [createShortFictionRunTool(params.pipeline, params.projectRoot, {
        actionPayload: params.actionPayload,
        language: lang,
        defaultSkills: params.productionSkills?.("shortWriting"),
        activeSkills: params.activeSkills,
      })];
    }
    if (isConfirmed("generate_cover")) {
      return [createGenerateCoverTool(params.projectRoot, { actionPayload: params.actionPayload })];
    }
    return [proposalTool, materialTool, materialRetrievalTool];
  }

  if (params.sessionKind === "script") {
    if (isConfirmed("script_create")) {
      return [createScriptCreationTool(params.pipeline, params.projectRoot, {
        actionPayload: params.actionPayload,
        language: lang,
        defaultSkills: params.productionSkills?.("script"),
        activeSkills: params.activeSkills,
      })];
    }
    return [proposalTool, projectReadTool, materialTool, materialRetrievalTool];
  }

  if (params.sessionKind === "storyboard") {
    if (isConfirmed("storyboard_create")) {
      return [createStoryboardCreationTool(params.pipeline, params.projectRoot, {
        actionPayload: params.actionPayload,
        language: lang,
        defaultSkills: params.productionSkills?.("storyboard"),
        activeSkills: params.activeSkills,
      })];
    }
    return [proposalTool, projectReadTool, materialTool, materialRetrievalTool];
  }

  if (params.sessionKind === "interactive-film") {
    if (isConfirmed("interactive_film_create")) {
      return [createInteractiveFilmCreationTool(params.pipeline, params.projectRoot, {
        actionPayload: params.actionPayload,
        language: lang,
        defaultSkills: params.productionSkills?.("interactiveFilm"),
        activeSkills: params.activeSkills,
      })];
    }
    return [proposalTool, projectReadTool, materialTool, materialRetrievalTool];
  }

  if (params.sessionKind === "interactive-film-authoring") {
    const projectId = params.bookId;
    if (!projectId) {
      throw new Error("interactive-film-authoring session requires a non-null bookId");
    }
    const agentCtx = params.pipeline.createAgentContext("film-authoring", projectId);
    const llm = filmLLMDepsFromClient(agentCtx.client, agentCtx.model, {
      activatedSkills: () => mergeActivatedSkillGuidance(
        params.productionSkills?.("interactiveFilm") ?? [],
        params.activeSkills?.() ?? [],
      ),
    });
    return createFilmAuthoringTools({
      projectRoot: params.projectRoot,
      projectId,
      llm,
      proposeActionTool: proposalTool,
      confirmedIntent: params.requestedIntent,
      language: lang,
    });
  }


  if (params.sessionKind === "play") {
    if (isConfirmed("play_start")) {
      return [createPlayStartTool(params.pipeline, params.projectRoot, params.sessionId, params.playMode, {
        actionPayload: params.actionPayload,
        defaultSkills: params.productionSkills?.("play"),
        activeSkills: params.activeSkills,
      })];
    }
    if (params.playWorldExists) {
      return [
        createPlayEditTool(params.projectRoot, params.sessionId, lang),
        createPlayReviseTool(params.pipeline, params.projectRoot, params.sessionId, {
          language: lang,
          defaultSkills: params.productionSkills?.("play"),
          activeSkills: params.activeSkills,
        }),
        createPlayStepTool(params.pipeline, params.projectRoot, params.sessionId, {
          language: lang,
          defaultSkills: params.productionSkills?.("play"),
          activeSkills: params.activeSkills,
        }),
        materialTool,
        materialRetrievalTool,
      ];
    }
    return [proposalTool, materialTool, materialRetrievalTool];
  }

  if (params.sessionKind === "book-create" && !params.bookId) {
    if (isConfirmed("create_book")) {
      return [createSubAgentTool(params.pipeline, params.bookId, params.projectRoot, {
        actionPayload: params.actionPayload,
        architectCreateOnly: true,
        language: lang,
        activeSkills: params.activeSkills,
        workerSkills: params.workerSkills,
      })];
    }
    return [proposalTool, researchTool, materialTool, materialRetrievalTool];
  }

  if (!params.bookId) {
    return [];
  }

  const bookTools = [
    subAgentTool,
    createGenerateCoverTool(params.projectRoot, { actionPayload: params.actionPayload }),
    createReadTool(params.projectRoot, { allowSystemPaths: params.allowSystemFileRead }),
    createWriteTruthFileTool(params.pipeline, params.projectRoot, params.bookId),
    createRenameEntityTool(params.pipeline, params.projectRoot, params.bookId),
    createPatchChapterTextTool(params.pipeline, params.projectRoot, params.bookId),
    createReplaceChapterTextTool(params.pipeline, params.projectRoot, params.bookId),
    createResyncChapterStateTool(params.pipeline, params.bookId, {
      language: lang,
      defaultSkills: params.productionSkills?.("longWriting"),
      activeSkills: params.activeSkills,
    }),
    createDeleteLatestChapterTool(params.projectRoot, params.bookId),
    researchTool,
    materialTool,
    materialRetrievalTool,
    createManageBookReferenceTool(params.projectRoot, params.bookId),
    importChaptersTool,
    createNarrativeForecastCreateTool(params.pipeline, params.bookId, params.projectRoot),
    createNarrativeForecastGetTool(params.bookId, params.projectRoot),
    createNarrativeForecastSelectTool(params.bookId, params.projectRoot),
    createGrepTool(params.projectRoot),
    createLsTool(params.projectRoot),
  ];

  if (params.sessionKind === "edit") {
    // Edit mode stays deterministic: forecast create runs an LLM projection,
    // and get/select belong to the planning workflow, not text editing.
    return bookTools.filter((tool) => ![
      "sub_agent",
      "generate_cover",
      "research_web",
      "import_chapters",
      "create_narrative_forecast",
      "get_narrative_forecast",
      "select_narrative_branch",
    ].includes(tool.name));
  }

  return bookTools;
}

// ---------------------------------------------------------------------------
// 工具目录投影（debug/tools 端点数据源；Rust interaction/registry.rs debug
// 投影的对偶面，555 号）——桩依赖实例化工厂提取 name/description/parameters，
// 工厂仍是元数据单一事实源。
// ---------------------------------------------------------------------------

export interface ChatToolCatalogEntry {
  readonly name: string;
  readonly description: string;
  /// 参数 schema 的规范化 sha256——先做 OpenAI 惯例归一（anyOf 枚举折叠，见
  /// normalizeJsonSchemaForHash），再 canonical JSON（对象键递归排序）后哈希，
  /// 与 Rust serde_json BTreeMap 键序同构（双端目录机械对照键）。
  readonly parametersSha256: string;
}

/// OpenAI 惯例归一（555 号首跑差分裁决）：TS TypeBox 用 `anyOf` 表达枚举/
/// 多类型 union，Rust json! 用 OpenAI 惯例 `{enum:[..]}` / `type:[..]`——
/// 生产面本就存在等价编码分歧（pi-ai 非 strict 原样透传 TypeBox；Rust
/// openai_schema 直投 json!）。哈希对照锁的是「语义等价」，故归一到 Rust
/// 原生形态再哈希。折叠规则保守：仅当 anyOf 变体键集 ⊆ {const,type}（折叠
/// 为 enum 数组，保变体序）或 ⊆ {type}（折叠为 type 数组）时折叠，其余原样
/// 透传（真联合类型的意外形态留给差分器硬拦，不静默归一）。
function normalizeJsonSchemaForHash(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(normalizeJsonSchemaForHash);
  if (!value || typeof value !== "object") return value;
  const obj = value as Record<string, unknown>;
  const anyOf = obj.anyOf;
  if (Array.isArray(anyOf) && anyOf.length > 0 && anyOf.every((v) => v && typeof v === "object")) {
    const variants = anyOf as Record<string, unknown>[];
    const keysOf = (v: Record<string, unknown>) => Object.keys(v).sort().join(",");
    const allConst = variants.every((v) => keysOf(v) === "const,type" && v.type === "string" && typeof v.const === "string");
    const allType = variants.every((v) => keysOf(v) === "type" && typeof v.type === "string");
    if (allConst || allType) {
      const folded: Record<string, unknown> = {};
      for (const [key, item] of Object.entries(obj)) {
        if (key !== "anyOf") folded[key] = normalizeJsonSchemaForHash(item);
      }
      if (allConst) {
        folded.enum = variants.map((v) => v.const);
        // TypeBox Type.Union([Type.Literal...]) 外层不带 type 键——OpenAI 惯例
        // 形态（Rust json! 原生形态）要求显式 "type":"string"，补齐到同一基线。
        if (folded.type === undefined) folded.type = "string";
      } else {
        folded.type = variants.map((v) => v.type);
      }
      return folded;
    }
  }
  const out: Record<string, unknown> = {};
  for (const [key, item] of Object.entries(obj)) {
    out[key] = normalizeJsonSchemaForHash(item);
  }
  return out;
}

/// canonical JSON：对象键按 UTF-16 码元序递归排序（schema 键全 ASCII，与
/// serde BTreeMap 的 UTF-8 字节序一致），数组保序，Symbol 键（TypeBox Kind）
/// 不参与序列化。
function canonicalJsonString(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map(canonicalJsonString).join(",")}]`;
  }
  if (value && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${canonicalJsonString(item)}`).join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

function catalogEntry(tool: AgentTool<any>): ChatToolCatalogEntry {
  return {
    name: tool.name,
    description: tool.description,
    parametersSha256: createHash("sha256").update(canonicalJsonString(normalizeJsonSchemaForHash(tool.parameters))).digest("hex"),
  };
}

// 目录实例化用桩依赖：工厂构造体只做元数据装配与闭包捕获，不消费 deps
// （消费点全在 execute 内；chat-tool-set.test.ts 锁构造期安全）。
const CATALOG_STUB_PIPELINE = {} as PipelineRunner;
const CATALOG_STUB_ROOT = ".";
const CATALOG_STUB_BOOK_ID = "catalog-book";
const CATALOG_STUB_SESSION_ID = "catalog-session";
const CATALOG_STUB_LLM = {} as FilmLLMDeps;

/**
 * node 引擎 agent 面有效工具目录（书会话有效目录框架，见模块头注）。
 *
 * 顺序 = Rust 注册表声明序（跨端 31 名，对照阅读友好）+ node-only 13 名
 * （设计内不对称，差分器豁免表放行）。变体裁决：
 * - read/ls/grep 取书作用域（Rust BookSession 遮蔽裁决同款）；
 * - read 取系统读关闭分支（目录投影环境无 INKOS_AGENT_ALLOW_SYSTEM_READ）；
 * - sub_agent 取默认变体（architectCreateOnly=false）；
 * - propose_action 取 zh 通用变体（语言只影响 execute 文案，schema 面同源）。
 */
export function buildChatToolCatalog(): ChatToolCatalogEntry[] {
  const catalog: ChatToolCatalogEntry[] = [];
  const push = (tool: AgentTool<any>) => catalog.push(catalogEntry(tool));

  // ── 跨端 31 名（Rust 注册表声明序）───────────────────────────────────────
  // film 七件（film_authoring_tools.rs defs 首族）
  for (const tool of createFilmAuthoringTools({
    projectRoot: CATALOG_STUB_ROOT,
    projectId: CATALOG_STUB_BOOK_ID,
    llm: CATALOG_STUB_LLM,
    proposeActionTool: { name: "propose_action" } as AgentTool<any>,
  })) {
    if (tool.name !== "propose_action") push(tool);
  }
  push(createProposeActionTool("zh"));
  push(createResearchWebTool(CATALOG_STUB_ROOT));
  push(createImportChaptersTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_BOOK_ID, CATALOG_STUB_ROOT));
  push(createSubAgentTool(CATALOG_STUB_PIPELINE, null, CATALOG_STUB_ROOT));
  push(createUseSkillTool({ registry: {} as SkillRegistry }));
  push(createManageBookReferenceTool(CATALOG_STUB_ROOT, CATALOG_STUB_BOOK_ID));
  push(createWriteTruthFileTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, CATALOG_STUB_BOOK_ID));
  push(createRenameEntityTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, CATALOG_STUB_BOOK_ID));
  push(createPatchChapterTextTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, CATALOG_STUB_BOOK_ID));
  push(createReplaceChapterTextTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, CATALOG_STUB_BOOK_ID));
  push(createDeleteLatestChapterTool(CATALOG_STUB_ROOT, CATALOG_STUB_BOOK_ID));
  push(createGenerateCoverTool(CATALOG_STUB_ROOT));
  push(createResyncChapterStateTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_BOOK_ID, {}));
  push(createNarrativeForecastCreateTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_BOOK_ID, CATALOG_STUB_ROOT));
  push(createNarrativeForecastGetTool(CATALOG_STUB_BOOK_ID, CATALOG_STUB_ROOT));
  push(createNarrativeForecastSelectTool(CATALOG_STUB_BOOK_ID, CATALOG_STUB_ROOT));
  push(createPlayStepTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, CATALOG_STUB_SESSION_ID, {}));
  push(createPlayReviseTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, CATALOG_STUB_SESSION_ID, {}));
  push(createPlayEditTool(CATALOG_STUB_ROOT, CATALOG_STUB_SESSION_ID, "zh"));
  push(createReadTool(CATALOG_STUB_ROOT));
  push(createLsTool(CATALOG_STUB_ROOT));
  push(createGrepTool(CATALOG_STUB_ROOT));
  push(createIngestMaterialTool(CATALOG_STUB_ROOT));
  push(createRetrieveMaterialTool(CATALOG_STUB_ROOT));

  // ── node-only 13 名（设计内不对称：Rust 侧走 propose→confirm + 端点，
  //    agent_route.rs 备案；差分器豁免表逐组放行）────────────────────────
  // 四建书件（chat 会话确认意图一次性件）
  push(createFanficBookTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT));
  push(createContinuationImportTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_BOOK_ID, CATALOG_STUB_ROOT));
  push(createSpinoffBookTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT));
  push(createImitationBookTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT));
  // 六一次性生产件（short/script/storyboard/film 会话确认意图件 + 短篇翻译）
  push(createShortFictionRunTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, {}));
  push(createTranslationCreateTool(CATALOG_STUB_ROOT));
  push(createScriptCreationTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, {}));
  push(createStoryboardCreationTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, {}));
  push(createInteractiveFilmCreationTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, {}));
  push(createPlayStartTool(CATALOG_STUB_PIPELINE, CATALOG_STUB_ROOT, CATALOG_STUB_SESSION_ID, "open"));
  // film 三一次性件（确认意图直执件）
  for (const intent of ["draft_structure", "connect_choice", "remove_node"] as const) {
    for (const tool of createFilmAuthoringTools({
      projectRoot: CATALOG_STUB_ROOT,
      projectId: CATALOG_STUB_BOOK_ID,
      llm: CATALOG_STUB_LLM,
      proposeActionTool: { name: "propose_action" } as AgentTool<any>,
      confirmedIntent: intent,
    })) {
      push(tool);
    }
  }

  return catalog;
}
