import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 809 号：roster-candidates 正典派生——2.0 原生书无 memory.db 账本（TS 无写链），
 * 账本缺席时不再返回死空壳（currentChapter 0），改从正典 state/chapter_summaries.json
 * 的 characters 列抽候选、对照 works 树 story/entity_roster.md 出卡。Rust 对偶
 * ops_routes get_roster_candidates（同函数族 extract_character_candidates /
 * resolve_roster_candidates）。
 */
describe("roster-candidates canonical derivation (809)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "roster-candidates-canonical-"));
    await mkdir(join(root, ".inkos"), { recursive: true });
    await writeFile(join(root, "inkos.json"), JSON.stringify({ name: "probe", version: "0.1.0", language: "zh" }), "utf-8");
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const storyPath = (bookId: string, ...parts: ReadonlyArray<string>) =>
    join(root, "works", bookId, "source", "story", ...parts);

  const seedSummaries = async (bookId: string) => {
    await mkdir(storyPath(bookId, "state"), { recursive: true });
    await writeFile(storyPath(bookId, "state", "chapter_summaries.json"), JSON.stringify({
      rows: [
        { chapter: 1, title: "一", characters: "苏檀", events: "", stateChanges: "", hookActivity: "", mood: "", chapterType: "" },
        { chapter: 2, title: "二", characters: "苏檀、林动", events: "", stateChanges: "", hookActivity: "", mood: "", chapterType: "" },
      ],
    }), "utf-8");
  };

  it("derives candidates from canonical summaries and cards against the works-tree roster", async () => {
    const app = createStudioServer({} as never, root);
    await seedSummaries("b1");
    await writeFile(storyPath("b1", "entity_roster.md"), [
      "# 实体名册（Entity Roster）",
      "",
      "## entity-su-tan",
      "- name: 苏檀",
      "- aliases: ",
      "- kind: person",
      "- registeredAt: 1",
    ].join("\n"), "utf-8");

    const res = await app.request("/api/v1/books/b1/roster-candidates");
    expect(res.status).toBe(200);
    const body = await res.json() as {
      candidates: string[];
      cards: Array<{ action: string; candidate: string }>;
      rosterExists: boolean;
      currentChapter: number;
    };
    expect(body.currentChapter).toBe(3); // 最后摘要章 2 + 1
    expect([...body.candidates].sort()).toEqual(["林动", "苏檀"]);
    expect(body.rosterExists).toBe(true);
    // 名册已有苏檀 → 不再是 new-entity；林动未登记 → new-entity。
    expect(body.cards.find((card) => card.candidate === "林动")?.action).toBe("new-entity");
    expect(body.cards.find((card) => card.candidate === "苏檀")?.action).not.toBe("new-entity");
  });

  it("reports rosterExists=false with every candidate as new-entity when the roster is absent", async () => {
    const app = createStudioServer({} as never, root);
    await seedSummaries("b2");
    const body = await (await app.request("/api/v1/books/b2/roster-candidates")).json() as {
      candidates: string[];
      cards: Array<{ action: string; candidate: string; confidence: string }>;
      rosterExists: boolean;
      currentChapter: number;
    };
    expect(body.rosterExists).toBe(false);
    expect(body.currentChapter).toBe(3);
    expect(body.cards.map((card) => card.action)).toEqual(body.cards.map(() => "new-entity"));
    expect([...body.cards.map((card) => card.candidate)].sort()).toEqual(["林动", "苏檀"]);
  });

  it("falls back to an empty-but-live shape when neither ledger nor state exists", async () => {
    const app = createStudioServer({} as never, root);
    const body = await (await app.request("/api/v1/books/missing/roster-candidates")).json() as {
      candidates: string[]; cards: unknown[]; rosterExists: boolean; currentChapter: number;
    };
    expect(body).toEqual({ cards: [], candidates: [], rosterExists: false, currentChapter: 1 });
  });
});
