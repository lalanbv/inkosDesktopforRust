import { z } from "zod";

/**
 * 按任务模型路由（G16/345 号，Phase B 批次三末项；规划 G16 采纳）。
 *
 * 既有解析链（effective-llm-config）产出**全局单一模型**——写作要长文强模型、
 * 检测/审计要快而便宜的模型，一个全局模型要么贵要么弱。本模块在既有链之上
 * 叠加**任务维度**的路由契约：

 *   五类任务：writing / review / repair / detect / analysis
 *   逐字段回退：task 覆盖 → defaults → fallback（现有全局解析产物）
 *   级别覆盖：book 级逐字段覆盖 project 级
 *
 * **配置迁移兼容**：旧 inkos.json / book.json 无 routing 字段 → 全字段回退
 * fallback，现有行为逐字节不变（迁移测锁定）。

 * 双端：`engine-rs/src/models/task_routing.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/task-routing-vectors.json`（解析一致 duel）。
 */

export const TASK_MODEL_KINDS = ["writing", "review", "repair", "detect", "analysis"] as const;
export type TaskModelKind = (typeof TASK_MODEL_KINDS)[number];

export const TaskModelKindSchema = z.enum(TASK_MODEL_KINDS);

/** 单任务/缺省覆盖：全部字段可选（部分覆盖，未给字段逐字段回退）。 */
export const TaskModelOverrideSchema = z
  .object({
    model: z.string().min(1).optional(),
    service: z.string().min(1).optional(),
    temperature: z.number().optional(),
    maxTokens: z.number().int().positive().optional(),
  })
  .strict();

export type TaskModelOverride = z.infer<typeof TaskModelOverrideSchema>;

/** 一级路由配置（project 与 book 同构）。 */
export const TaskModelRoutingSchema = z
  .object({
    defaults: TaskModelOverrideSchema.optional(),
    tasks: z.record(TaskModelKindSchema, TaskModelOverrideSchema.optional()).optional(),
  })
  .strict();

export type TaskModelRouting = z.infer<typeof TaskModelRoutingSchema>;

export interface ResolvedTaskModel {
  readonly task: TaskModelKind;
  readonly model: string;
  readonly service?: string;
  readonly temperature?: number;
  readonly maxTokens?: number;
  /** 每个字段实际生效来源（task/defaults/fallback），排障与设置面展示用。 */
  readonly sources: ReadonlyArray<{ readonly field: string; readonly source: "task" | "defaults" | "fallback" }>;
}

type PartialOverride = Partial<TaskModelOverride>;

function overrideFields(override: PartialOverride | undefined): PartialOverride {
  if (!override) return {};
  const out: PartialOverride = {};
  if (typeof override.model === "string" && override.model) out.model = override.model;
  if (typeof override.service === "string" && override.service) out.service = override.service;
  if (typeof override.temperature === "number" && Number.isFinite(override.temperature)) {
    out.temperature = override.temperature;
  }
  if (typeof override.maxTokens === "number" && Number.isInteger(override.maxTokens) && override.maxTokens > 0) {
    out.maxTokens = override.maxTokens;
  }
  return out;
}

/**
 * 级别合并：book 逐字段覆盖 project（book.tasks[t] > book.defaults >
 * project.tasks[t] > project.defaults）。两侧都缺 → undefined（全 fallback）。
 */
export function mergeTaskRouting(
  book?: TaskModelRouting,
  project?: TaskModelRouting,
): TaskModelRouting | undefined {
  if (!book && !project) return undefined;
  const merged: { defaults?: PartialOverride; tasks: Record<string, PartialOverride> } = {
    tasks: {},
  };
  const defaults: PartialOverride = {};
  for (const layer of [project, book] as const) {
    const routing = layer as TaskModelRouting | undefined;
    if (!routing) continue;
    Object.assign(defaults, overrideFields(routing.defaults));
    for (const [kind, override] of Object.entries(routing.tasks ?? {})) {
      merged.tasks[kind] = { ...overrideFields(override), ...overrideFields(merged.tasks[kind]) };
    }
  }
  merged.defaults = defaults;
  // book 的 task 覆盖之上还应叠加 book.defaults 未被 task 覆盖的字段：
  // compose 顺序 = fallback ← project.defaults ← project.tasks[t] ← book.defaults ← book.tasks[t]
  if (book?.defaults) {
    const bookDefaults = overrideFields(book.defaults);
    for (const [kind, override] of Object.entries(merged.tasks)) {
      merged.tasks[kind] = { ...bookDefaults, ...overrideFields(override) };
    }
  }
  if (project?.defaults) {
    const projectDefaults = overrideFields(project.defaults);
    for (const [kind, override] of Object.entries(merged.tasks)) {
      merged.tasks[kind] = { ...projectDefaults, ...overrideFields(override) };
    }
  }
  return { defaults: overrideFields(merged.defaults), tasks: merged.tasks } as TaskModelRouting;
}

