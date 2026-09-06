import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { TimelineSchema, type Timeline } from "../models/timeline.js";

/**
 * 时间线节拍自动沉淀（191 号；与 189 号 Rust `agents/timeline_settler.rs` +
 * `pipeline/timeline_settle.rs` 逐字对齐，补齐 Node 回退端行为差异）。
 *
 * write-next 落盘后按既有情节线为本章补节拍：书籍级设置
 * `writing.autoTimelineBeats`（默认关）；LLM 失败/解析失败仅告警，
 * 不影响章节产物（章节已落盘，沉淀属事后增强）。
 */

export interface PlotlineBeat {
  readonly plotlineId: string;
  readonly title?: string;
  readonly note?: string;
}

export interface TimelineBeatsRequest {
  readonly chapterNumber: number;
  readonly chapterTitle: string;
  readonly chapterSummary: string;
  /** 既有情节线名册——模型只允许在其中挑选归属。 */
  readonly plotlines: ReadonlyArray<{ readonly id: string; readonly name: string }>;
  readonly language: "zh" | "en";
}

/** 节拍提取端口：返回模型原始文本（生产 = chatCompletion；测试 = 注入）。 */
export type TimelineBeatsChat = (req: TimelineBeatsRequest) => Promise<string>;

/** 构造（system, user）双消息 prompt（与 Rust build_beats_prompt 逐字一致）。 */
export function buildBeatsPrompt(req: TimelineBeatsRequest): { system: string; user: string } {
  const system = req.language === "en"
    ? "You are a novel story-grid editor. Output pure JSON only — no markdown fences, no commentary."
    : "你是网文时间线编辑。只输出纯 JSON，不要 markdown 围栏或任何解释文字。";
  let roster = "";
  for (const [id, name] of req.plotlines.map((line) => [line.id, line.name] as const)) {
    roster += `- ${id}: ${name}\n`;
  }
  const user = req.language === "en"
    ? `Chapter ${req.chapterNumber} "${req.chapterTitle}" was just written. Summary:\n${req.chapterSummary}\n\nPlotlines:\n${roster}\nFor each plotline, decide whether this chapter advances it. Output pure JSON:\n{"beats":[{"plotlineId":"<id from roster>","title":"<=12 chars beat title","note":"one-sentence beat note"}]}\nOnly include plotlines this chapter actually touches; omit the rest.`
    : `第 ${req.chapterNumber} 章《${req.chapterTitle}》刚完成写作。章节梗概：\n${req.chapterSummary}\n\n情节线名册：\n${roster}\n请为每条情节线判断本章是否推进了它，并输出纯 JSON：\n{"beats":[{"plotlineId":"<名册中的 id>","title":"12字以内的节拍标题","note":"一句话节拍说明"}]}\n只输出本章真正推进的情节线；未推进的不要输出。`;
  return { system, user };
}

/**
 * 宽松解析模型输出：剥 markdown 围栏、过滤未知 plotline id 与空节拍
 * （title/note 皆空视为无效）；结构性垃圾抛错。
 */
export function parseBeatsJson(text: string, validIds: ReadonlySet<string>): ReadonlyArray<PlotlineBeat> {
  const trimmed = text.trim();
  const stripped = (trimmed.startsWith("```json") || trimmed.startsWith("```"))
    ? trimmed.replace(/^```(json)?/, "").replace(/```$/, "").trim()
    : trimmed;
  let value: unknown;
  try {
    value = JSON.parse(stripped);
  } catch (error) {
    throw new Error(`beats JSON parse failed: ${error instanceof Error ? error.message : String(error)}`);
  }
  const beats = (value as { beats?: unknown })?.beats;
  if (!Array.isArray(beats)) {
    throw new Error("beats JSON missing `beats` array");
  }
  const out: PlotlineBeat[] = [];
  for (const item of beats) {
    const record = item as Record<string, unknown>;
    const plotlineId = typeof record.plotlineId === "string" ? record.plotlineId : undefined;
    if (!plotlineId || !validIds.has(plotlineId)) continue;
    const title = typeof record.title === "string" ? record.title : undefined;
    const note = typeof record.note === "string" ? record.note : undefined;
    const isBlank = (v: string | undefined): boolean => v === undefined || v.trim().length === 0;
    if (isBlank(title) && isBlank(note)) continue;
    out.push({ plotlineId, title, note });
  }
  return out;
}

/**
 * 把某章的节拍合并进时间线（原位）：命中既有 cell 则替换，否则按章号升序
 * 插入；未知 plotline id 忽略；空 title/note 净化为 undefined。返回实际落格
 * 的线条数；有落格时刷新 updatedAt（ISO，与 Rust utc_now_iso 同格式）。
 */
export function mergeChapterBeats(timeline: Timeline, chapter: number, beats: ReadonlyArray<PlotlineBeat>): number {
  let applied = 0;
  for (const beat of beats) {
    const line = timeline.plotlines.find((p) => p.id === beat.plotlineId);
    if (!line) continue;
    const cell = {
      chapter,
      ...(beat.title !== undefined && beat.title.trim().length > 0 ? { title: beat.title } : {}),
      ...(beat.note !== undefined && beat.note.trim().length > 0 ? { note: beat.note } : {}),
    };
    const existingIndex = line.cells.findIndex((c) => c.chapter === chapter);
    if (existingIndex >= 0) {
      line.cells[existingIndex] = cell;
    } else {
      const insertAt = line.cells.findIndex((c) => c.chapter > chapter);
      if (insertAt >= 0) line.cells.splice(insertAt, 0, cell);
      else line.cells.push(cell);
    }
    applied += 1;
  }
  if (applied > 0) {
    timeline.updatedAt = new Date().toISOString();
  }
  return applied;
}

/** 沉淀结果：`null` = 无时间线可沉淀（静默跳过）；数字 = 落格 n 条线条。 */
export async function settleTimelineBeatsForChapter(deps: {
  readonly bookDir: string;
  readonly chapterNumber: number;
  readonly chapterTitle: string;
  readonly chapterSummary: string;
  readonly language: "zh" | "en";
  readonly chat: TimelineBeatsChat;
}): Promise<number | null> {
  const path = join(deps.bookDir, "story", "timeline.json");
  let raw: string;
  try {
    raw = await readFile(path, "utf-8");
  } catch {
    return null;
  }
  let timeline: Timeline;
  try {
    timeline = TimelineSchema.parse(JSON.parse(raw));
  } catch (error) {
    throw new Error(`timeline.json 不可解析，跳过节拍沉淀: ${error instanceof Error ? error.message : String(error)}`);
  }
  if (timeline.plotlines.length === 0) {
    return null;
  }
  const beats = parseBeatsJson(
    await deps.chat({
      chapterNumber: deps.chapterNumber,
      chapterTitle: deps.chapterTitle,
      chapterSummary: deps.chapterSummary,
      plotlines: timeline.plotlines.map((line) => ({ id: line.id, name: line.name })),
      language: deps.language,
    }),
    new Set(timeline.plotlines.map((line) => line.id)),
  );
  const applied = mergeChapterBeats(timeline, deps.chapterNumber, beats);
  if (applied === 0) {
    return 0;
  }
  await mkdir(join(deps.bookDir, "story"), { recursive: true });
  await writeFile(path, `${JSON.stringify(timeline, null, 2)}\n`, "utf-8");
  return applied;
}
