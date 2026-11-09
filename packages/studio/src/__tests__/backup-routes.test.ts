import { readFileSync } from "node:fs";
import { mkdtemp, mkdir, readFile, readdir, rm, stat, symlink, writeFile } from "node:fs/promises";
import { gzipSync } from "node:zlib";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createStudioServer } from "../api/server.js";
import { allocateSnapshotName, isSafeRelativePath, isSnapshotName, listBackupSnapshots } from "../api/backup-snapshot.js";
import { untar } from "../lib/tar-read.js";

/** 955 号：快照名规范形态双端共享向量表（Rust backup_routes.rs tests include_str 同文件）。 */
const snapshotNameVectors = JSON.parse(
  readFileSync(join(fileURLToPath(new URL(".", import.meta.url)), "golden", "backup-snapshot-name-vectors.json"), "utf8"),
) as { accept: string[]; reject: string[]; sortedDesc: { input: string[]; expected: string[] } };

/** 956 号：回滚删除面 introduced 路径安全判定双端共享向量表（Rust include_str 同文件）。 */
const introducedPathVectors = JSON.parse(
  readFileSync(join(fileURLToPath(new URL(".", import.meta.url)), "golden", "backup-introduced-path-vectors.json"), "utf8"),
) as { accept: string[]; reject: string[] };

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
    // 938/939 号：works+五新目录种子（用户创作数据，备份域补缺的断言对象）。
    // 949 号：manifest 补齐 schema 必填面（version/lineage/metadata/时间戳）
    // + source/ 目录——种子成为合法 work，948 打包前 sync 全走成功路径。
    const worksDir = join(root, "works", "w1");
    await mkdir(join(worksDir, "source"), { recursive: true });
    await writeFile(join(worksDir, "work.json"), JSON.stringify({
      version: 2, id: "w1", title: "坊市旧事", profileId: "short-story", language: "zh", status: "draft",
      lineage: [], artifacts: [], metadata: {},
      createdAt: "2026-11-09T00:00:00.000Z", updatedAt: "2026-11-09T00:00:00.000Z",
    }), "utf-8");
    for (const dir of ["translations", "genres", "skills", "radar", "worlds"]) {
      await mkdir(join(root, dir), { recursive: true });
      await writeFile(join(root, dir, "data.json"), JSON.stringify({ seeded: dir }), "utf-8");
    }
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
    expect(names).toContain("inkos-backup/works/w1/work.json"); // 938 号：works 域入备份
    // 939 号：五新目录种子条目全部入包（translations/genres/skills/radar/worlds）。
    for (const dir of ["translations", "genres", "skills", "radar", "worlds"]) {
      expect(names).toContain(`inkos-backup/${dir}/data.json`);
    }
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

  it("export: 打包前 per-work sync 快照补齐——手工放置 source 补登 + 杂散文件/坏 work 不阻断（948/949）", async () => {
    await seedProject();
    // 手工放置形态：source 有文件、manifest 未注册（947 备案的补齐对象）。
    await writeFile(join(root, "works", "w1", "source", "outline.md"), "# 大纲\n\n- 第一幕。\n", "utf-8");
    // Finder .DS_Store 形态杂散文件 + 坏 work 目录——不得阻断 w1 补齐与备份。
    await writeFile(join(root, "works", ".DS_Store"), "junk", "utf-8");
    await mkdir(join(root, "works", "w-broken"), { recursive: true });
    await writeFile(join(root, "works", "w-broken", "work.json"), "{not-json", "utf-8");
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/backup/export");
    expect(res.status).toBe(200);
    // 补齐集成实证：手工放置的 source 文件被补登为 candidate 修订（accept=false
    // 不翻转 accept 面——currentRevisionId 保持空）且快照落盘。
    const manifest = JSON.parse(await readFile(join(root, "works", "w1", "work.json"), "utf-8")) as {
      artifacts: Array<{ currentRevisionId: string | null; revisions: Array<{ status: string; snapshotPath: string }> }>;
    };
    expect(manifest.artifacts).toHaveLength(1);
    expect(manifest.artifacts[0]!.currentRevisionId).toBeNull();
    expect(manifest.artifacts[0]!.revisions[0]!.status).toBe("candidate");
    await expect(stat(join(root, "works", "w1", manifest.artifacts[0]!.revisions[0]!.snapshotPath))).resolves.toBeTruthy();
    // 快照工件入包（works/ 目录整体复制覆盖多版本形态——938 目录级复制设计）。
    const bytes = new Uint8Array(await res.arrayBuffer());
    const { entries } = untar(bytes);
    expect(entries.some((e) => e.name.startsWith("inkos-backup/works/w1/revisions/"))).toBe(true);
  });

  it("export includeSessions=1: sessions/ 入包（940 号参数真化）", async () => {
    await seedProject();
    const sessionsDir = join(root, "sessions", "sess-1");
    await mkdir(sessionsDir, { recursive: true });
    await writeFile(join(sessionsDir, "session.jsonl"), JSON.stringify({ seq: 1, type: "turn" }), "utf-8");
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/backup/export?includeSessions=1");
    expect(res.status).toBe(200);
    const bytes = new Uint8Array(await res.arrayBuffer());
    const { entries, rejected } = untar(bytes);
    expect(rejected).toEqual([]);
    const names = entries.map((e) => e.name);
    expect(names).toContain("inkos-backup/sessions/sess-1/session.jsonl"); // 940 号参数真化
    const manifestEntry = entries.find((e) => e.name === "inkos-backup/backup-manifest.json")!;
    const manifest = JSON.parse(new TextDecoder().decode(manifestEntry.bytes));
    expect(manifest.scope).toEqual({ includeSecrets: false, includeSessions: true });
  });

  it("export 默认（未设 includeSessions）：sessions/ 不入包（默认排除语义保持）", async () => {
    await seedProject();
    const sessionsDir = join(root, "sessions", "sess-1");
    await mkdir(sessionsDir, { recursive: true });
    await writeFile(join(sessionsDir, "session.jsonl"), "{}", "utf-8");
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/backup/export");
    const bytes = new Uint8Array(await res.arrayBuffer());
    const names = untar(bytes).entries.map((e) => e.name);
    expect(names.some((n) => n.startsWith("inkos-backup/sessions/"))).toBe(false); // 默认排除保持
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
    // 955 号：计数面=数据条目（manifest 元数据不计）——954 前 fileCount 含
    // manifest、newFiles 把它报成「新增 1」（真实导出包实际无新文件可写）。
    expect(previewBody.preview.fileCount).toBe(untar(exported).entries.length - 1);
    expect(previewBody.preview.newFiles).toBe(0);
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
    // 955 号：预览总数=确认后的写回数（用户看到的两个数字一致）。
    expect(confirmBody.restored).toBe(previewBody.preview.fileCount);
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
    // 951 号：回滚闭环——pre-restore 快照（951 起附带裸名 manifest）直接
    // 灌回 import 端点，book.json 回到「导入前」状态（=tampered 版本）。
    const rollback = await app.request("/api/v1/backup/import?confirm=1", {
      method: "POST", body: snapshotRaw,
    });
    expect(rollback.status).toBe(200); // 此前快照无 manifest → 400，回滚保险半成品
    const rollbackBody = await rollback.json() as { ok: boolean; restored: number };
    expect(rollbackBody.ok).toBe(true);
    expect(rollbackBody.restored).toBe(snapshotEntries.length - 1); // 全部覆盖文件-manifest
    expect(await readFile(join(root, "books", "b1", "book.json"), "utf-8")).toBe('{"tampered":true}');
  });

  // ── 954 号：快照回滚用户可执行闭环（列举 + 服务端回滚执行）──

  it("snapshots: backups/ 形态过滤列举 + 缺目录=空列表（954）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    // 缺 backups/ = 空列表（不报错）
    const empty = await app.request("/api/v1/backup/snapshots");
    expect(empty.status).toBe(200);
    expect(((await empty.json()) as { snapshots: unknown[] }).snapshots).toEqual([]);
    // import confirm 产快照 + 形态外残留（tmp 半成品/杂散/同名目录）不入列
    const exported = new Uint8Array(
      await (await app.request("/api/v1/backup/export")).arrayBuffer(),
    );
    await writeFile(join(root, "books", "b1", "book.json"), '{"tampered":true}', "utf-8");
    await app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: exported });
    await writeFile(join(root, "backups", ".pre-restore-1.tmp"), "partial", "utf-8");
    await writeFile(join(root, "backups", "other.tar"), "junk", "utf-8");
    await mkdir(join(root, "backups", "pre-restore-999.tar"), { recursive: true });
    const listed = await app.request("/api/v1/backup/snapshots");
    expect(listed.status).toBe(200);
    const { snapshots } = await listed.json() as { snapshots: Array<{ name: string; size: number }> };
    expect(snapshots).toHaveLength(1);
    expect(snapshots[0]!.name).toMatch(/^pre-restore-\d+\.tar$/);
    expect(snapshots[0]!.size).toBeGreaterThan(0);
  });

  it("rollback 两段式: preview 盘点 → confirm 回滚 + 回滚自身留新快照（服务端走 import 同一管道）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    const exported = new Uint8Array(
      await (await app.request("/api/v1/backup/export")).arrayBuffer(),
    );
    await writeFile(join(root, "books", "b1", "book.json"), '{"tampered":true}', "utf-8");
    await app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: exported });
    const backups = await readdir(join(root, "backups"));
    const snapshotName = backups.find((n) => /^pre-restore-\d+\.tar$/.test(n))!;
    // 现网已恢复为导出内容；快照正体=导入前时刻（tampered 版本）。
    // preview 段：服务端读快照走 import 同一执行体（安全三则+manifest 先验生效）。
    const previewRes = await app.request(
      `/api/v1/backup/rollback?name=${encodeURIComponent(snapshotName)}`,
      { method: "POST" },
    );
    expect(previewRes.status).toBe(200);
    const previewBody = await previewRes.json() as { preview: { fileCount: number; overwriting: string[] } };
    expect(previewBody.preview.overwriting).toContain("books/b1/book.json");
    // confirm 段：回滚执行——book.json 回导入前时刻。
    const confirmRes = await app.request(
      `/api/v1/backup/rollback?name=${encodeURIComponent(snapshotName)}&confirm=1`,
      { method: "POST" },
    );
    expect(confirmRes.status).toBe(200);
    const confirmBody = await confirmRes.json() as { ok: boolean; restored: number; snapshot: boolean };
    expect(confirmBody.ok).toBe(true);
    expect(confirmBody.snapshot).toBe(true);
    // restored 派生断言：快照条目=全部被覆盖文件（11 种子文件全存在）+manifest。
    const snapshotEntries = untar(new Uint8Array(await readFile(join(root, "backups", snapshotName)))).entries;
    expect(confirmBody.restored).toBe(snapshotEntries.length - 1);
    expect(await readFile(join(root, "books", "b1", "book.json"), "utf-8")).toBe('{"tampered":true}');
    // 回滚自身也留快照（import 不变式自动保持）：backups/ 现有 2 份。
    const after = await readdir(join(root, "backups"));
    expect(after.filter((n) => /^pre-restore-\d+\.tar$/.test(n))).toHaveLength(2);
  });

  it("rollback 守卫: 非法名/穿越先 400 invalid（先于盘面）+ 合法形态缺失 400 not found", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    const traversal = await app.request("/api/v1/backup/rollback?name=..%2F..%2Fsecrets.json", { method: "POST" });
    expect(traversal.status).toBe(400);
    expect(((await traversal.json()) as { error: string }).error).toBe("invalid snapshot name");
    const missing = await app.request(
      `/api/v1/backup/rollback?name=${encodeURIComponent("pre-restore-1731100000000.tar")}`,
      { method: "POST" },
    );
    expect(missing.status).toBe(400);
    expect(((await missing.json()) as { error: string }).error).toBe("snapshot not found");
  });

  // ── 955 号：954 新增代码审查修复（命名碰撞/并发串行/形态对齐/回滚面=列举面）──

  const snapshotBook = async (name: string): Promise<string | undefined> => {
    const entries = untar(new Uint8Array(await readFile(join(root, "backups", name)))).entries;
    const book = entries.find((entry) => entry.name === "books/b1/book.json");
    return book ? new TextDecoder().decode(book.bytes) : undefined;
  };

  it("快照名形态+排序: 双端共享 golden 向量（955——954 版安全整数判定与 Rust u64 在 16-19 位名上分叉）", async () => {
    for (const name of snapshotNameVectors.accept) expect(isSnapshotName(name), name).toBe(true);
    for (const name of snapshotNameVectors.reject) expect(isSnapshotName(name), JSON.stringify(name)).toBe(false);
    // 排序向量：落盘为常规文件后经列举=数值倒序（954 版纯字典序把 pre-restore-9
    // 排在 13 位新快照之前）。
    await mkdir(join(root, "backups"), { recursive: true });
    for (const name of snapshotNameVectors.sortedDesc.input) {
      await writeFile(join(root, "backups", name), "x", "utf-8");
    }
    const listed = (await listBackupSnapshots(root)).map((item) => item.name);
    expect(listed).toEqual(snapshotNameVectors.sortedDesc.expected);
  });

  it("快照唯一名分配: 已占用毫秒名（含同名目录）顺延探测（955）", async () => {
    const backupsDir = join(root, "backups");
    await mkdir(backupsDir, { recursive: true });
    expect(await allocateSnapshotName(backupsDir, 1731100000000)).toBe("pre-restore-1731100000000.tar");
    await writeFile(join(backupsDir, "pre-restore-1731100000000.tar"), "a", "utf-8");
    // 同名目录同样视为占用（rename 到目录会失败，不能当空位）。
    await mkdir(join(backupsDir, "pre-restore-1731100000001.tar"));
    expect(await allocateSnapshotName(backupsDir, 1731100000000)).toBe("pre-restore-1731100000002.tar");
  });

  it("同毫秒二次恢复: 两份快照并存且首份不被覆盖（955——954 版 writeFile 覆盖=回滚保险静默丢失）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    const originalBook = await readFile(join(root, "books", "b1", "book.json"), "utf-8");
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(1731100000000);
    try {
      for (const marker of ["A", "B"]) {
        const res = await app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: markerPackage(marker) });
        expect(res.status).toBe(200);
      }
    } finally {
      nowSpy.mockRestore();
    }
    const listed = (await listBackupSnapshots(root)).map((item) => item.name);
    expect(listed).toEqual(["pre-restore-1731100000001.tar", "pre-restore-1731100000000.tar"]);
    // 首份=首次恢复前（原件），次份=二次恢复前（A 的恢复结果）。
    expect(await snapshotBook("pre-restore-1731100000000.tar")).toBe(originalBook);
    expect(await snapshotBook("pre-restore-1731100000001.tar")).toBe('{"marker":"A"}');
  });

  it("并发两次确认恢复: 串行锁保证终态为某一包整体、两份快照并存且后者=先执行者恢复结果（955）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    const originalBook = await readFile(join(root, "books", "b1", "book.json"), "utf-8");
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(1731100000000);
    try {
      const results = await Promise.all(["A", "B"].map((marker) =>
        app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: markerPackage(marker) })));
      expect(results.map((res) => res.status)).toEqual([200, 200]);
    } finally {
      nowSpy.mockRestore();
    }
    const book = await readFile(join(root, "books", "b1", "book.json"), "utf-8");
    const winner = book.includes('"A"') ? "A" : "B";
    const loser = winner === "A" ? "B" : "A";
    // 不交错：两文件同出一包。
    expect(await readFile(join(root, "inkos.json"), "utf-8")).toBe(`{"name":"${winner}"}`);
    const listed = (await listBackupSnapshots(root)).map((item) => item.name);
    expect(listed).toHaveLength(2);
    expect(await snapshotBook(listed[1]!)).toBe(originalBook);
    expect(await snapshotBook(listed[0]!)).toBe(`{"marker":"${loser}"}`);
    // 原子写零残留：快照与写回目标目录无 writeTextAtomic 临时文件。
    for (const dir of [join(root, "backups"), join(root, "books", "b1")]) {
      expect((await readdir(dir)).filter((name) => name.includes(".tmp-"))).toEqual([]);
    }
  });

  it.skipIf(process.platform === "win32")("回滚面=列举面: 列举不可见的符号链接名不得被直接回滚（955——954 版 readFile 跟随链接）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    await app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: markerPackage("A") });
    const [real] = (await listBackupSnapshots(root)).map((item) => item.name);
    await symlink(join(root, "backups", real!), join(root, "backups", "pre-restore-1.tar"));
    const listed = await app.request("/api/v1/backup/snapshots");
    const listedNames = ((await listed.json()) as { snapshots: Array<{ name: string }> }).snapshots.map((s) => s.name);
    expect(listedNames).toEqual([real]);
    const viaLink = await app.request("/api/v1/backup/rollback?name=pre-restore-1.tar", { method: "POST" });
    expect(viaLink.status).toBe(400);
    expect(((await viaLink.json()) as { error: string }).error).toBe("snapshot not found");
    // 真快照本体照常可回滚（守卫不误伤）。
    const viaReal = await app.request(`/api/v1/backup/rollback?name=${encodeURIComponent(real!)}`, { method: "POST" });
    expect(viaReal.status).toBe(200);
  });

  it("根目录同名 backup-manifest.json: 不入覆盖面不入快照、快照仍可回滚（955——954 前快照内双 manifest 致回滚先验失败）", async () => {
    await seedProject();
    const userNote = JSON.stringify({ note: "用户自己的清单笔记" });
    await writeFile(join(root, "backup-manifest.json"), userNote, "utf-8");
    const originalBook = await readFile(join(root, "books", "b1", "book.json"), "utf-8");
    const app = createStudioServer({} as never, root);
    const preview = await app.request("/api/v1/backup/import", { method: "POST", body: markerPackage("A") });
    const previewBody = await preview.json() as { preview: { fileCount: number; overwriting: string[]; newFiles: number } };
    expect(previewBody.preview).toEqual({ fileCount: 2, overwriting: ["books/b1/book.json", "inkos.json"], newFiles: 0 });
    await app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: markerPackage("A") });
    expect(await readFile(join(root, "backup-manifest.json"), "utf-8")).toBe(userNote);
    const [snapshot] = (await listBackupSnapshots(root)).map((item) => item.name);
    const snapshotEntries = untar(new Uint8Array(await readFile(join(root, "backups", snapshot!)))).entries;
    expect(snapshotEntries.filter((entry) => entry.name === "backup-manifest.json")).toHaveLength(1);
    // 快照可回滚：book.json 回到恢复前原件，用户笔记原样保留。
    const rolled = await app.request(`/api/v1/backup/rollback?name=${encodeURIComponent(snapshot!)}&confirm=1`, { method: "POST" });
    expect(rolled.status).toBe(200);
    expect(await readFile(join(root, "books", "b1", "book.json"), "utf-8")).toBe(originalBook);
    expect(await readFile(join(root, "backup-manifest.json"), "utf-8")).toBe(userNote);
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

  // —— 950 号：跨端备份包互灌（长路径三通道端到端）——

  it("export 长路径回灌: >100 字节路径经 prefix/LongLink 通道入包并恢复落盘（此前 export 抛错）", async () => {
    await seedProject();
    // 真实形态长路径：works/<长id>/revisions/<长rev>/...（>100 字节）。
    const workId = "w-12345678-1234-5678-1234-567812345678";
    const revDir = join(root, "works", workId, "revisions", "r-87654321-4321-8765-4321-876543218765");
    await mkdir(revDir, { recursive: true });
    const longRel = join("works", workId, "revisions", "r-87654321-4321-8765-4321-876543218765", "chapter-0001-r2.json");
    expect(longRel.length).toBeGreaterThan(100);
    await writeFile(join(revDir, "chapter-0001-r2.json"), JSON.stringify({ rev: "long-path-body" }), "utf-8");
    // >255 字节路径（prefix 拆分不可达）→ GNU LongLink 'L' 写法通道。
    const deepId = `w-${"d".repeat(250)}`;
    const deepDir = join(root, "works", deepId, "revisions", "r1");
    await mkdir(deepDir, { recursive: true });
    const deepRel = join("works", deepId, "revisions", "r1", "chapter-0001-r9.json");
    expect(deepRel.length).toBeGreaterThan(255);
    await writeFile(join(deepDir, "chapter-0001-r9.json"), JSON.stringify({ rev: "longlink-body" }), "utf-8");

    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/backup/export");
    expect(res.status).toBe(200); // 此前 >100 字节路径 writeTarString 抛错=export 500
    const exported = new Uint8Array(await res.arrayBuffer());
    const { entries, rejected } = untar(exported);
    expect(rejected).toEqual([]);
    // prefix 通道（≤255 有边界）：完整名恢复。
    const fullName = `inkos-backup/${longRel.replace(/\\/g, "/")}`;
    const longEntry = entries.find((e) => e.name === fullName);
    expect(longEntry).toBeDefined();
    expect(JSON.parse(new TextDecoder().decode(longEntry!.bytes))).toEqual({ rev: "long-path-body" });
    // 'L' 通道（>255 或无边界）：完整名恢复（GNU 扩展写法，Rust 读侧同形态）。
    const deepName = `inkos-backup/${deepRel.replace(/\\/g, "/")}`;
    const deepEntry = entries.find((e) => e.name === deepName);
    expect(deepEntry).toBeDefined();
    expect(JSON.parse(new TextDecoder().decode(deepEntry!.bytes))).toEqual({ rev: "longlink-body" });

    // 回灌到另一个项目根：写回后长路径文件原样落盘。
    const root2 = await mkdtemp(join(tmpdir(), "backup-restore-"));
    try {
      const app2 = createStudioServer({} as never, root2);
      const confirmed = await app2.request("/api/v1/backup/import?confirm=1", {
        method: "POST", body: exported,
      });
      expect(confirmed.status).toBe(200);
      const body = await confirmed.json() as { ok: boolean; restored: number };
      expect(body.ok).toBe(true);
      expect(body.restored).toBe(entries.length - 1);
      expect(JSON.parse(await readFile(join(root2, longRel), "utf-8"))).toEqual({ rev: "long-path-body" });
      expect(JSON.parse(await readFile(join(root2, deepRel), "utf-8"))).toEqual({ rev: "longlink-body" });
    } finally {
      await rm(root2, { recursive: true, force: true });
    }
  });

  it("import 接受 Rust 形态 GNU LongLink gzip 包（此前 'L' 条目 rejected 整包 400）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    // 模拟 Rust 导出侧（tar::Builder + Header::new_gnu）长路径产出：
    // 'L' 扩展条目 + 截断名真条目 + manifest，gzip 层与 Rust GzEncoder 同。
    const longName = "inkos-backup/works/w-12345678-1234-5678-1234-567812345678/revisions/r-12345678-1234-5678-1234-567812345678/chapter-0001-r2.json";
    const tar = simpleTar([
      { name: "inkos-backup/backup-manifest.json", payload: new TextEncoder().encode(JSON.stringify({ version: 1, generator: "inkos-desktop" })) },
      { name: "././@LongLink", payload: new TextEncoder().encode(`${longName}\0`), typeflag: "L" },
      { name: longName.slice(0, 100), payload: new TextEncoder().encode('{"rev":"from-rust-gnu"}') },
    ]);
    const res = await app.request("/api/v1/backup/import?confirm=1", {
      method: "POST", body: new Uint8Array(gzipSync(tar)),
    });
    expect(res.status).toBe(200); // 此前 'L' 进 rejected → unsafe entries 400
    const body = await res.json() as { ok: boolean; restored: number };
    expect(body.ok).toBe(true);
    expect(body.restored).toBe(1); // manifest 不计；'L' 扩展条目不产出文件条目
    const restored = JSON.parse(await readFile(join(root, longName.slice("inkos-backup/".length)), "utf-8"));
    expect(restored).toEqual({ rev: "from-rust-gnu" });
  });

  it("import 接受 Rust 形态 pre-restore 快照回滚（GNU 头裸名条目+manifest）——跨端快照回滚", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    // Rust 快照形态（951 起）：Header::new_gnu + 裸名条目 + 裸名 manifest。
    // 场景：Rust 引擎恢复留档 → Node 回退端回滚。
    const tar = simpleTar([
      { name: "backup-manifest.json", payload: new TextEncoder().encode(JSON.stringify({ version: 1, generator: "inkos-desktop", kind: "pre-restore-snapshot" })), gnuMagic: true },
      { name: "books/b1/book.json", payload: new TextEncoder().encode('{"rolled-back":true}'), gnuMagic: true },
    ]);
    const res = await app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: tar });
    expect(res.status).toBe(200);
    const body = await res.json() as { ok: boolean; restored: number };
    expect(body.ok).toBe(true);
    expect(body.restored).toBe(1); // manifest 不计
    expect(JSON.parse(await readFile(join(root, "books", "b1", "book.json"), "utf-8"))).toEqual({ "rolled-back": true });
  });

  it("快照长名回滚回归: 长路径 overwriting 的快照条目名不截断（953 审查修复——修复前 Buffer.copy 静默截断致回滚写错位置）", async () => {
    await seedProject();
    const workId = "w-12345678-1234-5678-1234-567812345678";
    const longRel = join("works", workId, "revisions", "r-87654321-4321-8765-4321-876543218765", "chapter-0001-r2.json");
    expect(longRel.length).toBeGreaterThan(100);
    const longPath = join(root, longRel);
    await mkdir(join(longPath, ".."), { recursive: true });
    await writeFile(longPath, '{"v":1}', "utf-8");

    const app = createStudioServer({} as never, root);
    // 导入含长路径条目的包（GNU 'L' 形态=模拟 Rust 导出）→ 覆盖现网 v1，
    // 快照必须以完整长名留档 v1（950 前 export 抛错/手写快照截断名）。
    const longName = `inkos-backup/${longRel.replace(/\\/g, "/")}`;
    const pack = simpleTar([
      { name: "inkos-backup/backup-manifest.json", payload: new TextEncoder().encode(JSON.stringify({ version: 1, generator: "inkos-desktop" })), gnuMagic: true },
      { name: "././@LongLink", payload: new TextEncoder().encode(`${longName}\0`), typeflag: "L", gnuMagic: true },
      { name: longName.slice(0, 100), payload: new TextEncoder().encode('{"v":2}'), gnuMagic: true },
    ]);
    const res1 = await app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: pack });
    expect(res1.status).toBe(200);
    expect(JSON.parse(await readFile(longPath, "utf-8"))).toEqual({ v: 2 });

    // 快照条目名=完整长名（修复前：前 100 字节截断名）。
    const backups = await readdir(join(root, "backups"));
    const snapshotRaw = await readFile(join(root, "backups", backups[0]!));
    const snapNames = untar(new Uint8Array(snapshotRaw)).entries.map((e) => e.name);
    expect(snapNames).toContain(longName.slice("inkos-backup/".length));

    // 回滚 → 长路径文件回到快照留档的 v1（修复前：回滚写到截断名路径，
    // 长路径文件停留 v2）。
    const res2 = await app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: snapshotRaw });
    expect(res2.status).toBe(200);
    expect(JSON.parse(await readFile(longPath, "utf-8"))).toEqual({ v: 1 });
  });

  // ── 956 号：完全回滚语义（快照 manifest 决策面 + introduced 可选删除）──

  /** 956 号：真流恢复包（覆盖 books/b1/book.json + 新增 notes/new-file.md）。 */
  const restorePack = (overwrittenPayload: string, introducedPayload: string): Uint8Array<ArrayBuffer> => {
    const encode = (text: string) => new TextEncoder().encode(text);
    return simpleTar([
      { name: "inkos-backup/backup-manifest.json", payload: encode(JSON.stringify({ version: 1, generator: "inkos-desktop" })) },
      { name: "inkos-backup/books/b1/book.json", payload: encode(overwrittenPayload) },
      { name: "inkos-backup/notes/new-file.md", payload: encode(introducedPayload) },
    ]);
  };
  /** 956 号：读快照内 manifest（裸名条目）。 */
  const readSnapshotManifest = async (name: string): Promise<Record<string, unknown>> => {
    const raw = await readFile(join(root, "backups", name));
    const entries = untar(new Uint8Array(raw)).entries;
    const manifestEntry = entries.find((e) => e.name === "backup-manifest.json")!;
    return JSON.parse(new TextDecoder().decode(manifestEntry.bytes)) as Record<string, unknown>;
  };
  const listSnapshots = async (app: ReturnType<typeof createStudioServer>): Promise<string[]> => {
    const res = await app.request("/api/v1/backup/snapshots");
    const body = await res.json() as { snapshots: Array<{ name: string }> };
    return body.snapshots.map((item) => item.name);
  };

  it("快照 manifest 决策面: overwriting/introduced 名单精确，新文件不入快照数据条目（956）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/backup/import?confirm=1", {
      method: "POST", body: restorePack('{"restored":true}', "introduced content"),
    });
    expect(res.status).toBe(200);
    const confirmBody = await res.json() as { snapshot: boolean };
    expect(confirmBody.snapshot).toBe(true);
    const [snapshot] = await listSnapshots(app);
    const raw = untar(new Uint8Array(await readFile(join(root, "backups", snapshot!))));
    const names = raw.entries.map((e) => e.name);
    expect(names).toContain("books/b1/book.json");
    expect(names).not.toContain("notes/new-file.md"); // 保守回滚不删它，不入快照
    const manifest = await readSnapshotManifest(snapshot!);
    expect(manifest.kind).toBe("pre-restore-snapshot");
    expect(manifest.overwriting).toEqual(["books/b1/book.json"]);
    expect(manifest.introduced).toEqual(["notes/new-file.md"]);
  });

  it("纯新增恢复也留快照: manifest 记 introduced 名单（956——此前 overwriting 空=无快照=纯新增无回滚保险）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    const encode = (text: string) => new TextEncoder().encode(text);
    const pack = simpleTar([
      { name: "inkos-backup/backup-manifest.json", payload: encode(JSON.stringify({ version: 1, generator: "inkos-desktop" })) },
      { name: "inkos-backup/notes/only-new.md", payload: encode("only new") },
    ]);
    const res = await app.request("/api/v1/backup/import?confirm=1", { method: "POST", body: pack });
    const confirmBody = await res.json() as { snapshot: boolean; restored: number };
    expect(confirmBody.snapshot).toBe(true);
    expect(confirmBody.restored).toBe(1);
    const [snapshot] = await listSnapshots(app);
    const entries = untar(new Uint8Array(await readFile(join(root, "backups", snapshot!)))).entries;
    expect(entries).toHaveLength(1); // 唯一条目=自身 manifest（无被覆盖文件可留档）
    expect(entries[0]!.name).toBe("backup-manifest.json");
    const manifest = JSON.parse(new TextDecoder().decode(entries[0]!.bytes)) as Record<string, unknown>;
    expect(manifest.introduced).toEqual(["notes/only-new.md"]);
  });

  it("回滚预览透传 introduced 清单（取自快照 manifest 非盘面推断）；import 端点同字节永不透传（956）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    await app.request("/api/v1/backup/import?confirm=1", {
      method: "POST", body: restorePack('{"v2":true}', "new content"),
    });
    const [snapshot] = await listSnapshots(app);
    await mkdir(join(root, "notes"), { recursive: true });
    await writeFile(join(root, "notes", "new-file.md"), "mutated", "utf-8"); // 现网改文件
    const previewRes = await app.request(
      `/api/v1/backup/rollback?name=${encodeURIComponent(snapshot!)}`, { method: "POST" },
    );
    expect(previewRes.status).toBe(200);
    const preview = await previewRes.json() as { preview: { introduced?: string[] } };
    expect(preview.preview.introduced).toEqual(["notes/new-file.md"]);
    // 同一快照字节走 import（Ignore 模式）——无 introduced 键。
    const bytes = await readFile(join(root, "backups", snapshot!));
    const importRes = await app.request("/api/v1/backup/import", { method: "POST", body: new Uint8Array(bytes) });
    const importPreview = await importRes.json() as { preview: { introduced?: string[] } };
    expect(importPreview.preview.introduced).toBeUndefined();
  });

  it("完全回滚: 写回+删除 introduced+回滚自身保险快照自洽，保守回滚保险快照复原待删文件（956）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    const original = await readFile(join(root, "books", "b1", "book.json"), "utf-8");
    await app.request("/api/v1/backup/import?confirm=1", {
      method: "POST", body: restorePack('{"v2":true}', "introduced-v2"),
    });
    const [snapshot] = await listSnapshots(app);
    await mkdir(join(root, "notes"), { recursive: true });
    await writeFile(join(root, "notes", "new-file.md"), "mutated-before-full-rollback", "utf-8");

    const doneRes = await app.request(
      `/api/v1/backup/rollback?name=${encodeURIComponent(snapshot!)}&confirm=1&deleteIntroduced=1`,
      { method: "POST" },
    );
    expect(doneRes.status).toBe(200);
    const done = await doneRes.json() as {
      ok: boolean; restored: number; snapshot: boolean; removed: number;
      deleteFailed?: Array<{ path: string; error: string }>;
    };
    expect(done.ok).toBe(true);
    expect(done.restored).toBe(1);
    expect(done.snapshot).toBe(true);
    expect(done.removed).toBe(1);
    expect(done.deleteFailed).toBeUndefined();
    expect(await readFile(join(root, "books", "b1", "book.json"), "utf-8")).toBe(original);
    await expect(stat(join(root, "notes", "new-file.md"))).rejects.toMatchObject({ code: "ENOENT" });

    // 回滚自身保险快照（第 2 份）自洽：数据条目=被覆盖+待删删除前内容；
    // manifest 撤销面=overwriting 被覆盖/introduced 空/removing 被删。
    const names = await listSnapshots(app);
    expect(names).toHaveLength(2);
    const guard = names[0]!; // 数值倒序首位=本次回滚留档
    const guardEntries = untar(new Uint8Array(await readFile(join(root, "backups", guard)))).entries;
    const dataNames = guardEntries.filter((e) => e.name !== "backup-manifest.json").map((e) => e.name);
    expect(dataNames).toEqual(["books/b1/book.json", "notes/new-file.md"]);
    const guardManifest = await readSnapshotManifest(guard);
    expect(guardManifest.overwriting).toEqual(["books/b1/book.json"]);
    expect(guardManifest.introduced).toEqual([]);
    expect(guardManifest.removing).toEqual(["notes/new-file.md"]);
    // 保险闭环：保守回滚保险快照 → 待删文件复原（内容=删除前时刻）。
    const undoneRes = await app.request(
      `/api/v1/backup/rollback?name=${encodeURIComponent(guard)}&confirm=1`, { method: "POST" },
    );
    const undone = await undoneRes.json() as { restored: number };
    expect(undone.restored).toBe(2);
    expect(await readFile(join(root, "notes", "new-file.md"), "utf-8")).toBe("mutated-before-full-rollback");
  });

  it("保守回滚语义不变: 不带 deleteIntroduced 时 introduced 保留、响应无 removed 键（956）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    await app.request("/api/v1/backup/import?confirm=1", {
      method: "POST", body: restorePack('{"v2":true}', "new content"),
    });
    const [snapshot] = await listSnapshots(app);
    await mkdir(join(root, "notes"), { recursive: true });
    await writeFile(join(root, "notes", "new-file.md"), "new content", "utf-8");
    const doneRes = await app.request(
      `/api/v1/backup/rollback?name=${encodeURIComponent(snapshot!)}&confirm=1`, { method: "POST" },
    );
    const done = await doneRes.json() as { restored: number; removed?: number };
    expect(done.restored).toBe(1);
    expect(done.removed).toBeUndefined();
    await expect(stat(join(root, "notes", "new-file.md"))).resolves.toBeTruthy();
  });

  it("955- 旧形态快照 + deleteIntroduced=1 → 400 lacks（先验于盘面零变更）；同快照保守回滚照常（向后兼容）（956）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    await mkdir(join(root, "backups"), { recursive: true });
    const encode = (text: string) => new TextEncoder().encode(text);
    const legacy = simpleTar([
      { name: "backup-manifest.json", payload: encode(JSON.stringify({ version: 1, generator: "inkos-desktop", kind: "pre-restore-snapshot" })) },
      { name: "books/b1/book.json", payload: encode('{"legacy":true}') },
    ]);
    await writeFile(join(root, "backups", "pre-restore-1731100000000.tar"), legacy);
    const before = await readFile(join(root, "books", "b1", "book.json"), "utf-8");
    const errRes = await app.request(
      "/api/v1/backup/rollback?name=pre-restore-1731100000000.tar&confirm=1&deleteIntroduced=1",
      { method: "POST" },
    );
    expect(errRes.status).toBe(400);
    expect(((await errRes.json()) as { error: string }).error).toContain("lacks introduced list");
    expect(await readFile(join(root, "books", "b1", "book.json"), "utf-8")).toBe(before);
    expect(await listSnapshots(app)).toEqual(["pre-restore-1731100000000.tar"]); // 无新增快照
    // 向后兼容：同快照保守回滚成功。
    const doneRes = await app.request(
      "/api/v1/backup/rollback?name=pre-restore-1731100000000.tar&confirm=1", { method: "POST" },
    );
    expect(((await doneRes.json()) as { restored: number }).restored).toBe(1);
  });

  it("introduced 路径安全判定=双端共享 golden 向量；篡改快照 delete 模式 preview/confirm 同一先验 400、keep 宽容（956）", async () => {
    for (const path of introducedPathVectors.accept) {
      expect(isSafeRelativePath(path), `accept: ${path}`).toBe(true);
    }
    for (const path of introducedPathVectors.reject) {
      expect(isSafeRelativePath(path), `reject: ${path}`).toBe(false);
    }
    await seedProject();
    const app = createStudioServer({} as never, root);
    await mkdir(join(root, "backups"), { recursive: true });
    const encode = (text: string) => new TextEncoder().encode(text);
    const evil = simpleTar([
      {
        name: "backup-manifest.json",
        payload: encode(JSON.stringify({
          version: 1, generator: "inkos-desktop", kind: "pre-restore-snapshot",
          introduced: ["../../../outside/secret.txt"],
        })),
      },
      { name: "books/b1/book.json", payload: encode('{"evil":true}') },
    ]);
    await writeFile(join(root, "backups", "pre-restore-1731200000000.tar"), evil);
    const before = await readFile(join(root, "books", "b1", "book.json"), "utf-8");
    // delete 模式 preview=confirm 同一先验（预览预言确认结果）。
    const previewRes = await app.request(
      "/api/v1/backup/rollback?name=pre-restore-1731200000000.tar&deleteIntroduced=1", { method: "POST" },
    );
    expect(previewRes.status).toBe(400);
    expect(((await previewRes.json()) as { error: string }).error).toContain("unsafe introduced path");
    const confirmRes = await app.request(
      "/api/v1/backup/rollback?name=pre-restore-1731200000000.tar&confirm=1&deleteIntroduced=1",
      { method: "POST" },
    );
    expect(confirmRes.status).toBe(400);
    expect(((await confirmRes.json()) as { error: string }).error).toContain("unsafe introduced path");
    expect(await readFile(join(root, "books", "b1", "book.json"), "utf-8")).toBe(before);
    expect(await listSnapshots(app)).toEqual(["pre-restore-1731200000000.tar"]); // 无新增快照/tmp
    // keep 模式：宽容展示（200 无 introduced 键），confirm 不删。
    const keepPreview = await app.request(
      "/api/v1/backup/rollback?name=pre-restore-1731200000000.tar", { method: "POST" },
    );
    expect(((await keepPreview.json()) as { preview: { introduced?: string[] } }).preview.introduced).toBeUndefined();
    const keepConfirm = await app.request(
      "/api/v1/backup/rollback?name=pre-restore-1731200000000.tar&confirm=1", { method: "POST" },
    );
    const keepBody = await keepConfirm.json() as { restored: number; removed?: number };
    expect(keepBody.restored).toBe(1);
    expect(keepBody.removed).toBeUndefined();
  });

  it("待删文件在操作前已被外部删除=终态已达成：removed=0（本次未删，诚实计数）、无 deleteFailed、不留快照（956）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    // 纯新增包（与 Rust 同名用例同形）：回滚时无覆盖面，删除面终态已达成。
    await app.request("/api/v1/backup/import?confirm=1", {
      method: "POST", body: simpleTar([
        { name: "inkos-backup/backup-manifest.json", payload: new TextEncoder().encode(JSON.stringify({ version: 1, generator: "inkos-desktop" })) },
        { name: "inkos-backup/notes/new-file.md", payload: new TextEncoder().encode("new content") },
      ]),
    });
    const [snapshot] = await listSnapshots(app);
    await mkdir(join(root, "notes"), { recursive: true });
    await writeFile(join(root, "notes", "new-file.md"), "new content", "utf-8");
    await rm(join(root, "notes", "new-file.md")); // 外部删除
    const doneRes = await app.request(
      `/api/v1/backup/rollback?name=${encodeURIComponent(snapshot!)}&confirm=1&deleteIntroduced=1`,
      { method: "POST" },
    );
    const done = await doneRes.json() as { removed: number; deleteFailed?: unknown[]; snapshot: boolean };
    expect(done.removed).toBe(0); // 终态已达成=本次未删任何文件（诚实计数）
    expect(done.deleteFailed).toBeUndefined();
    expect(done.snapshot).toBe(false); // 无覆盖/新建/删除=不留快照
    expect(await listSnapshots(app)).toHaveLength(1);
  });

  it.skipIf(process.platform === "win32")("待删名字为符号链接: 不跟随不删、deleteFailed 明示、目标不动（956 删除面=常规文件）", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);
    await app.request("/api/v1/backup/import?confirm=1", {
      method: "POST", body: simpleTar([
        { name: "inkos-backup/backup-manifest.json", payload: new TextEncoder().encode(JSON.stringify({ version: 1, generator: "inkos-desktop" })) },
        { name: "inkos-backup/notes/link.md", payload: new TextEncoder().encode("placeholder") },
      ]),
    });
    const [snapshot] = await listSnapshots(app);
    await mkdir(join(root, "notes"), { recursive: true });
    const target = join(root, "notes", "real-target.txt");
    await writeFile(target, "keep me", "utf-8");
    await rm(join(root, "notes", "link.md")); // 恢复写出的常规文件
    await symlink(target, join(root, "notes", "link.md"));
    const doneRes = await app.request(
      `/api/v1/backup/rollback?name=${encodeURIComponent(snapshot!)}&confirm=1&deleteIntroduced=1`,
      { method: "POST" },
    );
    const done = await doneRes.json() as {
      removed: number; deleteFailed: Array<{ path: string; error: string }>;
    };
    expect(done.removed).toBe(0);
    expect(done.deleteFailed).toEqual([{ path: "notes/link.md", error: "not a regular file" }]);
    await expect(stat(join(root, "notes", "link.md"))).resolves.toBeTruthy(); // 链接本身保留
    expect(await readFile(target, "utf-8")).toBe("keep me"); // 目标不动
  });
});

