import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  isFactActiveAt,
  type CurrentStateFact,
  type RuntimeStateDelta,
  type StateManifest,
} from "../models/runtime-state.js";
import { applyRuntimeStateDelta, type RuntimeStateSnapshot } from "../state/state-reducer.js";

/**
 * R65 第一步：快照事实有效期语义共享 golden（双腿同一事实源）。
 *
 * Rust 差分：`engine-rs/tests/golden_state_fact_validity_diff.rs`
 * （以 `CurrentStatePatch` 驱动）；本文件以 `factOps.upsert` 驱动
 * `applyRuntimeStateDelta`——两驱动同语义。activeAt 表锁
 * `isFactActiveAt` 边界（失效章等于当前章仍有效）。
 */

const here = dirname(fileURLToPath(import.meta.url));

interface ActiveAtVector {
  readonly name: string;
  readonly validUntil: number | null;
  readonly queryChapter: number;
  readonly active: boolean;
}

interface FactVector {
  readonly subject: string;
  readonly predicate: string;
  readonly object: string;
  readonly validFromChapter: number;
  readonly validUntilChapter: number | null;
  readonly sourceChapter: number;
}

interface SupersedeCase {
  readonly name: string;
  readonly language: "zh" | "en";
  readonly chapter: number;
  readonly inputFacts: readonly FactVector[];
  readonly tsUpsert: { readonly subject: string; readonly predicate: string; readonly object: string };
  readonly expectedFacts: readonly FactVector[];
}

interface Vectors {
  readonly activeAt: readonly ActiveAtVector[];
  readonly supersede: readonly SupersedeCase[];
}

const load = async (): Promise<Vectors> =>
  JSON.parse(await readFile(join(here, "golden", "state-fact-validity.json"), "utf-8")) as Vectors;

const canonicalSort = (facts: readonly CurrentStateFact[]): CurrentStateFact[] =>
  [...facts].sort((left, right) => (
    left.predicate < right.predicate ? -1
      : left.predicate > right.predicate ? 1
        : left.object < right.object ? -1
          : left.object > right.object ? 1
            : 0
  ));

describe("state fact validity shared golden", () => {
  it("activeAt boundary table matches isFactActiveAt", async () => {
    const vectors = await load();
    for (const vector of vectors.activeAt) {
      expect(isFactActiveAt({ validUntilChapter: vector.validUntil }, vector.queryChapter))
        .toBe(vector.active);
    }
  });

  it("supersede via factOps.upsert keeps history", async () => {
    const vectors = await load();
    for (const c of vectors.supersede) {
      const manifest: StateManifest = {
        schemaVersion: 2,
        language: c.language,
        lastAppliedChapter: 0,
        projectionVersion: 1,
      };
      const snapshot: RuntimeStateSnapshot = {
        manifest,
        currentState: { chapter: c.chapter - 1, facts: c.inputFacts.map((f) => ({ ...f })) },
        hooks: { hooks: [] },
        chapterSummaries: { rows: [] },
      };
      const delta: RuntimeStateDelta = {
        chapter: c.chapter,
        factOps: { upsert: [{ ...c.tsUpsert }], expire: [] },
        hookOps: { upsert: [], mention: [], resolve: [], defer: [] },
        newHookCandidates: [],
      };
      const next = applyRuntimeStateDelta({ snapshot, delta });
      const actual = canonicalSort(next.currentState.facts);
      const expected = canonicalSort(c.expectedFacts.map((f) => ({ ...f })));
      expect(actual).toEqual(expected);
    }
  });
});
