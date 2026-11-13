import type { AgentTool } from "@mariozechner/pi-agent-core";
import { Type, type TSchema } from "@sinclair/typebox";
import type { WorkManifest } from "../contracts.js";
import { loadWorkManifest } from "../work-store.js";
import type { AnyAgentTool } from "../../agent/pi-compat.js";

/** Creation owns Work identity and its registered source; production fills that Work. */
export function bindProductionTool(root: string, work: WorkManifest, tool: AnyAgentTool): AnyAgentTool {
  const original = tool.parameters as { properties: Record<string, TSchema> };
  const hasBoundSource = work.artifacts.some(artifact => artifact.revisions.some(revision =>
    revision.id === artifact.currentRevisionId && revision.path === "source/source-material.md"));
  const owned = new Set(["title", "projectId", ...(hasBoundSource ? ["sourceText", "sourcePath"] : [])]);
  const properties = Object.fromEntries(Object.entries(original.properties).filter(([key]) => !owned.has(key)));
  if (typeof work.metadata.intent === "string" && properties.instruction) {
    properties.instruction = Type.Optional(properties.instruction);
  }
  return {
    ...tool,
    name: "generate",
    label: "Generate current work",
    description: `Generate the first production in the bound ${work.profileId} Work ${work.id}. Its identity and title are fixed.${hasBoundSource ? " The host supplies the registered source version; no source text or path is needed." : ""} To create or derive another Work, use workspace__create_work with its intended Profile first. Omitted instruction uses this Work's saved creative brief.`,
    parameters: Type.Object(properties, { additionalProperties: false }),
    // 1018 号：mz 0.73.1 下 AgentTool<any,any> 的 execute params 推断 unknown
    // （逆变位），显式 Record 化配合 in 检查与透传。
    async execute(id, params: Record<string, unknown>, signal, onUpdate) {
      if ("projectId" in params || "title" in params) {
        throw Object.assign(new Error("Production cannot create or retarget a Work."), {
          code: "WORK_TARGET_BOUND", recovery: { action: "workspace__create_work", reason: "Create the requested Work and Profile before generating its content." },
        });
      }
      if (hasBoundSource && ("sourceText" in params || "sourcePath" in params)) {
        throw Object.assign(new Error("Production uses this Work's registered source. Create a new derived Work to choose a different source."), {code:"WORK_SOURCE_BOUND"});
      }
      const current = await loadWorkManifest(root, work.id);
      return tool.execute(id, { ...params, title: current.title, projectId: current.id,
        instruction: params.instruction ?? current.metadata.intent }, signal, onUpdate);
    },
  };
}
