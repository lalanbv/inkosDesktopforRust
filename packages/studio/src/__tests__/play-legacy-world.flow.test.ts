import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

// 1675 号：产品引擎建的世界（只有 legacy 根 `worlds/<id>/` 那棵树）在 Node 腿的读取面
// 上从 404 变成读得到——首次触达按需迁移到 works，文件图谱在第一次开库时导入。

describe("interactive worlds created by the product engine", () => {
  const roots: string[] = [];
  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  async function projectRoot(): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "inkos-studio-play-legacy-"));
    roots.push(root);
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "legacy-play-project", version: "2.0.0", language: "zh",
      llm: { model: "test-model", provider: "openai" }, notify: [],
    }));
    return root;
  }

  async function seedLegacyWorld(root: string, id: string): Promise<string> {
    const runDir = join(root, "worlds", id, "runs", "main");
    await mkdir(join(runDir, "state"), { recursive: true });
    await writeFile(join(root, "worlds", id, "world.json"), JSON.stringify({
      id, title: "产品引擎世界", premise: "镜中图书馆没有出口。", worldContract: "", visualContract: "",
      mode: "open", language: "zh",
      createdAt: "2026-01-01T00:00:00.000Z", updatedAt: "2026-01-01T00:00:00.000Z",
    }, null, 2));
    await writeFile(join(runDir, "transcript.jsonl"), `${JSON.stringify({ role: "user", content: "产品引擎第一问", timestamp: 1 })}\n`);
    await writeFile(join(runDir, "state", "current.json"), `${JSON.stringify({ turn: 1, lastSummary: "产品引擎第一问" }, null, 2)}\n`);
    await writeFile(join(runDir, "play-graph.json"), JSON.stringify({
      entities: {
        "engine-hero": { id: "engine-hero", type: "actor", label: "引擎来客", summary: "", status: "", note: "模型多给的键" },
      },
      edges: {}, stateSlots: {},
      events: { e1: { id: "e1", turn: 1, actionKind: "user_input", rawInput: "产品引擎第一问", outcomeSummary: "", createdAt: "2026-01-01T00:00:01.000Z" } },
    }, null, 2));
    return runDir;
  }

  it("are readable on the first request, and the second request sees the same graph", async () => {
    const root = await projectRoot();
    await seedLegacyWorld(root, "w-engine");
    const app = createStudioServer({} as never, root);

    const first = await app.request("/api/v1/play/runs/w-engine/main");
    expect(first.status).toBe(200);
    const body = await first.json() as { title: string; transcript: { content: string }[]; currentState: { turn: number }; graph: { entities: { id: string; label: string }[]; events: { rawInput: string }[] } };
    expect(body.title).toBe("产品引擎世界");
    expect(body.transcript.map((turn) => turn.content)).toEqual(["产品引擎第一问"]);
    expect(body.currentState.turn).toBe(1);
    // 文件图谱导入成了 sqlite 图谱；模型多给的键没有跟进来。
    expect(body.graph.entities).toEqual([{ id: "engine-hero", type: "actor", label: "引擎来客", summary: "", status: "" }]);
    expect(body.graph.events.map((event) => event.rawInput)).toEqual(["产品引擎第一问"]);

    const second = await app.request("/api/v1/play/runs/w-engine/main");
    expect(second.status).toBe(200);
    expect((await second.json() as { graph: { entities: unknown[] } }).graph.entities).toEqual(body.graph.entities);

    const manifest = JSON.parse(await readFile(join(root, "works", "w-engine", "work.json"), "utf8")) as { profileId: string; metadata: { legacySource: string } };
    expect(manifest.profileId).toBe("interactive-world");
    expect(manifest.metadata.legacySource).toBe("worlds/w-engine");
    // 迁移是拷贝：产品引擎那棵树原样保留。
    expect(await readFile(join(root, "worlds", "w-engine", "runs", "main", "transcript.jsonl"), "utf8")).toContain("产品引擎第一问");
  });

  it("still answer 404 for a world neither root has", async () => {
    const root = await projectRoot();
    const app = createStudioServer({} as never, root);
    const response = await app.request("/api/v1/play/runs/w-nowhere/main");
    expect(response.status).toBe(404);
    expect(await response.json()).toEqual({ error: { code: "PLAY_WORLD_NOT_FOUND" } });
  });

  it("answer 404 for a world without that run", async () => {
    const root = await projectRoot();
    await seedLegacyWorld(root, "w-engine");
    const app = createStudioServer({} as never, root);
    const response = await app.request("/api/v1/play/runs/w-engine/other-run");
    expect(response.status).toBe(404);
    expect(await response.json()).toEqual({ error: { code: "PLAY_RUN_NOT_FOUND" } });
  });

  it("serve the illustrations the product engine generated", async () => {
    const root = await projectRoot();
    const runDir = await seedLegacyWorld(root, "w-image");
    await mkdir(join(runDir, "images"), { recursive: true });
    const png = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    await writeFile(join(runDir, "images", "scene-turn-1.png"), png);

    const app = createStudioServer({} as never, root);
    const response = await app.request("/api/v1/play/runs/w-image/main/images/scene-turn-1.png");
    expect(response.status).toBe(200);
    expect(Buffer.from(await response.arrayBuffer())).toEqual(png);
  });

  it("take image settings without leaving an orphan works directory that would block the migration", async () => {
    const root = await projectRoot();
    await seedLegacyWorld(root, "w-settings");
    const app = createStudioServer({} as never, root);

    const response = await app.request("/api/v1/play/runs/w-settings/main/image-settings", {
      method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ actors: true }),
    });
    expect(response.status).toBe(200);
    expect(JSON.parse(await readFile(join(root, "works", "w-settings", "source", "runs", "main", "images", "settings.json"), "utf8"))).toMatchObject({ actors: true });
    // 先迁移再写：works 里这份带着清单，不是孤儿目录。
    expect(JSON.parse(await readFile(join(root, "works", "w-settings", "work.json"), "utf8"))).toMatchObject({ profileId: "interactive-world" });
  });
});
