import type { ActivatedSkillGuidance } from "../agent/skill-tool.js";
import type { AgentSkill } from "./types.js";
import bindings from "./production-skill-bindings.json" with { type: "json" };

// R44/566 号：生产能力键族显式化（原从 PRODUCTION_SKILL_IDS 推导，绑定表
// 迁 JSON 单源后改为与 production-skill-bindings.json capabilities 键族
// 逐一对应；Rust 侧 ProductionSkillCapability 枚举同族）。
export type ProductionSkillCapability =
  | "longWriting"
  | "longReview"
  | "shortWriting"
  | "play"
  | "script"
  | "storyboard"
  | "interactiveFilm"
  | "translation";

// R44/566 号：绑定表单源=production-skill-bindings.json（resolveJsonModule
// 编译期内嵌；Rust 侧 include_str! 同文件——双端零漂移）。
const PRODUCTION_SKILL_BINDINGS = bindings.capabilities as Record<
  ProductionSkillCapability,
  ReadonlyArray<string>
>;

export const PRODUCTION_SKILL_IDS: Readonly<
  Record<ProductionSkillCapability, ReadonlyArray<string>>
> = PRODUCTION_SKILL_BINDINGS;

export const NON_LONG_PRODUCTION_CAPABILITIES = [
  "shortWriting",
  "play",
  "script",
  "storyboard",
  "interactiveFilm",
  "translation",
] as const satisfies ReadonlyArray<ProductionSkillCapability>;

export function resolveProductionSkillActivations(
  availableSkills: ReadonlyArray<AgentSkill>,
  capability: ProductionSkillCapability,
): ActivatedSkillGuidance[] {
  const byId = new Map(availableSkills.map((skill) => [skill.id, skill]));
  return PRODUCTION_SKILL_IDS[capability].flatMap((id) => {
    const skill = byId.get(id);
    return skill ? [{ skill, resources: [] }] : [];
  });
}

export function mergeActivatedSkillGuidance(
  ...groups: ReadonlyArray<ReadonlyArray<ActivatedSkillGuidance>>
): ActivatedSkillGuidance[] {
  const merged = new Map<string, ActivatedSkillGuidance>();
  for (const group of groups) {
    for (const activation of group) merged.set(activation.skill.id, activation);
  }
  return [...merged.values()];
}

export function activatedSkillIds(
  activations: ReadonlyArray<ActivatedSkillGuidance>,
): string[] {
  return activations.map((activation) => activation.skill.id);
}
