import { mkdtemp, mkdir, readFile, rm, utimes, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { syncWorkSourceArtifacts } from "../harness/source-sync.js";
import { loadWorkManifest } from "../harness/work-store.js";
import { createPlayDB } from "../play/play-db-factory.js";
import { PlayStore } from "../play/play-store.js";

// 1675 号：互动叙事存储桥接第二批——Node 腿认得产品引擎创建的世界。
//
// 产品引擎把世界写在 legacy 根 `worlds/<id>/`（新运行走文件后端图谱）；TS 2.0 只认
// `works/<id>/source/`。Node 腿首次触达一个 legacy 根世界时按需迁移到 works（复用
// 现成的 migrateLegacyWorks），文件图谱在 sqlite 图谱第一次打开时导入。
//
// 「两棵树都在」的对账规则：进度较新的那份为准。产品引擎按它选根；Node 腿只能写
// works，所以 legacy 根那份较新时拒绝续写。

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function tempRoot(prefix: string): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), prefix));
  roots.push(root);
  return root;
}

/// 产品引擎建的世界：只有 `worlds/<id>/` 那棵树——世界定义、一轮转录、当前状态、
/// 文件后端的图谱（四张以 id 为键的记录表，形态同 Rust `persist_file`）。
async function seedLegacyWorld(root: string, id: string): Promise<void> {
  const worldDir = join(root, "worlds", id);
  const runDir = join(worldDir, "runs", "main");
  await mkdir(join(runDir, "state"), { recursive: true });
  await writeFile(
    join(worldDir, "world.json"),
    JSON.stringify(
      {
        id,
        title: "产品引擎世界",
        premise: "镜中图书馆没有出口。",
        worldContract: "",
        visualContract: "",
        mode: "open",
        language: "zh",
        createdAt: "2026-01-01T00:00:00.000Z",
        updatedAt: "2026-01-01T00:00:00.000Z",
      },
      null,
      2,
    ),
  );
  await writeFile(
    join(runDir, "transcript.jsonl"),
    `${JSON.stringify({ role: "user", content: "产品引擎第一问", timestamp: 1 })}\n`,
  );
  await writeFile(
    join(runDir, "state", "current.json"),
    `${JSON.stringify({ turn: 1, lastSummary: "产品引擎第一问" }, null, 2)}\n`,
  );
  await writeFile(
    join(runDir, "play-graph.json"),
    JSON.stringify(
      {
        entities: {
          "engine-hero": { id: "engine-hero", type: "actor", label: "引擎来客", summary: "", status: "" },
        },
        edges: {},
        stateSlots: {},
        events: {
          e1: {
            id: "e1",
            turn: 1,
            actionKind: "user_input",
            rawInput: "产品引擎第一问",
            outcomeSummary: "",
            createdAt: "2026-01-01T00:00:01.000Z",
          },
        },
      },
      null,
      2,
    ),
  );
}

