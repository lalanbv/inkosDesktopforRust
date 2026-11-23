import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 1667 号：路由覆盖盲区清偿第二批——不依赖 LLM 的面（校验路径 + 简单成功路径）。
 *
 * 1664 号首扫 34 条零引用路由里清偿第二批。此前这些面只在一次性活体探活里
 * 打过最小载荷（证「可达」），没有任何常驻断言。本文件把状态码、错误码与
 * 关键落盘位置钉成回归面。
 */
describe("route coverage blind spots, batch 2 (1667)", () => {
  let root: string;
  let app: ReturnType<typeof createStudioServer>;
  const J = { "Content-Type": "application/json" };
  const call = async (path: string, method = "GET", body?: unknown) => {
    const res = await app.request(path, { method, headers: J, body: body === undefined ? undefined : JSON.stringify(body) });
    return { status: res.status, body: (await res.json().catch(() => null)) as any };
  };

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "baseline-batch2-"));
    app = createStudioServer({} as never, root);
  });
  afterEach(async () => {
    vi.unstubAllEnvs();
    await rm(root, { recursive: true, force: true });
  });

  it("chapter workspace brief: validation + persisted under the canonical book dir", async () => {
    expect((await call("/api/v1/books/b1/chapters/0/workspace/brief", "PUT", { brief: "x" })).status).toBe(400);
    expect((await call("/api/v1/books/b1/chapters/1/workspace/brief", "PUT", { brief: 5 })).status).toBe(400);

    const saved = await call("/api/v1/books/b1/chapters/1/workspace/brief", "PUT", { brief: "收紧节奏" });
    expect(saved).toEqual({ status: 200, body: { ok: true, chapterNumber: 1, brief: "收紧节奏" } });
    const stored = await readFile(join(root, "works", "b1", "source", "story", "runtime", "chapter-0001.user-brief.md"), "utf-8");
    expect(stored).toContain("收紧节奏");
  });

  it("import family and foundation revise reject empty payloads with their fork messages", async () => {
    expect(await call("/api/v1/books/b1/foundation/revise", "POST", {})).toMatchObject({ status: 400, body: { error: "feedback is required" } });
    expect(await call("/api/v1/books/b1/import/chapters", "POST", {})).toMatchObject({ status: 400, body: { error: "text is required" } });
    expect(await call("/api/v1/books/b1/import/canon", "POST", {})).toMatchObject({ status: 400, body: { error: "fromBookId is required" } });
    expect(await call("/api/v1/books/b1/import/canon-file", "POST", {})).toMatchObject({ status: 400, body: { error: "filePath is required" } });
    expect(await call("/api/v1/import/canon/upload", "POST", {})).toMatchObject({ status: 400, body: { error: { code: "INVALID_CANON_UPLOAD" } } });
  });

  it("derivative-book inits and cover secrets reject invalid input", async () => {
    expect(await call("/api/v1/fanfic/init", "POST", {})).toMatchObject({ status: 400, body: { error: "title and sourceText are required" } });
    expect(await call("/api/v1/imitation/init", "POST", {})).toMatchObject({ status: 400, body: { error: "title, referenceText and storyIdea are required" } });
    expect(await call("/api/v1/spinoff/init", "POST", {})).toMatchObject({ status: 400, body: { error: "title and parentBookId are required" } });
    for (const method of ["GET", "PUT"] as const) {
      expect(await call("/api/v1/cover/secret/no-such-cover-service", method, method === "PUT" ? {} : undefined))
        .toMatchObject({ status: 400, body: { error: "Unsupported cover service" } });
    }
  });

  it("daemon stop without a daemon, and play-run image faces on a missing run", async () => {
    expect(await call("/api/v1/daemon/stop", "POST", {})).toMatchObject({ status: 400, body: { error: "Daemon not running" } });
    expect((await call("/api/v1/play/runs/w1/r1/images/a.png")).status).toBe(404);
    // 缺 work 的生图：不得 200。状态码现为 400 裸 ENOENT 串（应 404——1664 号
    // 立案的双端同形欠账）；修为 404 后本断言无需改动。
    expect((await call("/api/v1/play/runs/w1/r1/generate-image", "POST", {})).status).toBeGreaterThanOrEqual(400);
  });

  it("play-run image settings: booleans normalized and persisted in the run's image settings file", async () => {
    const { PlayStore, readPlayImageSettings } = await import("@actalk/inkos-core");
    const saved = await call("/api/v1/play/runs/w1/r1/image-settings", "PUT", { actors: 1, moments: "", inventory: true });
    expect(saved).toEqual({
      status: 200,
      body: { ok: true, imageSettings: { actors: true, moments: false, inventory: true } },
    });
    expect(await readPlayImageSettings(new PlayStore(root).runDir("w1", "r1")))
      .toEqual({ actors: true, moments: false, inventory: true });
  });

  it("project artifacts refuse paths outside generated writing artifacts", async () => {
    for (const method of ["GET", "PUT"] as const) {
      expect(await call("/api/v1/project/artifacts/cover.png", method, method === "PUT" ? { content: "x" } : undefined))
        .toMatchObject({ status: 400, body: { error: { code: "INVALID_PROJECT_ARTIFACT_PATH" } } });
    }
  });

  it("skills trust: list, add and remove a trusted source directory", async () => {
    expect(await call("/api/v1/skills/trust")).toEqual({ status: 200, body: { entries: [] } });
    expect(await call("/api/v1/skills/trust", "POST", { path: "relative/skills" })).toMatchObject({ status: 400, body: { error: { code: "INVALID_SKILL_TRUST_PATH" } } });
    const missing = join(root, "no-such-skill-dir");
    expect(await call("/api/v1/skills/trust", "POST", { path: missing })).toMatchObject({ status: 404, body: { error: { code: "SKILL_TRUST_PATH_NOT_FOUND" } } });

    const skillDir = join(root, "trusted-skill");
    await mkdir(skillDir);
    await writeFile(join(skillDir, "SKILL.md"), "# trusted-skill\n\n夹具技能。\n", "utf-8");
    const added = await call("/api/v1/skills/trust", "POST", { path: skillDir });
    expect(added.status).toBe(200);
    expect(JSON.stringify(added.body)).toContain(skillDir);
    expect(((await call("/api/v1/skills/trust")).body.entries as unknown[])).toHaveLength(1);

    const removed = await call(`/api/v1/skills/trust?path=${encodeURIComponent(skillDir)}`, "DELETE");
    expect(removed).toMatchObject({ status: 200, body: { ok: true } });
    expect(((await call("/api/v1/skills/trust")).body.entries as unknown[])).toHaveLength(0);
  });

  it("session play-mode: invalid value, ghost session, and a real switch", async () => {
    expect(await call("/api/v1/sessions/ghost/play-mode", "PUT", { playMode: "auto" })).toMatchObject({ status: 400, body: { error: { code: "INVALID_PLAY_MODE" } } });
    expect(await call("/api/v1/sessions/ghost/play-mode", "PUT", { playMode: "guided" })).toMatchObject({ status: 404, body: { error: "Session not found" } });

    const created = await call("/api/v1/sessions", "POST", { sessionKind: "chat" });
    expect(created.status).toBe(200);
    const sessionId = created.body.session.sessionId as string;
    const switched = await call(`/api/v1/sessions/${sessionId}/play-mode`, "PUT", { playMode: "guided" });
    expect(switched.status).toBe(200);
    expect(switched.body.session.playMode).toBe("guided");
    expect((await call(`/api/v1/sessions/${sessionId}`)).body.session.playMode).toBe("guided");
  });

  it("services config import-env: refuses without an api key, imports the project .env credential with it", async () => {
    // 导入源 = 项目根 .env（其次全局 .env）。负向用例写成确定性形态：项目 .env
    // 在位但只有模型键——项目源已检出故不回落全局，无论本机有无全局配置恒 400。
    await writeFile(join(root, ".env"), "INKOS_LLM_MODEL=some-model\n", "utf-8");
    expect((await call("/api/v1/services/config/import-env", "POST", {})).status).toBe(400);

    // 1015/855 号惯用形：凭据只经 env 注入（测试键占位符），源码不落可用字面量。
    // 导入写入项目配置——先有 inkos.json（真实项目的前置；缺失即 500 非本面语义）。
    const apiKey = process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder";
    await writeFile(join(root, "inkos.json"), JSON.stringify({ name: "import-env", version: "0.1.0" }), "utf-8");
    await writeFile(join(root, ".env"), `INKOS_LLM_API_KEY=${apiKey}\nINKOS_LLM_BASE_URL=http://127.0.0.1:9/v1\n`, "utf-8");
    const imported = await call("/api/v1/services/config/import-env", "POST", {});
    expect(imported.status).toBe(200);
    expect(imported.body).toMatchObject({ ok: true, source: "project" });
    const config = JSON.parse(await readFile(join(root, "inkos.json"), "utf-8")) as { llm?: { service?: string; services?: Array<{ baseUrl?: string }> } };
    expect(typeof config.llm?.service).toBe("string");
    expect(config.llm?.services?.[0]?.baseUrl).toBe("http://127.0.0.1:9/v1");
  });
});
