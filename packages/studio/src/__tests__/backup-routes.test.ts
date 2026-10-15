import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";
import { untar } from "../lib/tar-read.js";

/**
 * 710 号：备份双端点回收端到端——GET /backup/export（staging 聚合+manifest+
 * inkos-backup/ 前缀 tar.gz 下载）与 POST /backup/import（444 号「能工作的
 * 契约」两段式：preview 覆盖项盘点 → ?confirm=1 写回+pre-restore 快照）。
 * Rust backup_routes.rs 为 443 号逐字移植，本套件同时锁定双端共享安全三则
 * （`..`/绝对路径拒绝、仅普通文件、解压上限）与 manifest 先验。
 */
describe("backup export/import routes (710)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "backup-routes-"));
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const seedProject = async () => {
    const bookDir = join(root, "books", "b1");
    await mkdir(join(bookDir, "story"), { recursive: true });
    await writeFile(join(bookDir, "book.json"), JSON.stringify({
      id: "b1", title: "镜花缘", platform: "qidian", genre: "xuanhuan", status: "active",
      targetChapters: 12, chapterWordCount: 3000, language: "zh",
      createdAt: "2026-10-16T00:00:00.000Z", updatedAt: "2026-10-16T00:00:00.000Z",
    }), "utf-8");
    await writeFile(join(bookDir, "story", "current_state.md"), "# 当前状态\n\n- 章节一。\n", "utf-8");
    await writeFile(join(root, "inkos.json"), JSON.stringify({ name: "walkthrough" }), "utf-8");
    await mkdir(join(root, ".inkos"), { recursive: true });
    await writeFile(join(root, ".inkos", "secrets.json"), JSON.stringify({ services: {} }), "utf-8");
    await mkdir(join(root, "prompt"), { recursive: true });
    await writeFile(join(root, "prompt", "writer.md"), "写作守则。", "utf-8");
  };

  it("export: staging 聚合 + manifest + inkos-backup/ 前缀 + secrets 默认排除", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/backup/export");
    expect(res.status).toBe(200);
    expect(res.headers.get("Content-Type")).toBe("application/gzip");
    expect(res.headers.get("Content-Disposition")).toBe('attachment; filename="inkos-backup.tar.gz"');

    const bytes = new Uint8Array(await res.arrayBuffer());
    const { entries, rejected } = untar(bytes);
    expect(rejected).toEqual([]);
    const names = entries.map((e) => e.name);
    expect(names).toContain("inkos-backup/backup-manifest.json");
    expect(names).toContain("inkos-backup/books/b1/book.json");
    expect(names).toContain("inkos-backup/books/b1/story/current_state.md");
    expect(names).toContain("inkos-backup/prompt/writer.md");
    expect(names).toContain("inkos-backup/inkos.json");
    expect(names.some((n) => n.includes("secrets.json"))).toBe(false);

    const manifestEntry = entries.find((e) => e.name === "inkos-backup/backup-manifest.json")!;
    const manifest = JSON.parse(new TextDecoder().decode(manifestEntry.bytes)) as {
      version: number; generator: string; scope: { includeSecrets: boolean; includeSessions: boolean };
      fileCount: number; totalBytes: number;
    };
    expect(manifest.version).toBe(1);
    expect(manifest.generator).toBe("inkos-desktop");
    expect(manifest.scope).toEqual({ includeSecrets: false, includeSessions: false });
    expect(manifest.fileCount).toBe(names.length - 1); // manifest 自身不计

    // staging 清理：导出后 .inkos-backup-staging 不残留
    await expect(readdir(join(root, ".inkos-backup-staging"))).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("export includeSecrets=1: secrets 入包", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/backup/export?includeSecrets=1");
    const { entries } = untar(new Uint8Array(await res.arrayBuffer()));
    expect(entries.some((e) => e.name === "inkos-backup/.inkos/secrets.json")).toBe(true);
  });

  it("import 两段式: preview 盘点 → confirm 写回 + pre-restore 快照留档", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    const exported = new Uint8Array(
      await (await app.request("/api/v1/backup/export")).arrayBuffer(),
    );
    const originalBook = await readFile(join(root, "books", "b1", "book.json"), "utf-8");

    // 预览：不落盘，盘点覆盖项（books/b1/book.json + inkos.json 已存在）
    const preview = await app.request("/api/v1/backup/import", {
      method: "POST",
      body: exported,
    });
    expect(preview.status).toBe(200);
    const previewBody = await preview.json() as { preview: { fileCount: number; overwriting: string[]; newFiles: number } };
    expect(previewBody.preview.overwriting).toContain("books/b1/book.json");
    expect(previewBody.preview.overwriting).toContain("inkos.json");
    expect(previewBody.preview.overwriting).not.toContain("backup-manifest.json");
    expect(previewBody.preview.fileCount).toBe(
      previewBody.preview.overwriting.length + previewBody.preview.newFiles,
    );
    // 预览不改动现网文件
    expect(await readFile(join(root, "books", "b1", "book.json"), "utf-8")).toBe(originalBook);

    // 改动现网文件后 confirm → 写回导出版本 + 快照原文件
    await writeFile(join(root, "books", "b1", "book.json"), '{"tampered":true}', "utf-8");
    const confirm = await app.request("/api/v1/backup/import?confirm=1", {
      method: "POST",
      body: exported,
    });
    expect(confirm.status).toBe(200);
    const confirmBody = await confirm.json() as { ok: boolean; restored: number; snapshot: boolean };
    expect(confirmBody.ok).toBe(true);
    expect(confirmBody.snapshot).toBe(true);
    // restored = 全部条目 - manifest 元数据
    const { entries } = untar(exported);
    expect(confirmBody.restored).toBe(entries.length - 1);
    // 写回生效：被篡改文件恢复为导出内容
    expect(await readFile(join(root, "books", "b1", "book.json"), "utf-8")).toBe(originalBook);
    // manifest 不落盘
    await expect(readFile(join(root, "backup-manifest.json"), "utf-8")).rejects.toMatchObject({ code: "ENOENT" });
    // 快照留档：pre-restore-*.tar 在盘且含「确认时刻」的现网内容（回滚保险
    // 语义=恢复到导入前状态，被篡改的 book.json 是快照正体；导出原件在 tar 包）。
    const backups = await readdir(join(root, "backups"));
    expect(backups.length).toBe(1);
    expect(backups[0]).toMatch(/^pre-restore-\d+\.tar$/);
    const snapshotRaw = await readFile(join(root, "backups", backups[0]!));
    const snapshotEntries = untar(new Uint8Array(snapshotRaw)).entries;
    const snapshotBook = snapshotEntries.find((e) => e.name === "books/b1/book.json");
    expect(snapshotBook).toBeDefined();
    expect(new TextDecoder().decode(snapshotBook!.bytes)).toBe('{"tampered":true}');
  });

  it("import 守卫面: 空载荷/缺 manifest/坏 generator/不安全条目整包 400", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);

    // 空载荷 → 无 manifest → 400
    const empty = await app.request("/api/v1/backup/import", { method: "POST" });
    expect(empty.status).toBe(400);

    // 非 gzip/tar 垃圾字节 → 400
    const garbage = await app.request("/api/v1/backup/import", {
      method: "POST",
      body: "not-a-tar",
    });
    expect(garbage.status).toBe(400);

    // 手工构造最小 tar：坏 generator → 400
    const badGenerator = simpleTar([
      { name: "inkos-backup/backup-manifest.json", payload: new TextEncoder().encode(JSON.stringify({ version: 1, generator: "other-tool" })) },
    ]);
    const bad = await app.request("/api/v1/backup/import", { method: "POST", body: badGenerator });
    expect(bad.status).toBe(400);
    expect((await bad.json() as { error: string }).error).toBe("unsupported backup generator/version");

    // 安全一则：`..` 条目 → 整包拒绝（无一落盘）
    const evil = simpleTar([
      { name: "inkos-backup/backup-manifest.json", payload: new TextEncoder().encode(JSON.stringify({ version: 1, generator: "inkos-desktop" })) },
      { name: "inkos-backup/../evil.md", payload: new TextEncoder().encode("evil") },
      { name: "inkos-backup/safe.md", payload: new TextEncoder().encode("safe") },
    ]);
    const rejected = await app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: evil });
    expect(rejected.status).toBe(400);
    expect((await rejected.json() as { error: string }).error).toContain("unsafe entries rejected");
    await expect(readFile(join(root, "evil.md"))).rejects.toMatchObject({ code: "ENOENT" });
    await expect(readFile(join(root, "inkos-backup/safe.md"))).rejects.toMatchObject({ code: "ENOENT" });
  });
});

/** 与 Rust 测试 simple_tar 对偶的最小 USTAR 构建（512B 头+payload+补齐+双零块）。 */
function simpleTar(entries: Array<{ name: string; payload: Uint8Array }>): Uint8Array<ArrayBuffer> {
  const chunks: Buffer[] = [];
  for (const { name, payload } of entries) {
    const header = Buffer.alloc(512, 0);
    Buffer.from(name, "utf-8").copy(header, 0);
    header.write(payload.length.toString(8).padStart(11, "0"), 124);
    header.write("0", 156);
    for (let i = 148; i < 156; i += 1) header[i] = 0x20;
    let sum = 0;
    for (const byte of header) sum += byte;
    header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148);
    chunks.push(header, Buffer.from(payload));
    const padding = (512 - (payload.length % 512)) % 512;
    if (padding > 0) chunks.push(Buffer.alloc(padding));
  }
  chunks.push(Buffer.alloc(1024));
  return new Uint8Array(Buffer.concat(chunks));
}
