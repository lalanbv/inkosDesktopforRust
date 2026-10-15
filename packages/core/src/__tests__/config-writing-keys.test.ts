import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { BookConfigSchema, ProjectConfigSchema, StateManager } from "../index.js";

/**
 * 709 号：写键自由面回归锁——Rust BookConfig（engine-rs models/book.rs）与
 * Rust WritingConfig（models/project.rs）镜像的可选键（writing/seriesId/series/
 * governance / project writing）复位进 strict schema。此前 best-of-n（701）/
 * auto-beats、series-id（707）/Rust 引擎（review-mode PUT）把这些键落盘后，
 * strict 根拒收使 loadBookConfig/loadProjectConfig ZodError——整本书/项目在
 * TS 回退腿不可加载的潜伏自毁键。本套件锁定：写面落盘键在位时双加载器不崩。
 */
describe("book/project config writing keys tolerance (709)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "config-writing-keys-"));
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const baseBook = {
    id: "b1",
    title: "镜花缘",
    platform: "qidian",
    genre: "xuanhuan",
    status: "active",
    targetChapters: 12,
    chapterWordCount: 3000,
    language: "zh",
    createdAt: "2026-10-16T00:00:00.000Z",
    updatedAt: "2026-10-16T00:00:00.000Z",
  };

  it("BookConfigSchema：writing/seriesId/series/governance 可选键 roundtrip（Rust BookConfig 镜像）", () => {
    const full = BookConfigSchema.parse({
      ...baseBook,
      seriesId: "jing_hua_yuan",
      series: { name: "镜花缘系列", order: 2 },
      writing: { reviewMode: "manual", revisionGate: "strict", autoTimelineBeats: true, sceneBeats: false },
      governance: { policy: "quality-first", maxConsecutiveDebts: 3, bestOfN: { enabled: true, candidates: 3, minScore: 80 } },
    });
    expect(full.writing?.reviewMode).toBe("manual");
    expect(full.writing?.autoTimelineBeats).toBe(true);
    expect(full.seriesId).toBe("jing_hua_yuan");
    expect(full.series?.order).toBe(2);
    expect(full.governance?.bestOfN?.minScore).toBe(80);

    // 非法子值仍拒：reviewMode 三态外值、candidates 越界、maxConsecutiveDebts 越界
    expect(BookConfigSchema.safeParse({ ...baseBook, writing: { reviewMode: "yolo" } }).success).toBe(false);
    expect(BookConfigSchema.safeParse({ ...baseBook, governance: { bestOfN: { candidates: 9 } } }).success).toBe(false);
    expect(BookConfigSchema.safeParse({ ...baseBook, governance: { maxConsecutiveDebts: 99 } }).success).toBe(false);
  });

  it("StateManager.loadBookConfig：写面落盘键在位时加载不崩（701/707 潜伏自毁键清偿锁）", async () => {
    const state = new StateManager(root);
    // 2.0 StateManager 正典布局 = works/{id}/source（bookDir）；books/ 为
    // legacy 保留树（697 号迁移边界工项管）——本锁钉正典加载器。
    const bookDir = join(root, "works", "b1", "source");
    await mkdir(bookDir, { recursive: true });
    await writeFile(join(bookDir, "book.json"), JSON.stringify({
      ...baseBook,
      seriesId: "jing_hua_yuan",
      writing: { autoTimelineBeats: true },
      governance: { bestOfN: { enabled: true, candidates: 2, minScore: 70 } },
    }), "utf-8");
    const loaded = await state.loadBookConfig("b1");
    expect(loaded.id).toBe("b1");
    expect(loaded.writing?.autoTimelineBeats).toBe(true);
    expect(loaded.governance?.bestOfN?.enabled).toBe(true);
  });

  it("ProjectConfigSchema：writing 段（reviewRetries/reviewMode/revisionGate）合法容忍——Rust 引擎落盘键不再毒化 TS 回退腿", () => {
    const config = ProjectConfigSchema.parse({
      name: "inkos-contract-diff",
      version: "0.1.0",
      llm: { provider: "custom", baseUrl: "http://127.0.0.1:9/v1", apiKey: "sk", model: "m" },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
      writing: { reviewRetries: 2, reviewMode: "manual", revisionGate: "always" },
    });
    expect(config.writing?.reviewRetries).toBe(2);
    expect(config.writing?.reviewMode).toBe("manual");

    // 非法子值仍拒
    expect(ProjectConfigSchema.safeParse({
      name: "x", version: "0.1.0",
      llm: { provider: "custom", baseUrl: "http://x/v1", apiKey: "k", model: "m" },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
      writing: { reviewMode: "yolo" },
    }).success).toBe(false);

    // 缺省省略语义不变：无 writing 段照常解析
    expect(ProjectConfigSchema.safeParse({
      name: "x", version: "0.1.0",
      llm: { provider: "custom", baseUrl: "http://x/v1", apiKey: "k", model: "m" },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
    }).success).toBe(true);
  });
});
