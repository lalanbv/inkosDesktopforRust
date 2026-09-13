import { describe, expect, it } from "vitest";
import { buildBookCreatePrefill } from "./radar-prefill";

describe("buildBookCreatePrefill (R27/402)", () => {
  const full = { platform: "起点", genre: "都市悬疑", concept: "水货账房被旧账拖回港口旧案" };

  it("composes a zh draft with platform, genre and premise", () => {
    expect(buildBookCreatePrefill(full, true)).toBe(
      "帮我建一本新书。目标平台：起点。题材：都市悬疑。核心设定：水货账房被旧账拖回港口旧案",
    );
  });

  it("composes an en draft with the same fields", () => {
    const en = buildBookCreatePrefill({ platform: "qidian", genre: "mystery", concept: "laundered ledgers" }, false);
    expect(en).toBe(
      "Help me create a new book. Target platform: qidian. Genre: mystery. Core premise: laundered ledgers",
    );
  });

  it("drops missing fields instead of emitting empty segments", () => {
    expect(buildBookCreatePrefill({ platform: "", genre: "玄幻", concept: "" }, true)).toBe(
      "帮我建一本新书。题材：玄幻。",
    );
    expect(buildBookCreatePrefill({ platform: "", genre: "", concept: "" }, false)).toBe(
      "Help me create a new book.",
    );
  });

  it("trims stray whitespace from radar fields", () => {
    expect(buildBookCreatePrefill({ platform: " 番茄 ", genre: " 玄幻 ", concept: " 符箓 " }, true)).toBe(
      "帮我建一本新书。目标平台：番茄。题材：玄幻。核心设定：符箓",
    );
  });
});
