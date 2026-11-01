import { mkdtemp, mkdir, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it } from "vitest";
import { StateManager } from "../state/manager.js";

// 807 号：删章口径与索引重建同集——legacy 连字符旧章（`NNNN-标题.md`）与裸章号
// （`NNNN.md`）必须随目标章之后的章节一起删除，否则被空索引触发的宽容重建捞回
// =「reject 后章节复活」（rust 对偶 engine-rs manager.rs 同病同修）。
it("rollbackToChapter deletes legacy hyphen and bare-numbered chapter files alongside canonical ones", async () => {
  const root = await mkdtemp(join(tmpdir(), "inkos-rollback-legacy-"));
  try {
    const manager = new StateManager(root);
    const bookDir = manager.bookDir("legacy");
    const chaptersDir = join(bookDir, "chapters");
    const snapshotDir = join(bookDir, "story", "snapshots", "0");
    await mkdir(chaptersDir, { recursive: true });
    await mkdir(join(snapshotDir, "state"), { recursive: true });
    await writeFile(join(chaptersDir, "0001-镜中醒来.md"), "legacy");
    await writeFile(join(chaptersDir, "0002_续章.md"), "canonical");
    await writeFile(join(chaptersDir, "0003.md"), "bare");
    await writeFile(join(snapshotDir, "current_state.md"), "# state");
    await writeFile(join(snapshotDir, "pending_hooks.md"), "# hooks");
    await writeFile(
      join(snapshotDir, "state", "manifest.json"),
      JSON.stringify({ schemaVersion: 2, language: "zh", lastAppliedChapter: 0, projectionVersion: 1 }),
    );

    // 索引缺失 → 宽容重建把三种命名全部视为章节（读侧口径）。
    const before = (await manager.loadChapterIndex("legacy")).map((meta) => meta.number).sort();
    expect(before).toEqual([1, 2, 3]);

    const discarded = [...(await manager.rollbackToChapter("legacy", 0))].sort();
    expect(discarded).toEqual([1, 2, 3]);
    const remaining = (await readdir(chaptersDir)).filter((name) => name.endsWith(".md"));
    expect(remaining).toEqual([]);
    // 文件已清空 → 索引为空且不再被重建捞回。
    expect(await manager.loadChapterIndex("legacy")).toEqual([]);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
