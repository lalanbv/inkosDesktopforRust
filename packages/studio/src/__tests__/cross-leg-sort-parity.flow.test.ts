import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInitialWorkManifestWrite, saveStoryGraph, saveTranslationManifest } from "@actalk/inkos-core";
import { afterEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

// 1684 号（R60 出口：跨腿排序口径全仓扫）——Node 腿有一族列表按 UTF-16 码元序
//（`<` 比较器 / 默认 `.sort()` / `localeCompare`）排，而产品引擎对应端点按码点序
//（Rust 字节序 == Unicode 码点序）。两类序在 BMP 基本区完全一致，只在「增补平面
//（😀 U+1F600）vs 高位 BMP（全角标点！U+FF01、私用区）」这对边界上相反——
// 所有向量都拿这一对钉住：码点序 = ！在 😀 前；UTF-16 码元序相反。
// localeCompare 则连 ASCII 大小写都与码点序不同（ICU 大小写 tertiary）。
// 每个面一条流级测试：期望顺序逐字面（不是子集、不是长度），`<`/locale 复活即红。

const ASTRAL = "😀"; // U+1F600：增补平面
const FULLWIDTH = "！"; // U+FF01：高位 BMP（码点在 😀 之前，UTF-16 码元在其之后）

describe("cross-leg sort parity (codepoint order)", () => {
  const roots: string[] = [];
  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  async function tempRoot(): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "inkos-sort-parity-"));
    roots.push(root);
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "sort-parity-project", version: "2.0.0", language: "zh",
      llm: { model: "test-model", provider: "openai" }, notify: [],
    }));
    return root;
  }

  const STAMP = "2026-08-17T00:00:00.000Z";

  /** 一本书（works 树）：book.json + 一章 + 索引，bookIsThere 与章节轴都认。 */
  async function seedBook(root: string, id: string): Promise<string> {
    const initial = createInitialWorkManifestWrite({ workId: id, title: `书${id}`, profileId: "longform-novel", language: "zh", writes: [] });
    const source = join(root, "works", id, "source");
    await mkdir(join(source, "chapters"), { recursive: true });
    await writeFile(join(root, initial.write.relativePath), initial.write.content);
    await writeFile(join(source, "book.json"), JSON.stringify({
      id, title: `书${id}`, platform: "local", genre: "mystery", status: "active",
      targetChapters: 12, chapterWordCount: 3000, language: "zh", createdAt: STAMP, updatedAt: STAMP,
    }));
    await writeFile(join(source, "chapters", `0001_开篇.md`), `# 第1章 开篇\n\n正文一。`);
    await writeFile(join(source, "chapters", "index.json"), JSON.stringify([
      { number: 1, title: "开篇", status: "approved", wordCount: 5, createdAt: STAMP, updatedAt: STAMP, auditIssues: [], lengthWarnings: [] },
    ]));
    return source;
  }

  /** 一份 works 清单（profileId 任指），图谱/翻译清单用保存器写（过 schema 校验）。 */
  async function seedWork(root: string, id: string, profileId: string): Promise<void> {
    const initial = createInitialWorkManifestWrite({ workId: id, title: id, profileId, language: "zh", writes: [] });
    await mkdir(join(root, "works", id, "source"), { recursive: true });
    await writeFile(join(root, initial.write.relativePath), initial.write.content);
  }

  it("skill documents list in codepoint order (astral after fullwidth)", async () => {
    const root = await tempRoot();
    // project 惯例技能目录（<root>/skills）默认信任、免确认，直接可发现。
    const skillDir = join(root, "skills", "sortdemo-skill");
    await mkdir(join(skillDir, "参考"), { recursive: true });
    await writeFile(join(skillDir, "SKILL.md"), "---\nname: sortdemo\ndescription: 排序口径夹具\n---\n\n正文");
    await writeFile(join(skillDir, `${FULLWIDTH}参考.md`), "甲");
    await writeFile(join(skillDir, `${ASTRAL}参考.md`), "乙");
    await writeFile(join(skillDir, "参考", `${ASTRAL}深层.md`), "丙");
    const app = createStudioServer({} as never, root);
    const listed = await (await app.request("/api/v1/skills")).json() as { skills?: Array<{ id?: string }> };
    const id = listed.skills?.find((skill) => typeof skill.id === "string" && skill.id.includes("sortdemo"))?.id;
    expect(id, "project-convention skill should be discoverable").toBeDefined();
    const res = await app.request(`/api/v1/skills/${encodeURIComponent(id!)}/documents`);
    expect(res.status).toBe(200);
    const { documents } = await res.json() as { documents: Array<{ path: string }> };
    // 码点序：S(0x53) < 参(0x53C2) < ！(0xFF01) < 😀(0x1F600)；UTF-16 码元序会把
    // 😀参考.md 排到 ！参考.md 前面（D83D 代理首单元 < FF01）。
    expect(documents.map((document) => document.path)).toEqual([
      "SKILL.md",
      `参考/${ASTRAL}深层.md`,
      `${FULLWIDTH}参考.md`,
      `${ASTRAL}参考.md`,
    ]);
  });

  it("search entity ties break by codepoint id order", async () => {
    const root = await tempRoot();
    await seedBook(root, "b1");
    await mkdir(join(root, "works", "b1", "source", "story"), { recursive: true });
    await writeFile(join(root, "works", "b1", "source", "story", "entity_codex.json"), JSON.stringify({
      cards: [
        { name: `${ASTRAL}门`, summary: `${ASTRAL}门是关键` },
        { name: `${FULLWIDTH}门`, summary: `${FULLWIDTH}门是关键` },
      ],
    }));
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/books/b1/search?q=%E9%97%A8&scope=entity");
    expect(res.status).toBe(200);
    const { hits } = await res.json() as { hits: Array<{ id: string }> };
    expect(hits.map((hit) => hit.id)).toEqual([`entity:${FULLWIDTH}门`, `entity:${ASTRAL}门`]);
  });

  it("search hook ties break by codepoint id order", async () => {
    const root = await tempRoot();
    await seedBook(root, "b2");
    const stateDir = join(root, "works", "b2", "source", "story", "state");
    await mkdir(stateDir, { recursive: true });
    await writeFile(join(stateDir, "hooks.json"), JSON.stringify({
      hooks: [
        { hookId: `${ASTRAL}1`, startChapter: 1, type: "setup", status: "open", lastAdvancedChapter: 0, expectedPayoff: "", notes: "" },
        { hookId: `${FULLWIDTH}1`, startChapter: 1, type: "setup", status: "open", lastAdvancedChapter: 0, expectedPayoff: "", notes: "" },
      ],
    }));
    await writeFile(join(stateDir, "chapter_summaries.json"), JSON.stringify({ rows: [{ chapter: 1 }] }));
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/books/b2/search?q=1&scope=hook");
    expect(res.status).toBe(200);
    const { hits } = await res.json() as { hits: Array<{ id: string }> };
    expect(hits.map((hit) => hit.id)).toEqual([`hook:${FULLWIDTH}1`, `hook:${ASTRAL}1`]);
  });

  it("truth file list orders roles by codepoint", async () => {
    const root = await tempRoot();
    await seedBook(root, "b3");
    const rolesDir = join(root, "works", "b3", "source", "story", "roles");
    // 四组 roles 目录（zh 主要/次要 + en major/minor）都是开放文件名，逐一钉序。
    for (const dir of ["主要角色", "次要角色", "major", "minor"]) {
      await mkdir(join(rolesDir, dir), { recursive: true });
      await writeFile(join(rolesDir, dir, `${ASTRAL}角.md`), "角色甲");
      await writeFile(join(rolesDir, dir, `${FULLWIDTH}角.md`), "角色乙");
    }
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/books/b3/truth");
    expect(res.status).toBe(200);
    const { files } = await res.json() as { files: Array<{ name: string }> };
    expect(files.map((file) => file.name)).toEqual([
      `roles/主要角色/${FULLWIDTH}角.md`,
      `roles/主要角色/${ASTRAL}角.md`,
      `roles/次要角色/${FULLWIDTH}角.md`,
      `roles/次要角色/${ASTRAL}角.md`,
      `roles/major/${FULLWIDTH}角.md`,
      `roles/major/${ASTRAL}角.md`,
      `roles/minor/${FULLWIDTH}角.md`,
      `roles/minor/${ASTRAL}角.md`,
    ]);
  });

  it("translations list sorts projectIds by descending codepoint", async () => {
    const root = await tempRoot();
    for (const id of ["a-t", "B-t"]) {
      await seedWork(root, id, "translation");
      await saveTranslationManifest(root, {
        id, title: `译${id}`, sourceLanguage: "zh", targetLanguage: "en",
        createdAt: STAMP, updatedAt: STAMP,
        source: { kind: "text", path: "src.txt", charCount: 12 },
        chapters: [],
      });
    }
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/translations");
    expect(res.status).toBe(200);
    const { translations } = await res.json() as { translations: Array<{ projectId: string }> };
    // 降序码点：a(0x61) > B(0x42)。localeCompare 降序会给 B-t 在前（ICU 大小写
    // tertiary 让升序 a < B，倒过来正好相反）。
    expect(translations.map((item) => item.projectId)).toEqual(["a-t", "B-t"]);
  });

  it("interactive films list sorts titles by codepoint", async () => {
    const root = await tempRoot();
    for (const id of ["f1", "f2"]) {
      await seedWork(root, id, "interactive-film");
    }
    const emptyGraph = {
      characters: [], variables: [], nodes: [], endings: [],
    };
    await saveStoryGraph(root, "f1", { schemaVersion: 1, projectId: "f1", title: "中篇", ...emptyGraph });
    await saveStoryGraph(root, "f2", { schemaVersion: 1, projectId: "f2", title: "文篇", ...emptyGraph });
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/interactive-films");
    expect(res.status).toBe(200);
    const { films } = await res.json() as { films: Array<{ projectId: string; title: string }> };
    // 码点序：中(U+4E2D) < 文(U+6587)。zh 排序按拼音 wen < zhong 会给出相反顺序。
    expect(films.map((film) => film.title)).toEqual(["中篇", "文篇"]);
  });

  it("radar history sorts scan files by descending codepoint", async () => {
    const root = await tempRoot();
    const radarDir = join(root, "radar");
    await mkdir(radarDir, { recursive: true });
    // localeCompare 与码点序在「emoji vs 全角」上恰好同序（ICU 把符号排在字母后），
    // 咬 locale 差异要用混合大小写 ASCII：ICU tertiary 让升序 a < B，降序 B 在前；
    // 码点序 a(0x61) > B(0x42)，降序 a 在前。
    await writeFile(join(radarDir, "scan-B1.json"), JSON.stringify({ timestamp: STAMP, marketSummary: "甲" }));
    await writeFile(join(radarDir, "scan-a1.json"), JSON.stringify({ timestamp: STAMP, marketSummary: "乙" }));
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/radar/history");
    expect(res.status).toBe(200);
    const { items } = await res.json() as { items: Array<{ file: string }> };
    expect(items.map((item) => item.file)).toEqual(["scan-a1.json", "scan-B1.json"]);
  });

  it("usage budget statuses order object-form services by codepoint key", async () => {
    const root = await tempRoot();
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "sort-parity-project", version: "2.0.0", language: "zh",
      llm: {
        model: "test-model", provider: "openai", baseUrl: "http://127.0.0.1:9",
        services: {
          [`${ASTRAL}svc`]: { dailyTokenBudget: 100 },
          [`${FULLWIDTH}svc`]: { dailyTokenBudget: 100 },
        },
      },
      notify: [],
    }));
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/usage/budget-status");
    expect(res.status).toBe(200);
    const { statuses } = await res.json() as { statuses: Array<{ serviceKey: string }> };
    expect(statuses.map((status) => status.serviceKey)).toEqual([`${FULLWIDTH}svc`, `${ASTRAL}svc`]);
  });

  it("quality-trend urgency ties break by codepoint hookId", async () => {
    const root = await tempRoot();
    await seedBook(root, "b4");
    const stateDir = join(root, "works", "b4", "source", "story", "state");
    await mkdir(stateDir, { recursive: true });
    await writeFile(join(stateDir, "hooks.json"), JSON.stringify({
      hooks: [
        { hookId: `${ASTRAL}1`, startChapter: 1, type: "setup", status: "open", lastAdvancedChapter: 0, expectedPayoff: "", notes: "" },
        { hookId: `${FULLWIDTH}1`, startChapter: 1, type: "setup", status: "open", lastAdvancedChapter: 0, expectedPayoff: "", notes: "" },
      ],
    }));
    await writeFile(join(stateDir, "chapter_summaries.json"), JSON.stringify({ rows: [{ chapter: 1 }] }));
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/books/b4/quality-trend");
    expect(res.status).toBe(200);
    const { urgency } = await res.json() as { urgency: Array<{ hookId: string }> };
    expect(urgency.map((item) => item.hookId)).toEqual([`${FULLWIDTH}1`, `${ASTRAL}1`]);
  });
});
