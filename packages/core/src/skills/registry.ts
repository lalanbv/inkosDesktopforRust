import type {
  AgentSkill,
  SkillRegistry,
  SkillResolutionInput,
  SkillResolutionResult,
} from "./types.js";

export interface CreateSkillRegistryOptions {
  readonly skills?: ReadonlyArray<AgentSkill>;
}

export function createSkillRegistry(options: CreateSkillRegistryOptions = {}): SkillRegistry {
  const skills = dedupeSkills(options.skills ?? []);
  const byId = new Map(skills.map((skill) => [skill.id, skill]));

  return {
    listSkills() {
      return skills;
    },
    getSkill(id: string) {
      return byId.get(normalizeSkillId(id));
    },
    resolveSkills(input: SkillResolutionInput) {
      // R37 治理面：disable-model-invocation 技能模型不可激活——并入拦截
      // 集（requested 强制也跳过）但**不进 disabledSkillIds 输出**（那是
      // 「显式禁用清单」语义；governance 合并在 disabledSkillIds 计算之后）。
      // listSkills/getSkill 保留（人侧/审计可见）。
      const normalizedInputDisabled = normalizeIdList(input.disabledSkills);
      const disabled = new Set(normalizedInputDisabled);
      const requested = normalizeIdList(input.requestedSkills);
      const missingSkillIds: string[] = [];
      const disabledSkillIds = normalizedInputDisabled.filter((id) => byId.has(id));
      for (const skill of skills) {
        if (skill.disableModelInvocation === true) disabled.add(skill.id);
      }
      const used = new Map<string, AgentSkill>();
      const forcedSkillIds: string[] = [];

      for (const id of requested) {
        const skill = byId.get(id);
        if (!skill) {
          missingSkillIds.push(id);
          continue;
        }
        if (disabled.has(id)) continue;
        used.set(id, skill);
        forcedSkillIds.push(id);
      }

      const availableSkills = skills.filter((skill) => !disabled.has(skill.id));

      return {
        usedSkills: [...used.values()],
        forcedSkillIds,
        missingSkillIds: dedupeStrings(missingSkillIds),
        disabledSkillIds,
        availableSkills,
        availableSkillIds: availableSkills.map((skill) => skill.id),
      } satisfies SkillResolutionResult;
    },
  };
}

function dedupeSkills(skills: ReadonlyArray<AgentSkill>): AgentSkill[] {
  // R44/566 号 rank 显式表：同 id 决胜=有效 rank 升序（undefined 视为
  // MAX_SAFE_INTEGER，手工构造注册表退化为纯后写胜=R44 前行为），平秩
  // 保持装载序后写胜。装载层缺省 rank 已由 external-loader 注入（六级表
  // project skills 100 → builtin 600），此处的 undefined 分支仅为防御。
  const byId = new Map<string, AgentSkill>();
  for (const skill of skills) {
    const id = normalizeSkillId(skill.id);
    const incumbent = byId.get(id);
    if (!incumbent || effectiveRank(skill) <= effectiveRank(incumbent)) {
      byId.set(id, { ...skill, id });
    }
  }
  return [...byId.values()].sort((left, right) => left.id.localeCompare(right.id));
}

function effectiveRank(skill: AgentSkill): number {
  return skill.rank ?? Number.MAX_SAFE_INTEGER;
}

function normalizeIdList(values: ReadonlyArray<string> | undefined): string[] {
  return dedupeStrings((values ?? []).map(normalizeSkillId).filter(Boolean));
}

function normalizeSkillId(value: string): string {
  return value.trim().toLowerCase();
}

function dedupeStrings(values: ReadonlyArray<string>): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const value of values) {
    if (!value || seen.has(value)) continue;
    seen.add(value);
    out.push(value);
  }
  return out;
}
