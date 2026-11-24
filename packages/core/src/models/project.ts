import { z } from "zod";
import { LLM_API_FORMATS } from "../llm/api-format.js";

export const LLMServiceEntrySchema = z.object({
  service: z.string().min(1),
  name: z.string().min(1).optional(),
  baseUrl: z.string().url().optional(),
  models: z.array(z.string().min(1)).optional(),
  temperature: z.number().min(0).max(2).optional(),
  // 1666 号：以下四键对齐 Rust `ServiceConfigEntry::to_json`（PUT services 的
  // 持久化形态）——此前 strict schema 不认它们：预算键一旦真的落进 inkos.json
  // （1661 加了 Rust 结构体与 studio 归一化，漏了这里；1666 修好「PUT 存得下
  // 预算」后在活体走查中暴露），Node 回退腿加载项目配置即抛
  // `Unrecognized key(s)`，/agent 对话整个不可用。`maxTokens` 是更早的同族
  // 漂移（Rust 腿保存过带 maxTokens 的条目后，Node 腿读同一份配置即崩）。
  // 数值一律宽松（Rust 为 f64 / u64，0 与负数由归一层判「未设」）：schema 层
  // 拒绝合法落盘值=又一次「读不了配置」。
  maxTokens: z.number().optional(),
  dailyTokenBudget: z.number().optional(),
  monthlyTokenBudget: z.number().optional(),
  confirmAtBudget: z.boolean().optional(),
  apiFormat: z.enum(LLM_API_FORMATS).optional(),
  stream: z.boolean().optional(),
}).strict();

const LLMCoverConfigSchema = z.object({
  service: z.enum(["kkaiapi", "openai", "google"]),
  model: z.string().min(1),
  baseUrl: z.string().url().optional(),
}).strict().optional();

export const LLMConfigSchema = z.object({
  provider: z.enum(["anthropic", "openai", "custom"]),
  service: z.string().default("custom"),
  configSource: z.enum(["env", "studio"]).default("env"),
  baseUrl: z.string().url(),
  apiKey: z.string().default(""),
  model: z.string().min(1),
  proxyUrl: z.string().url().optional(),
  temperature: z.number().min(0).max(2).optional(),
  thinkingBudget: z.number().int().min(0).default(0),
  extra: z.record(z.unknown()).optional(),
  headers: z.record(z.string()).optional(),
  apiFormat: z.enum(LLM_API_FORMATS).default("chat"),
  stream: z.boolean().default(true),
  services: z.array(LLMServiceEntrySchema).optional(),
  defaultModel: z.string().min(1).optional(),
  // 1604 号：.nullable() 补齐——Rust LLMConfig.cover 是 Option 无
  // skip_serializing_if（models/project.rs），struct 序列化恒产出 "cover":null
  //（keyset fixture 活体）；zod .optional() 只容 undefined 不容 null。
  cover: LLMCoverConfigSchema.nullable(),
}).strict();

export type LLMConfig = z.infer<typeof LLMConfigSchema>;

export const NotifyChannelSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("telegram"),
    botToken: z.string().min(1),
    chatId: z.string().min(1),
    format: z.enum(["markdown", "text"]).default("markdown"),
  }).strict(),
  z.object({
    type: z.literal("wechat-work"),
    webhookUrl: z.string().url(),
    format: z.enum(["markdown", "text"]).default("markdown"),
  }).strict(),
  z.object({
    type: z.literal("feishu"),
    webhookUrl: z.string().url(),
    format: z.enum(["markdown", "text"]).default("markdown"),
  }).strict(),
  z.object({
    type: z.literal("webhook"),
    url: z.string().url(),
    secret: z.string().optional(),
    events: z.array(z.string()).default([]),
    format: z.enum(["markdown", "text"]).default("markdown"),
  }).strict(),
]);

export type NotifyChannel = z.infer<typeof NotifyChannelSchema>;

export const DetectionConfigSchema = z.object({
  provider: z.enum(["gptzero", "originality", "custom"]).default("custom"),
  apiUrl: z.string().url(),
  apiKeyEnv: z.string().min(1),
  enabled: z.boolean().default(false),
  // 1602 号键面差集补齐（Rust DetectionConfig 镜像，models/project.rs:211——
  // serde default 同水位全可选；此前 strict 四键拒 Rust 侧合法形态，
  // project/detection PUT safeParse 同拒=用户无法保存）。
  threshold: z.number().optional(),
  autoRewrite: z.boolean().optional(),
  maxRetries: z.number().int().min(0).optional(),
  /** G4/340 号：参考作品专名表——仿写正文泄露检测。 */
  protectedNames: z.array(z.string()).optional(),
}).strict();

export type DetectionConfig = z.infer<typeof DetectionConfigSchema>;

export const AgentLLMOverrideSchema = z.object({
  model: z.string().min(1),
  provider: z.enum(["anthropic", "openai", "custom"]).optional(),
  baseUrl: z.string().url().optional(),
  apiKeyEnv: z.string().optional(),
  stream: z.boolean().optional(),
}).strict();

export type AgentLLMOverride = z.infer<typeof AgentLLMOverrideSchema>;

const ModelOverrideValueSchema = z.union([z.string(), AgentLLMOverrideSchema]);

