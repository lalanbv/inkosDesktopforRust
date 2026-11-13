import { Type, type Static } from "@sinclair/typebox";
import type { AgentTool } from "@mariozechner/pi-agent-core";
import { createBuiltInWorkProfileRegistry } from "../builtin-profiles.js";
import { createInitialWorkManifestWrite } from "../source-sync.js";
import { loadWorkManifest } from "../work-store.js";
import { commitAtomicFileSet } from "../../utils/atomic-file-set.js";
import { StateManager } from "../../state/manager.js";
import { withWorkMutationScope } from "../../utils/work-mutation-scope.js";
import type { CapabilityRegistry } from "../capability-registry.js";
import { CreationSourceReference, CreationSourceReferences, loadCreationSource } from "../../agent/creation-source.js";
import type { AtomicFileWrite } from "../../utils/atomic-file-set.js";
const CreateParameters = Type.Object({ workId: Type.String(), profileId: Type.String(), title: Type.String({ minLength: 1 }), intent: Type.String({ minLength: 1 }), language: Type.Optional(Type.String()),
  source: Type.Optional(CreationSourceReference),
  sources: Type.Optional(CreationSourceReferences),
});
export function createProfileWorkTools(root: string, capabilities: CapabilityRegistry): AgentTool<any>[] {
  return [
    { name: "list_work_profiles", label: "List creative profiles", description: "List installed creative profiles and the methods and actions they compose.", parameters: Type.Object({}),
      execute: async () => ({ content: [{ type: "text", text: JSON.stringify(createBuiltInWorkProfileRegistry(root).list()) }], details: { kind: "work_profiles" } }),
    },
    { name: "create_work", label: "Create work from profile", description: "Create and bind a Work using an installed Profile, persist the user's brief, and activate the Profile's production actions for the next step. For derivation, supply source or ordered sources covering all author-requested material. inspect_work returns sourceSets for complete manuscripts; use their source references when the author requests written content, rather than a brief or plan.", parameters: CreateParameters,
      // 1017/1018 号：mz 0.73.1 收紧 AgentTool.execute params 到 Static<TParameters>——
      // AgentTool<any,any> 归档面下推断回落 unknown（逆变位不可窄化注解），
      // 体内一次 cast 具体化。
      execute: async (_id, params) => {
        const p = params as Static<typeof CreateParameters>;
        const profile = createBuiltInWorkProfileRegistry(root).require(p.profileId);
        capabilities.forProfile(profile);
        return withWorkMutationScope(root, p.workId, () => new StateManager(root).acquireBookLock(p.workId), async () => {
          try { await loadWorkManifest(root, p.workId); throw Object.assign(new Error("Work already exists"), { code: "WORK_ALREADY_EXISTS" }); }
          catch (error) { if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error; }
          const writes: AtomicFileWrite[] = [{ relativePath: `works/${p.workId}/source/brief.md`, content: p.intent }];
          const lineage = [];
          if (p.source || p.sources) {
            const source = await loadCreationSource({projectRoot:root,source:p.source,sources:p.sources,purpose:'reference'});
            writes.push({relativePath:`works/${p.workId}/source/source-material.md`,content:source.text});
            lineage.push(...source.lineage);
          }
          const initial = createInitialWorkManifestWrite({ workId: p.workId, title: p.title, profileId: profile.id, language: p.language ?? "zh", writes, metadata: { intent: p.intent }, lineage });
          await commitAtomicFileSet({ rootDir: root, writes: [...writes, initial.write] });
          return { content: [{ type: "text", text: `Created ${p.title} with profile ${profile.id}.` }], details: { kind: "work_created", workId: p.workId, profileId: profile.id, lineage } };
        });
      },
    },
  ];
}
