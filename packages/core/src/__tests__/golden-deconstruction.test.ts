//! G5/350 号：拆书工作台 golden 断言（Phase C 第二大件首批）。
//!
//! 唯一事实源 = `golden/deconstruction-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_deconstruction_diff.rs` 读同一文件差分。
//! 断言：证据索引、四档人物档案、节奏/卖点统计、导出渲染（Extracted content
//! 标记与 reference-context.extractMaterialContent 消费口径对齐）、契约形状。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  analyzePacingStats,
  buildEvidenceIndex,
  deriveCharacterDossier,
  buildDeconstructionExport,
  renderDeconstructionExport,
  DECONSTRUCTION_DEPTHS,
  type DeconChapter,
  type DeconstructionDepth,
} from "../utils/deconstruction.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/deconstruction-vectors.json"), "utf-8"),
) as {
  chapters: DeconChapter[];
  index: { byCharacterKeys: string[]; linDongChapters: number[]; xiaoYanChapters: number[]; chapterTypeSequence: string[] };
  dossiers: Array<{
    name: string;
    depth: DeconstructionDepth;
    expected: Partial<CharacterDossierShape> & {
      name: string;
      appearances: number[];
      firstChapter: number;
      lastChapter: number;
      evidenceCount?: number;
    };
  }>;
  pacing: Array<{ name: string; expected: { counts: Record<string, number>; longestRun: { chapterType: string; length: number }; strongHookDensity: number } }>;
  exportRender: Array<{ name: string; language: "zh" | "en"; expectedContains: string[] }>;
  aggregate: Array<{
    name: string;
    input: { depth: DeconstructionDepth; language: "zh" | "en"; topCharacters: number };
    expected: { dossierNames: string[]; markdownContains: string[] };
  }>;
  contract: unknown;
};

interface CharacterDossierShape {
  name: string;
  appearances: number[];
  firstChapter: number;
  lastChapter: number;
  frequency?: number;
  longestAbsence?: number;
  evidence: Array<{ chapter: number; evidence: string }>;
}

const chapters = vectors.chapters as DeconChapter[];
const index = buildEvidenceIndex(chapters);

describe("deconstruction workbench (G5)", () => {
  it("builds the evidence index per shared vectors", () => {
    expect(Object.keys(index.byCharacter)).toEqual(vectors.index.byCharacterKeys);
    expect(index.byCharacter["林动"]?.map((entry) => entry.chapter)).toEqual(
      vectors.index.linDongChapters,
    );
    expect(index.byCharacter["小炎"]?.map((entry) => entry.chapter)).toEqual(
      vectors.index.xiaoYanChapters,
    );
    expect(index.chapterTypes.map((entry) => entry.chapterType)).toEqual(
      vectors.index.chapterTypeSequence,
    );
  });

  it("derives character dossiers per depth per shared vectors", () => {
    for (const vector of vectors.dossiers) {
      const got = deriveCharacterDossier(index, "林动", vector.depth, 5);
      // depth 字段未在向量重复：校验档位语义关键项。
      expect({ ...got, name: got.name }, `${vector.name} (${vector.depth})`).toMatchObject({
        name: vector.expected.name,
        appearances: vector.expected.appearances,
        firstChapter: vector.expected.firstChapter,
        lastChapter: vector.expected.lastChapter,
      });
      if (vector.expected.frequency !== undefined) {
        expect(got.frequency, vector.name).toBe(vector.expected.frequency);
      } else {
        expect(got.frequency, `${vector.name} frequency absent`).toBeUndefined();
      }
      if (vector.expected.longestAbsence !== undefined) {
        expect(got.longestAbsence, vector.name).toBe(vector.expected.longestAbsence);
      } else {
        expect(got.longestAbsence, `${vector.name} absence absent`).toBeUndefined();
      }
      if (vector.expected.evidenceCount !== undefined) {
        expect(got.evidence.length, vector.name).toBe(vector.expected.evidenceCount);
      }
      if (vector.expected.evidence !== undefined) {
        expect(got.evidence, vector.name).toEqual(vector.expected.evidence);
      }
    }
    expect([...DECONSTRUCTION_DEPTHS]).toEqual(["brief", "standard", "deep", "full"]);
  });

  it("computes pacing stats per shared vectors", () => {
    for (const vector of vectors.pacing) {
      expect(analyzePacingStats(chapters), vector.name).toEqual(vector.expected);
    }
  });

  it("renders exports with the Extracted content marker per shared vectors", () => {
    const dossiers = (["full", "brief"] as DeconstructionDepth[]).map((depth) =>
      deriveCharacterDossier(index, "林动", depth, 5),
    );
    const pacing = analyzePacingStats(chapters);
    for (const vector of vectors.exportRender) {
      const text = renderDeconstructionExport(dossiers, pacing, vector.language);
      for (const fragment of vector.expectedContains) {
        expect(text, `${vector.name}:${fragment}`).toContain(fragment);
      }
    }
  });

  it("aggregates one-shot export per shared vectors", () => {
    for (const vector of vectors.aggregate) {
      const got = buildDeconstructionExport({
        chapters,
        depth: vector.input.depth,
        language: vector.input.language,
        topCharacters: vector.input.topCharacters,
      });
      expect(got.dossiers.map((dossier) => dossier.name), vector.name).toEqual(
        vector.expected.dossierNames,
      );
      for (const fragment of vector.expected.markdownContains) {
        expect(got.markdown, `${vector.name}:${fragment}`).toContain(fragment);
      }
    }
  });

  it("freezes the machine-readable contract shape", () => {
    expect(vectors.contract).toEqual({
      depths: [...DECONSTRUCTION_DEPTHS],
      evidenceCaps: { brief: 0, standard: 3, deep: 10 },
      exportMarker: "## Extracted content",
      consumedBy: "reference-context.extractMaterialContent",
    });
  });
});
