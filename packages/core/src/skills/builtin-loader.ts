import { fileURLToPath } from "node:url";
import {
  loadConfiguredAgentSkills,
  loadExternalAgentSkills,
  type ExternalSkillDiagnostic,
  type LoadConfiguredAgentSkillsInput,
} from "./external-loader.js";
import type { AgentSkill } from "./types.js";

export interface LoadAvailableAgentSkillsResult {
  readonly skills: ReadonlyArray<AgentSkill>;
  readonly diagnostics: ReadonlyArray<ExternalSkillDiagnostic>;
}

export async function loadBuiltinAgentSkills(
  builtinRoot = builtinSkillsRoot(),
): Promise<LoadAvailableAgentSkillsResult> {
  return loadExternalAgentSkills({
    externalDirs: [builtinRoot],
    source: "builtin",
    // R44/566 号：内置层为六级缺省 rank 表的最末层（600，低者胜——配置层
    // 同名技能可覆盖内置默认，130 号语义的 rank 显式化形态）。
    defaultRank: 600,
  });
}

export async function loadAvailableAgentSkills(
  input: LoadConfiguredAgentSkillsInput,
): Promise<LoadAvailableAgentSkillsResult> {
  const [builtin, configured] = await Promise.all([
    loadBuiltinAgentSkills(),
    loadConfiguredAgentSkills(input),
  ]);
  return {
    // Registry de-duplication is last-write-wins, so project/user skills can
    // intentionally replace an InkOS default with the same AgentSkills id.
    skills: [...builtin.skills, ...configured.skills],
    diagnostics: [...builtin.diagnostics, ...configured.diagnostics],
  };
}

function builtinSkillsRoot(): string {
  return fileURLToPath(new URL("../../skills", import.meta.url));
}
