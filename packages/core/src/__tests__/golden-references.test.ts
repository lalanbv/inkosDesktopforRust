import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  extractMaterialContent,
  slugifyAnchor,
  splitReferenceSections,
} from "../references/reference-context.js";

/**
 * 216 号：references 纯函数域共享 golden 向量（双端守门）。
 * 唯一事实源 = golden/references-vectors.json；engine 侧
 * `engine-rs/tests/golden_references_diff.rs` include_str! 同文件断言。
 */

const vectorsPath = join(import.meta.dirname, "golden", "references-vectors.json");
type SplitVector = {
  name: string;
  input: { materialId: string; title: string; uses: string[]; note?: string | null; content: string };
  output: Array<{
    source: string;
    materialId: string;
    title: string;
    heading: string;
    uses: string[];
    note?: string | null;
    content: string;
  }>;
};

describe("references golden vectors (shared with engine)", () => {
  const vectors = JSON.parse(readFileSync(vectorsPath, "utf-8")) as {
    split: SplitVector[];
    extract: Array<{ name: string; input: string; output: string }>;
    slug: Array<{ name: string; input: string; output: string }>;
  };

  it.each(vectors.split)("split: %s", (vector) => {
    const got = splitReferenceSections({
      materialId: vector.input.materialId,
      title: vector.input.title,
      uses: vector.input.uses,
      ...(vector.input.note ? { note: vector.input.note } : {}),
      content: vector.input.content,
    }).map((section) => ({
      source: section.source,
      materialId: section.materialId,
      title: section.title,
      heading: section.heading,
      uses: section.uses,
      ...(section.note ? { note: section.note } : {}),
      content: section.content,
    }));
    expect(got).toEqual(vector.output);
  });

  it.each(vectors.extract)("extract: %s", (vector) => {
    expect(extractMaterialContent(vector.input)).toBe(vector.output);
  });

  it.each(vectors.slug)("slug: %s", (vector) => {
    expect(slugifyAnchor(vector.input)).toBe(vector.output);
  });
});