export const ResearchSearchConfigSchema = z.object({
  enabled: z.boolean().default(false),
  provider: z.enum(["tavily", "custom"]).default("tavily"),
  baseUrl: z.string().url().optional(),
  apiKey: z.string().optional(),
  apiKeyEnv: z.string().optional(),
  // 635/684 号：SSRF 私网豁免的用户开关（Rust parse_research_search 镜像——
  // 恒投影 default false；缺键则 GET 形状漂移且 PUT 往返静默清除用户显式配置）。
  allowPrivateEgress: z.boolean().default(false),
}).strict().default({
  enabled: false,
  provider: "tavily",
  allowPrivateEgress: false,
});

export type ResearchSearchConfig = z.infer<typeof ResearchSearchConfigSchema>;

export const ProjectConfigSchema = z.object({
  name: z.string().min(1),
  version: z.literal("0.1.0"),
  language: z.enum(["zh", "en"]).default("zh"),
  llm: LLMConfigSchema,
  notify: z.array(NotifyChannelSchema).default([]),
  detection: DetectionConfigSchema.optional(),
  researchSearch: ResearchSearchConfigSchema,
  modelOverrides: z.record(z.string(), ModelOverrideValueSchema).optional(),
  daemon: z.object({
    schedule: z.object({
      radarCron: z.string().default("0 */6 * * *"),
      writeCron: z.string().default("*/15 * * * *"),
    }).strict(),
    maxConcurrentBooks: z.number().int().min(1).default(3),
    chaptersPerCycle: z.number().int().min(1).default(1),
    retryDelayMs: z.number().int().min(0).default(30_000),
    cooldownAfterChapterMs: z.number().int().min(0).default(10_000),
    maxChaptersPerDay: z.number().int().min(1).default(50),
    // 1602 号键面差集补齐：Rust DaemonConfig 必序列化 quality_gates（无
    // skip_serializing_if，models/project.rs:408；QualityGates={maxAuditRetries:2,
    // pauseAfterConsecutiveFailures:3, retryTemperatureStep:0.1} 与 540 号手动
    // Default 同源）——strict 六键拒含 qualityGates 的 daemon 段。子键可选=
    // serde default 同水位；689 教训：strict().default 字面量同步补全键，
    // 投影形状与 Rust DaemonConfig::default 序列化对齐。
    qualityGates: z.object({
      maxAuditRetries: z.number().int().min(0).optional(),
      pauseAfterConsecutiveFailures: z.number().int().min(0).optional(),
      retryTemperatureStep: z.number().optional(),
    }).optional(),
  }).strict().default({
    schedule: {
      radarCron: "0 */6 * * *",
      writeCron: "*/15 * * * *",
    },
    maxConcurrentBooks: 3,
    chaptersPerCycle: 1,
    retryDelayMs: 30_000,
    cooldownAfterChapterMs: 10_000,
    maxChaptersPerDay: 50,
    qualityGates: {
      maxAuditRetries: 2,
      pauseAfterConsecutiveFailures: 3,
      retryTemperatureStep: 0.1,
    },
  }),
  // ── 709 号：writing 段复位（Rust WritingConfig 镜像，models/project.rs）──
  // 2.0 strict 化时裁掉的 fork 段。Rust 引擎把 reviewMode/revisionGate/
  // reviewRetries 写进 inkos.json（project chapter-review-mode PUT 等），
  // strict 根拒收 = TS 回退腿对 Rust 触碰过的项目 loadProjectConfig 即崩
  //（706 号 walkthrough-env boot 崩同型）——跨引擎配置投毒的收敛修复。
  // 子键全部可选：Rust 侧 serde default（retries=1/mode=auto/gate=strict）。
  writing: z.object({
    /** R14/686 号：审改循环重试上限旋钮（rcm 矩阵腿）。 */
    reviewRetries: z.number().int().min(0).optional(),
    reviewMode: z.enum(["auto", "manual"]).optional(),
    revisionGate: z.enum(["strict", "lenient", "always"]).optional(),
  }).optional(),
  // ── 1429 号：inputGovernanceMode 遗留键容忍（686 号删管线遗留，709 同族）──
  // 686 号随 TS 上游 e7c04465 删除 legacy 输入治理管线，Rust 侧
  //（engine-rs project_config_routes.rs 头注）对齐备案「inkos.json 遗留
  // inputGovernanceMode 键双端同为未知键忽略」——但 TS strict 根实际拒收
  //（ZodError unrecognized_keys 裸抛未经人话包装），与该契约注释漂移：
  // 8/6 起仓根实采 inkos.json 即带此键（值 "v2"），TS dev:server 与 Node
  // 回退腿 loadProjectConfig 必挂、Rust 引擎同文件 200（活体 1429 实证）。
  // 修法沿 709 先例=schema 单源容键（effective-llm-config 与
  // short-fiction-runner 两 parse 点同获覆盖），不动 strict 根设计；
  // z.unknown().optional() 不冻结已死管线的历史值域（Rust serde 未知键
  // 忽略同语义）。写回面直读 raw JSON 不经 schema，零写面副作用。
  inputGovernanceMode: z.unknown().optional(),
  // ── 1602 号：foundation 段复位（Rust FoundationConfig 镜像，models/project.rs:
  // 292={reviewRetries(serde default 2)}）──Rust ProjectConfig 有此字段且无
  // skip_serializing_if（凡 serde struct 序列化路径必带）；legacy/上游 1.x 项目
  // inkos.json 带此键时 TS strict 根 unrecognized key 拒（boot 崩同 706/1429 型，
  // Rust 同文件绿）。子键可选=serde default 同水位（709 writing 先例）。
  foundation: z.object({
    reviewRetries: z.number().int().min(0).optional(),
  }).optional(),
}).strict();

export type ProjectConfig = z.infer<typeof ProjectConfigSchema>;
