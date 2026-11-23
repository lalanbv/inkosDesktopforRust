import { spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { once } from "node:events";
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { createWorkManifest, saveWorkManifest, syncWorkSourceArtifacts } from "@actalk/inkos-core";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 1667 号：路由覆盖盲区清偿——依赖 LLM 的 fork 契约写面（audit / revise）。
 *
 * 1664 号首扫的三条零引用路由。端到端形态与冒烟同构：上游 = 仓内 canonical
 * 假端点 scripts/walkthrough-mock.mjs（自带 submit_chapter_review /
 * submit_revised_chapter 工具协议分支），书 = 已注册 work（能力链要求
 * works/<id>/work.json 清单在位）。锁的是 HTTP 薄层到能力运行时的接线：
 * 状态码、响应包络、以及修订后新版本快照落 works 树。
 */
describe("LLM-backed fork write faces: audit and revise (1667)", () => {
  let root: string;
  let app: ReturnType<typeof createStudioServer>;
  let mock: ChildProcess | undefined;
  let mockBase = "";
  const JSON_HEADERS = { "Content-Type": "application/json" };
  const sourceDir = () => join(root, "works", "b1", "source");
  const CHAPTER = "# 风起\n\n林动在坊市的石桥上停下脚步，远处传来玉符的低鸣。\n\n他把袖中的残卷攥紧，想起师父临别时的叮嘱，没有立刻回头。\n\n桥下的河水映出几盏灯火，也映出一个始终跟在身后的灰衣人影。\n\n";

  beforeAll(async () => {
    const probe = createServer();
    probe.listen(0, "127.0.0.1");
    await once(probe, "listening");
    const port = (probe.address() as { port: number }).port;
    await new Promise<void>((resolveClose) => probe.close(() => resolveClose()));
    const mockScript = fileURLToPath(new URL("../../../../scripts/walkthrough-mock.mjs", import.meta.url));
    mock = spawn(process.execPath, [mockScript, String(port)], { stdio: "ignore" });
    mockBase = `http://127.0.0.1:${port}/v1`;
    const deadline = Date.now() + 15_000;
    for (;;) {
      try {
        if ((await fetch(`${mockBase}/models`)).ok) break;
      } catch { /* 未就绪 */ }
      if (Date.now() > deadline) throw new Error("walkthrough-mock 未在 15s 内就绪");
      await new Promise((resolveWait) => setTimeout(resolveWait, 200));
    }
  }, 30_000);

  afterAll(() => {
    mock?.kill("SIGKILL");
  });

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "fork-write-faces-"));
    await mkdir(join(root, ".inkos"), { recursive: true });
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "fork-write-faces", version: "0.1.0", language: "zh",
      llm: { service: "custom:Mock", defaultModel: "lm-mock-model", services: [{ service: "custom", name: "Mock", baseUrl: mockBase }] },
    }), "utf-8");
    // 1015/855 号惯用形：客户端凭据走 env 注入 + 非凭据回退。
    await writeFile(join(root, ".inkos", "secrets.json"), JSON.stringify({
      services: { "custom:Mock": { apiKey: process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder" } },
    }), "utf-8");
    await mkdir(join(sourceDir(), "chapters"), { recursive: true });
    await writeFile(join(sourceDir(), "book.json"), JSON.stringify({
      id: "b1", title: "镜花水月", platform: "qidian", genre: "xuanhuan", status: "active",
      targetChapters: 12, chapterWordCount: 3000, language: "zh",
      createdAt: "2026-10-16T00:00:00.000Z", updatedAt: "2026-10-16T00:00:00.000Z",
    }), "utf-8");
    await writeFile(join(sourceDir(), "chapters", "0001_风起.md"), CHAPTER, "utf-8");
    // 审稿/修订上下文必读的正典面（与冒烟夹具 §2.5 同构）。
    await mkdir(join(sourceDir(), "story", "outline"), { recursive: true });
    await writeFile(join(sourceDir(), "story", "book_rules.md"), "# 本书规则\n\n（夹具缺省：无附加禁则。）\n", "utf-8");
    await writeFile(join(sourceDir(), "story", "book_rules.json"), `${JSON.stringify({
      version: "2",
      protagonist: { name: "林动", personalityLock: [], behavioralConstraints: [] },
      genreLock: { primary: "xuanhuan", forbidden: [] },
      prohibitions: [],
      enableFullCastTracking: false,
      allowedDeviations: [],
    }, null, 2)}\n`, "utf-8");
    await writeFile(join(sourceDir(), "story", "outline", "story_frame.md"), "# 故事框架\n\n（夹具缺省：三幕结构。）\n", "utf-8");
    await writeFile(join(sourceDir(), "story", "outline", "volume_map.md"), "# 卷册映射\n\n（夹具缺省：第一卷。）\n", "utf-8");
    await saveWorkManifest(root, createWorkManifest({ id: "b1", title: "镜花水月", profileId: "longform-novel", language: "zh" }));
    await syncWorkSourceArtifacts({ projectRoot: root, workId: "b1", accept: true });
    // 修订要以「第 0 章结构化运行时快照」为基线比较前后状态（写链产物）——
    // 夹具用 core 的初始化入口产出（与冒烟夹具 §2.5 同构），否则修订一律
    // 「baseline snapshot 0 is unavailable」。
    const { createInitialRuntimeState, StateManager } = await import("@actalk/inkos-core");
    await createInitialRuntimeState({ bookDir: sourceDir(), language: "zh", hooks: [] });
    await new StateManager(root).snapshotStateAt(sourceDir(), 0);
    app = createStudioServer({} as never, root);
  });

  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("audit runs the review capability and returns the observation envelope", async () => {
    const audited = await app.request("/api/v1/books/b1/audit/1", { method: "POST" });
    expect(audited.status).toBe(200);
    const body = (await audited.json()) as { summary: string; observations: unknown[] };
    expect(typeof body.summary).toBe("string");
    expect(Array.isArray(body.observations)).toBe(true);
    // 审稿是只读动作：正文不被改写。
    expect(await readFile(join(sourceDir(), "chapters", "0001_风起.md"), "utf-8")).toBe(CHAPTER);
  });

  it("revise runs the revise capability, snapshots a version and rewrites the chapter", async () => {
    const revised = await app.request("/api/v1/books/b1/revise/1", {
      method: "POST",
      headers: JSON_HEADERS,
      body: JSON.stringify({ brief: "收紧开篇节奏" }),
    });
    expect(revised.status).toBe(200);
    const versionsDir = join(sourceDir(), "chapters", ".versions", "0001");
    const versions = await readdir(versionsDir);
    // 修订前正文先入版本快照（来源 revision；快照存正文=不含首行标题），
    // 当前正文 = 修订结果。
    const snapshot = versions.find((name) => name.includes("_revision_"));
    expect(snapshot).toBeDefined();
    expect(await readFile(join(versionsDir, snapshot!), "utf-8")).toBe(CHAPTER.split("\n").slice(2).join("\n"));
    const rewritten = await readFile(join(sourceDir(), "chapters", "0001_风起.md"), "utf-8");
    expect(rewritten).not.toBe(CHAPTER);
    const body = (await revised.json()) as { kind: string; bookId: string; chapterNumber: number; changed: boolean; wordCount: number };
    expect(body).toMatchObject({ kind: "chapter_revision", bookId: "b1", chapterNumber: 1, changed: true });
    expect(body.wordCount).toBeGreaterThan(0);
  }, 30_000);

  it("rewrite runs the same capability in rework mode and reports completion", async () => {
    const reworked = await app.request("/api/v1/books/b1/rewrite/1", {
      method: "POST",
      headers: JSON_HEADERS,
      body: JSON.stringify({ brief: "换一个开场视角" }),
    });
    expect(reworked.status).toBe(200);
    const body = (await reworked.json()) as { status: string; bookId: string; chapter: number; result: { changed?: boolean } };
    expect(body).toMatchObject({ status: "complete", bookId: "b1", chapter: 1 });
    expect(body.result.changed).toBe(true);
    // rewrite 带 brief 时先落用户提示（章工作区），再走修订。
    expect(await readFile(join(sourceDir(), "chapters", "0001_风起.md"), "utf-8")).not.toBe(CHAPTER);
  }, 30_000);
});
