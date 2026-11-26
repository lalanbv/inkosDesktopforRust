import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInitialWorkManifestWrite, loadProjectConfig } from "@actalk/inkos-core";
import { afterEach, describe, expect, it } from "vitest";
import { createStudioServer, startupTrashSweep } from "../api/server.js";

// R60 第三批/1685 号：回收站保留期——GET/PUT /api/v1/project/trash-retention 与
// 启动装配点的清扫。GET 读面降级（缺失/损坏按缺省 30）；PUT 是 /project 族写入点
// （错误形态与 writing-goal 同位，config-write-parity.json 登记；非法值平铺 500，
// 与产品引擎 server/project_config_routes.rs get/put_trash_retention 同契约）。
// 清扫只在装配点跑（读端点零写，1682 号定线）：过期书级条目与回滚批次清掉，
// 平铺条目再老也不动（mtime 是内容时刻，不是删除时刻）。

describe("trash retention 端点与启动清扫", () => {
  const roots: string[] = [];
  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  async function tempRoot(config?: string): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "inkos-trash-retention-ep-"));
    roots.push(root);
    if (config !== undefined) await writeFile(join(root, "inkos.json"), config);
    return root;
  }

  const app = (root: string) => createStudioServer({} as never, root);
  const getJson = async (root: string) => (await app(root).request("/api/v1/project/trash-retention")).json();
  const put = async (root: string, body: string) =>
    await app(root).request("/api/v1/project/trash-retention", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body,
    });

  it("GET：缺失/损坏的 inkos.json 都降级为缺省 30", async () => {
    await expect(getJson(await tempRoot())).resolves.toEqual({ retentionDays: 30 });
    await expect(getJson(await tempRoot("{ not json"))).resolves.toEqual({ retentionDays: 30 });
    await expect(getJson(await tempRoot("[1]"))).resolves.toEqual({ retentionDays: 30 });
  });

  it("GET：已配置的值原样读出", async () => {
    const root = await tempRoot(JSON.stringify({ name: "p", version: "2.0.0", trash: { retentionDays: 45 } }));
    await expect(getJson(root)).resolves.toEqual({ retentionDays: 45 });
  });

  it("PUT：合法值落盘并回读；strict 根认识 trash 段（709 同族毒配置回归）", async () => {
    const root = await tempRoot(JSON.stringify({ name: "p", version: "0.1.0" }));
    const response = await put(root, JSON.stringify({ retentionDays: 45 }));
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({ ok: true, retentionDays: 45 });
    const saved = JSON.parse(await readFile(join(root, "inkos.json"), "utf-8"));
    expect(saved.trash).toEqual({ retentionDays: 45 });
    await expect(getJson(root)).resolves.toEqual({ retentionDays: 45 });
    // 写过 trash 段的项目必须还能被 Node 回退腿加载（strict 根不认键 = boot 崩，1429 同族）。
    const loaded = await loadProjectConfig(root, { consumer: "studio", requireApiKey: false });
    expect(loaded.trash?.retentionDays).toBe(45);
  });

  it("PUT：非法值平铺 500、文件逐字节不动", async () => {
    const root = await tempRoot(JSON.stringify({ name: "p", version: "2.0.0" }));
    const before = await readFile(join(root, "inkos.json"), "utf-8");
    for (const bad of ["0", "-1", "1.5", "40000", '"45"', "null"]) {
      const response = await put(root, `{"retentionDays": ${bad}}`);
      expect(response.status, bad).toBe(500);
      await expect(response.json(), bad).resolves.toEqual({
        error: "retentionDays must be an integer between 1 and 36500",
      });
    }
    expect(await readFile(join(root, "inkos.json"), "utf-8")).toBe(before);
  });

  it("PUT：请求体不是 JSON → INTERNAL_ERROR 信封（/project 族同位）", async () => {
    const root = await tempRoot(JSON.stringify({ name: "p", version: "2.0.0" }));
    const response = await put(root, "{ not json");
    expect(response.status).toBe(500);
    await expect(response.json()).resolves.toEqual({
      error: { code: "INTERNAL_ERROR", message: "Unexpected server error." },
    });
  });

  it("PUT：配置损坏 → INTERNAL_ERROR 信封、根不是对象 → PROJECT_CONFIG_INVALID，文件不动", async () => {
    const corrupt = await tempRoot("{ not json");
    expect((await put(corrupt, '{"retentionDays": 30}')).status).toBe(500);
    await expect((await put(corrupt, '{"retentionDays": 30}')).json()).resolves.toEqual({
      error: { code: "INTERNAL_ERROR", message: "Unexpected server error." },
    });
    const array = await tempRoot("[1]");
    const before = await readFile(join(array, "inkos.json"), "utf-8");
    expect((await put(array, '{"retentionDays": 30}')).status).toBe(500);
    await expect((await put(array, '{"retentionDays": 30}')).json()).resolves.toEqual({
      error: { code: "PROJECT_CONFIG_INVALID", message: "Refusing to overwrite inkos.json: its root is not a JSON object. Fix or remove the file, then retry." },
    });
    expect(await readFile(join(array, "inkos.json"), "utf-8")).toBe(before);
  });

  it("PUT：已有多键的 trash 段保留其余键", async () => {
    const root = await tempRoot(JSON.stringify({ name: "p", version: "2.0.0", trash: { futureKey: 1 } }));
    await put(root, JSON.stringify({ retentionDays: 7 }));
    const saved = JSON.parse(await readFile(join(root, "inkos.json"), "utf-8"));
    expect(saved.trash).toEqual({ futureKey: 1, retentionDays: 7 });
  });

  describe("startupTrashSweep（装配点清扫）", () => {
    async function seedBookWithBatch(root: string, id: string, batch: string | null): Promise<string> {
      const initial = createInitialWorkManifestWrite({ workId: id, title: id, profileId: "longform-novel", language: "zh", writes: [] });
      const source = join(root, "works", id, "source");
      await mkdir(join(source, "chapters", ".trash"), { recursive: true });
      await writeFile(join(root, initial.write.relativePath), initial.write.content);
      if (batch !== null) {
        await mkdir(join(source, "chapters", ".trash", batch, "chapters"), { recursive: true });
        await writeFile(join(source, "chapters", ".trash", batch, "chapters", "0003_回.md"), "旧正文");
      }
      return source;
    }

    it("过期书级条目与回滚批次清掉、未过期与平铺留着；retentionDays 缺省走 30", async () => {
      const root = await tempRoot();
      const expiredDir = join(root, ".trash", "books", "gone--20260901000000");
      await mkdir(expiredDir, { recursive: true });
      await writeFile(join(expiredDir, "work.json"), JSON.stringify({ id: "gone" }));
      const freshDir = join(root, ".trash", "books", "kept--20261125000000");
      await mkdir(freshDir, { recursive: true });
      await writeFile(join(freshDir, "work.json"), JSON.stringify({ id: "kept" }));
      const bookDir = await seedBookWithBatch(root, "b1", "rollback-20260901000000-3");
      const freshBookDir = await seedBookWithBatch(root, "b2", "rollback-20261125000000-3");
      await writeFile(join(freshBookDir, "chapters", ".trash", "0009_旧.md"), "更旧");

      const now = Date.parse("2026-11-26T00:00:00.000Z");
      const swept = await startupTrashSweep(root, undefined, now);
      expect(swept).toEqual({ books: 1, batches: 1 });
      const { readdir, stat } = await import("node:fs/promises");
      await expect(stat(expiredDir)).rejects.toMatchObject({ code: "ENOENT" }); // gone 被清掉
      await expect(readFile(join(freshDir, "work.json"), "utf-8")).resolves.toBeTruthy(); // kept 还在
      await expect(readdir(join(bookDir, "chapters", ".trash"))).resolves.toEqual([]); // 过期批次整批清走
      await expect(readdir(join(freshBookDir, "chapters", ".trash"))).resolves.toHaveLength(2); // 未过期批次 + 平铺
    });

    it("装配面传的 retentionDays 决定窗口（7 天时 11-20 的条目在 11-26 还剩 1 天，不清）", async () => {
      const root = await tempRoot();
      const dir = join(root, ".trash", "books", "edge--20261120000000");
      await mkdir(dir, { recursive: true });
      await writeFile(join(dir, "work.json"), JSON.stringify({ id: "edge" }));
      const now = Date.parse("2026-11-26T00:00:00.000Z");
      const swept = await startupTrashSweep(root, 7, now);
      expect(swept).toEqual({ books: 0, batches: 0 });
      await expect(readFile(join(dir, "work.json"), "utf-8")).resolves.toBeTruthy();
    });
  });
});