describe("legacy-root interactive worlds on the TS leg", () => {
  it("are brought into works on first touch and the original tree is kept", async () => {
    const root = await tempRoot("inkos-play-legacy-store-");
    await seedLegacyWorld(root, "w-legacy");

    const world = await new PlayStore(root).loadWorld("w-legacy");
    expect(world).toMatchObject({ id: "w-legacy", title: "产品引擎世界" });

    const manifest = await loadWorkManifest(root, "w-legacy");
    expect(manifest.profileId).toBe("interactive-world");
    expect(manifest.metadata.legacySource).toBe("worlds/w-legacy");

    // 迁移是拷贝：原目录原样保留。
    expect(await readFile(join(root, "worlds", "w-legacy", "world.json"), "utf8")).toContain(
      "产品引擎世界",
    );
    // works 那份带全了运行文件。
    const transcript = await readFile(
      join(root, "works", "w-legacy", "source", "runs", "main", "transcript.jsonl"),
      "utf8",
    );
    expect(transcript).toContain("产品引擎第一问");
    // 迁移之后路径函数都落在 works 那棵树上。
    expect(new PlayStore(root).runDir("w-legacy", "main")).toBe(
      join(root, "works", "w-legacy", "source", "runs", "main"),
    );
  });

  it("are listed with works-native worlds", async () => {
    const root = await tempRoot("inkos-play-legacy-list-");
    await seedLegacyWorld(root, "w-list");
    const store = new PlayStore(root);
    await store.createWorld({
      id: "w-native",
      title: "TS 腿世界",
      premise: "",
      worldContract: "",
      visualContract: "",
      mode: "open",
      language: "zh",
    });

    const worlds = await store.listWorlds();
    expect(worlds.map((world) => world.id).sort()).toEqual(["w-list", "w-native"]);
  });

  it("let the turn-tail sync resolve once they have a manifest", async () => {
    const root = await tempRoot("inkos-play-legacy-sync-");
    await seedLegacyWorld(root, "w-legacy");

    // 取证：没迁移之前，回合收尾同步的第一步 loadWorkManifest 直接 ENOENT——没有清单的
    // 世界即使被看见，回合也会在收尾同步处失败。
    await expect(
      syncWorkSourceArtifacts({
        projectRoot: root,
        workId: "w-legacy",
        accept: true,
        acceptPaths: ["source/runs/main/transcript.jsonl"],
      }),
    ).rejects.toMatchObject({ code: "ENOENT" });

    await new PlayStore(root).loadWorld("w-legacy");
    const manifest = await syncWorkSourceArtifacts({
      projectRoot: root,
      workId: "w-legacy",
      accept: true,
      acceptPaths: ["source/runs/main/transcript.jsonl"],
    });
    expect(manifest.id).toBe("w-legacy");
  });

  it("have their file-backend graph imported the first time the sqlite graph opens", async () => {
    const root = await tempRoot("inkos-play-legacy-graph-");
    await seedLegacyWorld(root, "w-legacy");
    const store = new PlayStore(root);
    await store.loadWorld("w-legacy");
    const runDir = store.runDir("w-legacy", "main");

    const db = createPlayDB(runDir);
    const snapshot = db.snapshot();
    expect(snapshot.entities.map((entity) => entity.label)).toEqual(["引擎来客"]);
    expect(snapshot.events.map((event) => event.rawInput)).toEqual(["产品引擎第一问"]);
    db.close?.();

    // 之后再开：导入不得重跑（重跑会用文件里的旧记录盖掉导入之后写进去的东西）。
    const second = createPlayDB(runDir);
    second.upsertEntity({ id: "late", type: "actor", label: "后来者", summary: "" });
    second.upsertEntity({ id: "engine-hero", type: "actor", label: "导入之后改的名", summary: "" });
    second.close?.();
    const third = createPlayDB(runDir);
    expect(
      third.snapshot().entities.map((entity) => [entity.id, entity.label]).sort(),
    ).toEqual([
      ["engine-hero", "导入之后改的名"],
      ["late", "后来者"],
    ]);
    third.close?.();
  });

  it("start an empty sqlite graph when there is no file graph to import", async () => {
    const root = await tempRoot("inkos-play-legacy-empty-");
    const runDir = join(root, "works", "w-fresh", "source", "runs", "main");
    await mkdir(runDir, { recursive: true });
    const db = createPlayDB(runDir);
    expect(db.snapshot()).toEqual({ entities: [], edges: [], stateSlots: [], events: [] });
    db.close?.();
  });

  it("drop the keys the sqlite graph has no column for, and null optional keys", async () => {
    const root = await tempRoot("inkos-play-legacy-columns-");
    await seedLegacyWorld(root, "w-alias");
    // 产品引擎存的是模型输出的原样对象：别名键（play_parser backfill_edges 只补规范键、
    // 不删别名）与模型多给的键都还在；从 sqlite 行转出来的记录带着 null 的可选键。
    await writeJson(join(root, "worlds", "w-alias", "runs", "main", "play-graph.json"), {
      entities: {
        "engine-hero": {
          id: "engine-hero", type: "actor", label: "引擎来客", summary: "", status: "",
          createdEventId: null, updatedEventId: null, note: "模型多给的键",
        },
      },
      edges: {
        "e-rel": {
          id: "e-rel", from: "engine-hero", fromId: "engine-hero", relation: "knows", type: "knows",
          toId: "ts-hero", value: null, validFromEventId: "e1", validUntilEventId: null,
          sourceEventId: "e1", visibility: null, strength: null, weight: 3,
        },
      },
      stateSlots: {
        "slot-mood": {
          id: "slot-mood", ownerEntityId: "engine-hero", kind: "flag", label: "心情",
          value: "警惕", updatedEventId: "e1",
        },
      },
      events: { e1: { ...EVENT, note: "多给的键" } },
    });
    const store = new PlayStore(root);
    await store.loadWorld("w-alias");
    const db = createPlayDB(store.runDir("w-alias", "main"));
    const snapshot = db.snapshot();
    db.close?.();
    expect(snapshot.entities).toEqual([
      { id: "engine-hero", type: "actor", label: "引擎来客", summary: "", status: "" },
    ]);
    expect(snapshot.edges).toEqual([
      {
        id: "e-rel", fromId: "engine-hero", type: "knows", toId: "ts-hero", value: {},
        validFromEventId: "e1", validUntilEventId: null, sourceEventId: "e1", visibility: {},
      },
    ]);
    expect(snapshot.stateSlots).toEqual([
      { id: "slot-mood", ownerEntityId: "engine-hero", kind: "flag", label: "心情", value: "警惕", updatedEventId: "e1" },
    ]);
    expect(snapshot.events).toEqual([EVENT]);
  });

  it("refuse to import a record the sqlite graph cannot represent, before creating the database", async () => {
    const root = await tempRoot("inkos-play-legacy-invalid-");
    await seedLegacyWorld(root, "w-bad");
    const graphPath = join(root, "worlds", "w-bad", "runs", "main", "play-graph.json");
    await writeJson(graphPath, {
      entities: { ok: { id: "ok", type: "actor", label: "好记录", summary: "" } },
      edges: {},
      stateSlots: {},
      // 产品引擎的 is_valid_event 只要求 rawInput 在场，空串也收；TS 这边 min(1)。
      events: { e0: { ...EVENT, id: "e0", rawInput: "" } },
    });
    const store = new PlayStore(root);
    await store.loadWorld("w-bad");
    const runDir = store.runDir("w-bad", "main");
    const before = await readFile(join(runDir, "play-graph.json"), "utf8");

    expect(() => createPlayDB(runDir)).toThrow(expect.objectContaining({ code: "PLAY_GRAPH_INVALID" }));
    // 库没有建出来：产品引擎照旧读文件里那份，记录一条没丢。文件也没被动过。
    await expect(readFile(join(runDir, "play.db"), "utf8")).rejects.toMatchObject({ code: "ENOENT" });
    expect(await readFile(join(runDir, "play-graph.json"), "utf8")).toBe(before);

    // 内容根本不是 JSON（写坏了）同理：不留下一个空库冒充它。
    await writeFile(join(runDir, "play-graph.json"), "{", "utf8");
    expect(() => createPlayDB(runDir)).toThrow(
      expect.objectContaining({ code: "PLAY_GRAPH_INVALID", message: expect.stringContaining("not valid JSON") }),
    );
    await expect(readFile(join(runDir, "play.db"), "utf8")).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("refuse to write when the legacy copy is the newer one", async () => {
    const root = await tempRoot("inkos-play-legacy-behind-");
    await seedLegacyWorld(root, "w-behind");
    const store = new PlayStore(root);
    await store.loadWorld("w-behind");
    // 迁移之后 works 是较新的那份：照常写。
    await store.appendTranscriptTurn("w-behind", "main", { role: "user", content: "Node 腿一问", timestamp: 2 });
    expect(await store.loadWorld("w-behind")).not.toBeNull();

    // 显式迁移过的世界在 1675 号之前的处境：用户接着在产品引擎上玩，legacy 根成了较新的那份。
    const later = new Date(Date.now() + 60_000);
    await utimes(join(root, "worlds", "w-behind", "runs", "main", "transcript.jsonl"), later, later);

    // 读照常（读到的是落后副本，与此前一样），写入一律拒绝。
    expect((await store.readTranscript("w-behind", "main")).map((turn) => turn.content)).toEqual([
      "产品引擎第一问",
      "Node 腿一问",
    ]);
    const refused = { code: "PLAY_WORLD_DIVERGED" };
    await expect(store.appendTranscriptTurn("w-behind", "main", { role: "user", content: "x", timestamp: 3 })).rejects.toMatchObject(refused);
    await expect(store.saveCurrentState("w-behind", "main", { turn: 3 })).rejects.toMatchObject(refused);
    await expect(store.updateWorld("w-behind", { premise: "改不动" })).rejects.toMatchObject(refused);
    // 什么都没写下去：works 那份的转录还是两行，世界定义原样。works 一变新，产品引擎
    // 就会换到这份落后副本上。
    expect((await store.readTranscript("w-behind", "main")).length).toBe(2);
    expect(await store.loadWorld("w-behind")).toMatchObject({ premise: "镜中图书馆没有出口。" });

    // 打平（两边进度文件的时刻相同——保留时刻的拷贝、备份还原后就是这个形态）放行：
    // 两腿对打平的取法一致，都是 works。
    const tied = new Date("2026-06-01T00:00:00Z");
    await pinProgressClock(join(root, "worlds", "w-behind"), tied);
    await pinProgressClock(join(root, "works", "w-behind", "source"), tied);
    await store.appendTranscriptTurn("w-behind", "main", { role: "user", content: "打平之后", timestamp: 4 });
    expect((await store.readTranscript("w-behind", "main")).length).toBe(3);
  });

  it("refuse to create a world over one that only the legacy root has and could not be brought over", async () => {
    const root = await tempRoot("inkos-play-legacy-over-");
    const worldDir = join(root, "worlds", "w-broken");
    await mkdir(worldDir, { recursive: true });
    await writeFile(join(worldDir, "world.json"), "{", "utf8");

    const store = new PlayStore(root);
    expect(await store.loadWorld("w-broken")).toBeNull();
    await expect(
      store.createWorld({ id: "w-broken", title: "新的", premise: "", worldContract: "", visualContract: "", mode: "open", language: "zh" }),
    ).rejects.toMatchObject({ code: "PLAY_WORLD_DIVERGED" });
    // 新建的那份会变成较新的那份，产品引擎跟着换过来——所以 works 里不能留下它。
    await expect(readFile(join(root, "works", "w-broken", "source", "world.json"), "utf8")).rejects.toMatchObject({ code: "ENOENT" });
  });
});

const EVENT = { id: "e1", turn: 1, actionKind: "user_input", rawInput: "产品引擎第一问", outcomeSummary: "", createdAt: "2026-01-01T00:00:01.000Z" };

/** 把一份世界的四类进度文件的修改时间钉到同一时刻（不在的文件跳过）。 */
async function pinProgressClock(worldDir: string, at: Date): Promise<void> {
  for (const file of ["world.json", "runs/main/transcript.jsonl", "runs/main/state/current.json", "runs/main/play-graph.json"]) {
    await utimes(join(worldDir, file), at, at).catch(() => undefined);
  }
}

async function writeJson(path: string, value: unknown): Promise<void> {
  await writeFile(path, JSON.stringify(value, null, 2));
}
