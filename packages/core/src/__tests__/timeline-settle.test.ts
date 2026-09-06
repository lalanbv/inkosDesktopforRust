import { afterEach, describe, expect, it, vi } from "vitest";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  buildBeatsPrompt,
  mergeChapterBeats,
  parseBeatsJson,
  settleTimelineBeatsForChapter,
  type TimelineBeatsRequest,
} from "../pipeline/timeline-settle.js";

const ROOTS: string[] = [];

afterEach(async () => {
  await Promise.all(ROOTS.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  vi.restoreAllMocks();
});

async function tempBookDir(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "inkos-timeline-settle-"));
  ROOTS.push(root);
  return root;
}

const ROSTER: TimelineBeatsRequest["plotlines"] = [
  { id: "main", name: "主线" },
  { id: "sub", name: "副线" },
];
const VALID_IDS = new Set(ROSTER.map((line) => line.id));

function req(overrides: Partial<TimelineBeatsRequest> = {}): TimelineBeatsRequest {
  return {
    chapterNumber: 3,
    chapterTitle: "风起",
    chapterSummary: "主角入场",
    plotlines: ROSTER,
    language: "zh",
    ...overrides,
  };
}

describe("timeline-settle（191 号 Node 回退端节拍沉淀，对齐 189 号 Rust）", () => {
  it("prompt 双语变体：名册 + 章号 + JSON 形状", () => {
    const zh = buildBeatsPrompt(req());
    expect(zh.system).toContain("纯 JSON");
    expect(zh.user).toContain("- main: 主线");
    expect(zh.user).toContain("第 3 章");
    expect(zh.user).toContain("plotlineId");

    const en = buildBeatsPrompt(req({ language: "en", chapterTitle: "Storm", chapterSummary: "The hero arrives." }));
    expect(en.system).toContain("pure JSON");
    expect(en.user).not.toContain("第 3 章");
    expect(en.user).toContain('Chapter 3 "Storm"');
  });

  it("parse：纯 JSON 与围栏包裹均可；未知 id 与空节拍过滤", () => {
    const plain = '{"beats":[{"plotlineId":"main","title":"风起","note":"主角入场"}]}';
    expect(parseBeatsJson(plain, VALID_IDS)).toHaveLength(1);
    expect(parseBeatsJson("```json\n" + plain + "\n```", VALID_IDS)).toHaveLength(1);

    const mixed = JSON.stringify({
      beats: [
        { plotlineId: "ghost", title: "幽灵", note: "未知线条" },
        { plotlineId: "main", title: "  ", note: "" },
        { plotlineId: "sub", title: "副线推进", note: "配角入场" },
      ],
    });
    const beats = parseBeatsJson(mixed, VALID_IDS);
    expect(beats).toHaveLength(1);
    expect(beats[0]).toMatchObject({ plotlineId: "sub", title: "副线推进" });
  });

  it("parse：结构性垃圾抛错（对齐 Rust Err 面）", () => {
    expect(() => parseBeatsJson("not json at all", VALID_IDS)).toThrow(/beats JSON parse failed/);
    expect(() => parseBeatsJson('{"items":[]}', VALID_IDS)).toThrow(/missing `beats` array/);
  });

  it("merge：原位替换 + 章号升序插入 + 空值净化", () => {
    const timeline = {
      version: 1 as const,
      bookId: "b1",
      updatedAt: "2026-01-01T00:00:00.000Z",
      plotlines: [
        { id: "main", name: "主线", cells: [{ chapter: 1, title: "风起", note: "旧节拍" }] },
        { id: "sub", name: "副线", cells: [] },
      ],
    };
    expect(mergeChapterBeats(timeline, 1, [{ plotlineId: "main", title: "风起（改）", note: "更新" }])).toBe(1);
    expect(timeline.plotlines[0].cells).toHaveLength(1);
    expect(timeline.plotlines[0].cells[0].title).toBe("风起（改）");

    expect(mergeChapterBeats(timeline, 3, [{ plotlineId: "main", title: "高潮" }])).toBe(1);
    expect(mergeChapterBeats(timeline, 2, [{ plotlineId: "main", note: "推进" }])).toBe(1);
    expect(timeline.plotlines[0].cells.map((cell) => cell.chapter)).toEqual([1, 2, 3]);
    expect(timeline.plotlines[0].cells[1].title).toBeUndefined();

    // 未知 plotline id 忽略。
    expect(mergeChapterBeats(timeline, 4, [{ plotlineId: "ghost", title: "不存在" }])).toBe(0);
    // 有落格时刷新 updatedAt。
    expect(timeline.updatedAt).not.toBe("2026-01-01T00:00:00.000Z");
  });

  it("settle：写入既有时间线并把名册透传给端口", async () => {
    const bookDir = await tempBookDir();
    await mkdir(join(bookDir, "story"), { recursive: true });
    await writeFile(
      join(bookDir, "story", "timeline.json"),
      JSON.stringify({ version: 1, bookId: "b1", updatedAt: "2026-01-01T00:00:00.000Z", plotlines: [{ id: "main", name: "主线", cells: [] }] }),
      "utf-8",
    );
    const chat = vi.fn().mockResolvedValue('{"beats":[{"plotlineId":"main","title":"风起·沉淀","note":"少年入场"}]}');
    const applied = await settleTimelineBeatsForChapter({
      bookDir,
      chapterNumber: 1,
      chapterTitle: "风起",
      chapterSummary: "主角入场",
      language: "zh",
      chat,
    });
    expect(applied).toBe(1);
    expect(chat).toHaveBeenCalledWith(expect.objectContaining({
      chapterNumber: 1,
      plotlines: [{ id: "main", name: "主线" }],
    }));
    const written = JSON.parse(await readFile(join(bookDir, "story", "timeline.json"), "utf-8"));
    expect(written.plotlines[0].cells).toEqual([expect.objectContaining({ chapter: 1, title: "风起·沉淀" })]);
  });

  it("settle：无时间线/无线条静默跳过且不发起 LLM 调用", async () => {
    const bookDir = await tempBookDir();
    const chat = vi.fn();
    expect(await settleTimelineBeatsForChapter({
      bookDir, chapterNumber: 1, chapterTitle: "t", chapterSummary: "s", language: "zh", chat,
    })).toBeNull();
    await mkdir(join(bookDir, "story"), { recursive: true });
    await writeFile(
      join(bookDir, "story", "timeline.json"),
      JSON.stringify({ version: 1, bookId: "b1", updatedAt: "t", plotlines: [] }),
      "utf-8",
    );
    expect(await settleTimelineBeatsForChapter({
      bookDir, chapterNumber: 1, chapterTitle: "t", chapterSummary: "s", language: "zh", chat,
    })).toBeNull();
    expect(chat).not.toHaveBeenCalled();
  });

  it("settle：坏载荷抛错、LLM 失败透传且不改写原文件", async () => {
    const bookDir = await tempBookDir();
    await mkdir(join(bookDir, "story"), { recursive: true });
    await writeFile(join(bookDir, "story", "timeline.json"), "{ broken", "utf-8");
    await expect(settleTimelineBeatsForChapter({
      bookDir, chapterNumber: 1, chapterTitle: "t", chapterSummary: "s", language: "zh",
      chat: async () => "{}",
    })).rejects.toThrow(/timeline\.json 不可解析/);

    const timelinePath = join(bookDir, "story", "timeline.json");
    await writeFile(timelinePath, JSON.stringify({ version: 1, bookId: "b1", updatedAt: "t", plotlines: [{ id: "main", name: "主线", cells: [] }] }), "utf-8");
    await expect(settleTimelineBeatsForChapter({
      bookDir, chapterNumber: 1, chapterTitle: "t", chapterSummary: "s", language: "zh",
      chat: async () => { throw new Error("llm down"); },
    })).rejects.toThrow("llm down");
    const raw = await readFile(timelinePath, "utf-8");
    expect(JSON.parse(raw).plotlines[0].cells).toEqual([]);
  });
});