/**
 * agent 名 → 任务类型映射（管线接线口径）：resolveOverride 前置查此表，
 * 命中且路由配置含该任务覆盖 → 以路由 model 替换全局 model。
 * detect 走外部检测服务（无 LLM model），不在映射内。
 */
export const TASK_AGENT_MAP: Readonly<Record<string, TaskModelKind>> = {
  writer: "writing",
  planner: "writing",
  architect: "writing",
  "continuity-auditor": "review",
  reviser: "repair",
  consolidator: "review",
  "state-validator": "review",
  radar: "analysis",
  "chapter-analyzer": "analysis",
};

type RoutingField = "model" | "service" | "temperature" | "maxTokens";
const ROUTING_FIELDS: ReadonlyArray<RoutingField> = ["model", "service", "temperature", "maxTokens"];

/** 组合单任务的逐字段值与来源（级别顺序：project.defaults → project.tasks[t] → book.defaults → book.tasks[t]，后者覆盖前者）。 */
function composeTaskOverride(
  book: TaskModelRouting | undefined,
  project: TaskModelRouting | undefined,
  task: TaskModelKind,
): { readonly override: PartialOverride; readonly perField: Record<RoutingField, "task" | "defaults" | "fallback"> } {
  const projectDefaults = overrideFields(project?.defaults);
  const projectTask = overrideFields(project?.tasks?.[task]);
  const bookDefaults = overrideFields(book?.defaults);
  const bookTask = overrideFields(book?.tasks?.[task]);
  const override: PartialOverride = {
    ...projectDefaults,
    ...projectTask,
    ...bookDefaults,
    ...bookTask,
  };
  const perField: Record<RoutingField, "task" | "defaults" | "fallback"> = {
    model: "fallback",
    service: "fallback",
    temperature: "fallback",
    maxTokens: "fallback",
  };
  for (const field of ROUTING_FIELDS) {
    if (bookTask[field] !== undefined) perField[field] = "task";
    else if (bookDefaults[field] !== undefined) perField[field] = "defaults";
    else if (projectTask[field] !== undefined) perField[field] = "task";
    else if (projectDefaults[field] !== undefined) perField[field] = "defaults";
    else perField[field] = "fallback";
  }
  return { override, perField };
}

/**
 * 任务模型解析（逐字段三层回退）：
 *   book.tasks[task] > book.defaults > project.tasks[task] > project.defaults
 *     > fallback（现有全局解析产物，model 必填兜底）。
 * 迁移兼容：routing 缺省时全字段 = fallback。
 */
export function resolveTaskModel(params: {
  readonly task: TaskModelKind;
  readonly bookRouting?: TaskModelRouting;
  readonly projectRouting?: TaskModelRouting;
  readonly fallbackModel: string;
  readonly fallbackService?: string;
  readonly fallbackTemperature?: number;
  readonly fallbackMaxTokens?: number;
}): ResolvedTaskModel {
  const { override, perField } = composeTaskOverride(
    params.bookRouting,
    params.projectRouting,
    params.task,
  );
  const fallback: PartialOverride = {
    model: params.fallbackModel,
    ...(params.fallbackService ? { service: params.fallbackService } : {}),
    ...(typeof params.fallbackTemperature === "number"
      ? { temperature: params.fallbackTemperature }
      : {}),
    ...(typeof params.fallbackMaxTokens === "number" ? { maxTokens: params.fallbackMaxTokens } : {}),
  };
  const effective = { ...fallback, ...override };
  const sources = ROUTING_FIELDS.map((field) => ({
    field,
    source: effective[field] !== undefined ? perField[field] : ("fallback" as const),
  }));
  return {
    task: params.task,
    model: effective.model ?? params.fallbackModel,
    ...(effective.service ? { service: effective.service } : {}),
    ...(effective.temperature !== undefined ? { temperature: effective.temperature } : {}),
    ...(effective.maxTokens !== undefined ? { maxTokens: effective.maxTokens } : {}),
    sources,
  };
}

/** agent 级便捷解析：命中映射且路由给出 model 覆盖时返回新 model，否则 undefined。 */
export function resolveAgentModel(params: {
  readonly agent: string;
  readonly routing?: TaskModelRouting;
  readonly fallbackModel: string;
}): string | undefined {
  const task = TASK_AGENT_MAP[params.agent];
  if (!task || !params.routing) return undefined;
  const resolved = resolveTaskModel({
    task,
    projectRouting: params.routing,
    fallbackModel: params.fallbackModel,
  });
  return resolved.model !== params.fallbackModel ? resolved.model : undefined;
}