/** 955 号：标记包（manifest+book.json+inkos.json 同标记）——同毫秒/并发恢复用例的可辨识载荷。 */
function markerPackage(marker: string): Uint8Array<ArrayBuffer> {
  const encode = (text: string) => new TextEncoder().encode(text);
  return simpleTar([
    { name: "inkos-backup/backup-manifest.json", payload: encode(JSON.stringify({ version: 1, generator: "inkos-desktop" })) },
    { name: "inkos-backup/books/b1/book.json", payload: encode(`{"marker":"${marker}"}`) },
    { name: "inkos-backup/inkos.json", payload: encode(`{"name":"${marker}"}`) },
  ]);
}

/** 与 Rust 测试 simple_tar 对偶的最小 USTAR 构建（512B 头+payload+补齐+双零块）。 */
function simpleTar(entries: Array<{ name: string; payload: Uint8Array; typeflag?: string; gnuMagic?: boolean }>): Uint8Array<ArrayBuffer> {
  const chunks: Buffer[] = [];
  for (const { name, payload, typeflag, gnuMagic } of entries) {
    const header = Buffer.alloc(512, 0);
    Buffer.from(name, "utf-8").copy(header, 0);
    header.write(payload.length.toString(8).padStart(11, "0"), 124);
    header.write(typeflag ?? "0", 156);
    // gnuMagic：Rust Header::new_gnu 形态（"ustar "+" \0"）；缺省=TS 快照
    // 构造形态（257 区全零，v7/基础头）。
    if (gnuMagic) {
      header.write("ustar ", 257, "utf-8");
      header.write(" \0", 263);
    }
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
