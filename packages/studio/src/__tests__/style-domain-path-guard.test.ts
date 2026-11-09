import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 957 号：写法档案域路径穿越守卫端到端——三站请求名直拼盘路径（档案保存 `{name}.json`
 * 写面 / 绑定 `{profileName}.json` 存在性探针 / 拆书产物 `{name}.md` 写面）统一过 books
 * id 段规范判据（201 号同一真源）。engine-rs ops_routes.rs 同端点同判据同消息同次序对偶。
 */
describe("style domain path guard (957)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "style-domain-guard-"));
    await mkdir(join(root, "books", "b1"), { recursive: true });
    await writeFile(join(root, "books", "b1", "book.json"), '{"id":"b1","title":"原稿"}', "utf-8");
    await writeFile(join(root, "inkos.json"), '{"name":"guard"}', "utf-8");
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const send = async (app: ReturnType<typeof createStudioServer>, method: string, path: string, body: unknown) => {
    const res = await app.request(path, {
      method,
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return { status: res.status, body: await res.json() as { error?: string } };
  };

  it("档案保存: 穿越名 400（目标 book.json 原样），合法中文名照常落盘", async () => {
    const app = createStudioServer({} as never, root);
    const before = await readFile(join(root, "books", "b1", "book.json"), "utf-8");
    for (const evil of ["../../books/b1/book", "../../../outside", "a/b", "a\\b", "x:y"]) {
      const res = await send(app, "POST", "/api/v1/style-profiles", { name: evil, profile: {} });
      expect(res.status, evil).toBe(400);
      expect(res.body.error, evil).toBe("invalid profile name");
    }
    expect(await readFile(join(root, "books", "b1", "book.json"), "utf-8")).toBe(before);
    const ok = await send(app, "POST", "/api/v1/style-profiles", { name: "古风·诗意", profile: {} });
    expect(ok.status).toBe(200);
    await expect(stat(join(root, ".inkos", "style-profiles", "古风·诗意.json"))).resolves.toBeTruthy();
  });

  it("绑定: 穿越名在存在性探针之前 400（inkos.json 在根——修复前 200 且落盘绑定）", async () => {
    const app = createStudioServer({} as never, root);
    const res = await send(app, "PUT", "/api/v1/books/b1/style-binding", { profileName: "../../inkos" });
    expect(res.status).toBe(400);
    expect(res.body.error).toBe("invalid profile name");
    await expect(stat(join(root, "books", "b1", "story", "style_binding.json"))).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("拆书: 穿越名 400 不写盘；chapters 守卫先于名字守卫；首尾空白 trim 落盘", async () => {
    const app = createStudioServer({} as never, root);
    const chapters = [{ chapter: 1, characters: ["林昭"], events: "事件" }];
    const evil = await send(app, "POST", "/api/v1/books/b1/deconstruct", { name: "../../../../escaped", chapters });
    expect(evil.status).toBe(400);
    expect(evil.body.error).toBe("invalid export name");
    await expect(stat(join(root, "escaped.md"))).rejects.toMatchObject({ code: "ENOENT" });
    const order = await send(app, "POST", "/api/v1/books/b1/deconstruct", { name: "../x", chapters: [] });
    expect(order.status).toBe(400);
    expect(order.body.error).toBe("chapters is required");
    const trimmed = await send(app, "POST", "/api/v1/books/b1/deconstruct", { name: "  拆书·卷二  ", chapters });
    expect(trimmed.status).toBe(200);
    await expect(stat(join(root, "books", "b1", "story", "deconstruction", "拆书·卷二.md"))).resolves.toBeTruthy();
  });
});
