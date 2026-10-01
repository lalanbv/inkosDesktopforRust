import { z } from "zod";

export const AgentSkillSchema = z.object({
  id: z.string().min(1).max(64),
  name: z.string().min(1).max(64),
  description: z.string().min(1).max(1024),
  body: z.string().default(""),
  source: z.enum(["builtin", "project", "user", "external"]).default("external"),
  baseDir: z.string().min(1).optional(),
  /**
   * R37 治理面（上游 pi 同名字段 `disable-model-invocation` 的映射，零发明）：
   * true = 人侧保留（listSkills/getSkill 可见）但模型不可激活
   * （resolveSkills 视为 disabled）。frontmatter kebab-case 由
   * external-loader 显式映射到本字段。
   */
  disableModelInvocation: z.boolean().optional(),
  /**
   * R44/566 号 invocation 双布尔第二位（dsh 同名语义 `user-invocable`，缺省
   * true）：false = 用户面不可见（GET /api/v1/skills 过滤），模型面不受
   * 影响。与 disable-model-invocation 正交，四组合保留。
   */
  userInvocable: z.boolean().optional(),
  /**
   * R44/566 号 rank 显式表：同 id 决胜=有效 rank 升序、平秩后写胜。装载层
   * 缺省表（project skills 100 → project .agents 200 → ~/.agents 300 →
   * ~/.openclaw 400 → env 500 → builtin 600，低者胜）由 loader 注入；
   * frontmatter `rank` 显式覆盖可跨层升降。手工构造（缺省 undefined）退化为
   * 纯后写胜（R44 前行为）。
   */
  rank: z.number().int().min(0).max(1000).optional(),
}).strict();
export type AgentSkill = z.infer<typeof AgentSkillSchema>;

export interface SkillResolutionInput {
  readonly requestedSkills?: ReadonlyArray<string>;
  readonly disabledSkills?: ReadonlyArray<string>;
}

export interface SkillResolutionResult {
  readonly usedSkills: ReadonlyArray<AgentSkill>;
  readonly forcedSkillIds: ReadonlyArray<string>;
  readonly missingSkillIds: ReadonlyArray<string>;
  readonly disabledSkillIds: ReadonlyArray<string>;
  readonly availableSkills: ReadonlyArray<AgentSkill>;
  readonly availableSkillIds: ReadonlyArray<string>;
}

export interface SkillRegistry {
  listSkills(): ReadonlyArray<AgentSkill>;
  getSkill(id: string): AgentSkill | undefined;
  resolveSkills(input: SkillResolutionInput): SkillResolutionResult;
}
