import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  buildPlannerUserMessage,
  getPlannerMemoSystemPrompt,
  getPlannerMemoUserTemplate,
} from "../agents/planner-prompts.js";
import { buildSettlerSystemPrompt, buildSettlerUserPrompt } from "../agents/settler-prompts.js";
import { buildWriterSystemPrompt } from "../agents/writer-prompts.js";
import type { BookConfig } from "../models/book.js";
import type { BookRules } from "../models/book-rules.js";
import type { GenreProfile } from "../models/genre-profile.js";

/**
 * 528 号：写作链 prompt golden 快照（planner + settler + writer，双端守门）。
 * 事实源 = golden/writing-prompts.json；engine 侧
 * `engine-rs/tests/golden_writing_prompts_diff.rs` include_str! 同文件断言，
 * 锁死手抄移植面（措辞漂移=双端同输入下给 LLM 的指令分叉）。
 * 529 号：writer 面 6 案例扩展（对齐 c56586ec 后形态）。
 * 首次生成 / 快照更新：REGEN=1 npx vitest run 本文件。
 */
const goldenPath = join(import.meta.dirname, "golden", "writing-prompts.json");

const bookFixture = {
  id: "b1",
  title: "镜花水月",
  platform: "qidian",
  genre: "东方玄幻",
  status: "active",
  targetChapters: 200,
  chapterWordCount: 3000,
  createdAt: "2026-01-01T00:00:00.000Z",
  updatedAt: "2026-01-01T00:00:00.000Z",
} as BookConfig;

const genreFixture = {
  name: "东方玄幻",
  id: "xuanhuan",
  language: "zh",
  chapterTypes: ["日常推进"],
  fatigueWords: [],
  numericalSystem: false,
  powerScaling: false,
  eraResearch: false,
  pacingRule: "",
  satisfactionTypes: [],
  auditDimensions: [],
} as GenreProfile;

const numericalGenreFixture = { ...genreFixture, numericalSystem: true } as GenreProfile;

const fullCastRules = { enableFullCastTracking: true } as unknown as BookRules;

function buildGoldenSnapshot(): Record<string, string> {
  const snapshot: Record<string, string> = {};

  // planner：system / user 模板（zh/en 双语逐字）。
  snapshot["planner.system.zh"] = getPlannerMemoSystemPrompt("zh");
  snapshot["planner.system.en"] = getPlannerMemoSystemPrompt("en");
  snapshot["planner.template.zh"] = getPlannerMemoUserTemplate("zh");
  snapshot["planner.template.en"] = getPlannerMemoUserTemplate("en");

  // planner user message：黄金三章案例（ch2 + brief + 本章指令）。
  snapshot["planner.user.zh.golden"] = buildPlannerUserMessage({
    chapterNumber: 2,
    previousChapterEndingExcerpt: "……门缝里的光熄了。",
    recentSummaries: "- 第1章：林秋入府为杂役，捡到残缺腰牌。",
    currentArcProse: "主线：腰牌来历牵出府邸旧案。",
    protagonistMatrixRow: "- 林秋｜杂役｜隐忍、观察力强",
    opponentRows: "- 王管事｜克扣月钱、试探新人",
    collaboratorRows: "- 师兄｜指点规矩、来路不明",
    relevantThreads: "- H003 杂役腰牌（pressured）",
    recyclableHooks: "- H003：距上次推进 1 章",
    isGoldenOpening: true,
    bookRulesRelevant: "- 禁止主角突然圣母\n- 反派不降智",
    lengthBudget: { target: 2000, softMin: 1600, softMax: 2400, hardMin: 1200, hardMax: 3000, unit: "字" },
    brief: "双主线：权谋 60% + 感情 40%；开场三章节奏要快。",
    chapterContext: "本章要出现第一次系统提示。",
    language: "zh",
  });

  // planner user message：普通章节案例（ch5，无 brief/指令，英文书）。
  snapshot["planner.user.en.plain"] = buildPlannerUserMessage({
    chapterNumber: 5,
    previousChapterEndingExcerpt: "The light behind the door went out.",
    recentSummaries: "- Ch1: Lin Qiu enters the manor as a servant.",
    currentArcProse: "Main line: the token traces back to an old case.",
    protagonistMatrixRow: "- Lin Qiu | servant | observant",
    opponentRows: "- Steward Wang | petty tyranny",
    collaboratorRows: "- Senior brother | unclear motives",
    relevantThreads: "- H003 servant token (pressured)",
    recyclableHooks: "- H003: advanced 1 chapter ago",
    isGoldenOpening: false,
    bookRulesRelevant: "- No sudden saintliness",
    lengthBudget: { target: 2000, softMin: 1600, softMax: 2400, hardMin: 1200, hardMax: 3000, unit: "words" },
    language: "en",
  });

  // settler system：无数值体系基线案例。
  snapshot["settler.system.zh.plain"] = buildSettlerSystemPrompt(bookFixture, genreFixture, null);

  // settler system：数值体系 + 全员追踪分支案例。
  snapshot["settler.system.zh.numerical"] = buildSettlerSystemPrompt(bookFixture, numericalGenreFixture, fullCastRules);

  // settler user message：全量案例（可选块全给 + governed 控制块让卷纲让位）。
  snapshot["settler.user.zh.full"] = buildSettlerUserPrompt({
    chapterNumber: 12,
    title: "镜中裂痕",
    content: "林秋推开门，看见了不该看见的东西。",
    currentState: "位置：柴房；目标：查清腰牌来历",
    ledger: "灵石：120→80",
    hooks: "- H003 杂役腰牌（pressured，最近推进 11）",
    chapterSummaries: "## 第 11 章\n- 林秋被罚抄规矩。",
    subplotBoard: "- 支线A：师兄的身世（推进中）",
    emotionalArcs: "- 林秋：隐忍→将爆发",
    characterMatrix: "- 林秋×王管事：提防",
    volumeOutline: "第 12 章：腰牌第一次发光。",
    observations: "1) 林秋进入正房 2) 腰牌发热",
    selectedEvidenceBlock: "- E009 门口血迹（第 9 章）",
    validationFeedback: "上一轮 H003 状态与正文矛盾，请修正。",
    governedControlBlock: "【GOVERNED CONTROL】按治理方案压缩支线。",
  });

  // settler user message：最小案例（truth 文件未创建 + 卷纲块兜底）。
  snapshot["settler.user.zh.min"] = buildSettlerUserPrompt({
    chapterNumber: 1,
    title: "入府",
    content: "林秋背着一卷铺盖走进角门。",
    currentState: "(文件尚未创建)",
    ledger: "",
    hooks: "(文件尚未创建)",
    chapterSummaries: "(文件尚未创建)",
    subplotBoard: "(文件尚未创建)",
    emotionalArcs: "(文件尚未创建)",
    characterMatrix: "(文件尚未创建)",
    volumeOutline: "第 1 章：入府，捡到腰牌。",
  });

  // ---------------------------------------------------------------------------
  // writer system：6 案例面（529 号扩展）。
  // 生产唯一调用形态 = writer.ts 的 creative + governed；其余面锁分支矩阵
  // （en / golden-open / legacy+full / numerical+fullCast+主角铁律 / fanfic 三段）。
  // lengthSpec 一律不传 → 走 buildLengthSpec(3000) 默认推导（同为手抄移植面）。
  // ---------------------------------------------------------------------------

  const writerGenreEn = { ...genreFixture, name: "Eastern Xuanhuan", language: "en" } as GenreProfile;

  const writerRulesFull = {
    version: "1.0",
    protagonist: {
      name: "林秋",
      personalityLock: ["隐忍", "观察力强"],
      behavioralConstraints: ["不滥杀", "不透露腰牌来历"],
    },
    genreLock: { primary: "东方玄幻", forbidden: ["科幻", "西幻"] },
    narrativePerson: "first",
    prohibitions: ["禁止主角突然圣母", "反派不降智"],
    enableFullCastTracking: true,
  } as unknown as BookRules;

  // 生产主形态：zh + creative + governed，ch6（无黄金开篇段）。
  snapshot["writer.system.zh.governed-creative"] = buildWriterSystemPrompt(
    bookFixture, genreFixture, null, "", "题材正文：灵气复苏下的都市修行。",
    "## 文风\n短句为主，动作外化情绪。", undefined, 6, "creative", undefined, undefined, "governed",
  );

  // 黄金开篇：ch2 追加黄金三章纪律段。
  snapshot["writer.system.zh.governed-creative-golden-open"] = buildWriterSystemPrompt(
    bookFixture, genreFixture, null, "", "", "", undefined, 2, "creative", undefined, undefined, "governed",
  );

  // 英文书：en 序列 + en 字数单位（words）。
  snapshot["writer.system.en.governed-creative"] = buildWriterSystemPrompt(
    bookFixture, writerGenreEn, null, "", "Genre guidance: qi revival in a modern city.",
    "Style: short sentences, show don't tell.", "风格指纹样本", 12, "creative", undefined, "en", "governed",
  );

  // 库兼容面：legacy + full（旧输出格式）；styleGuide 缺失标记 → 文风指南段缺位。
  snapshot["writer.system.zh.legacy-full"] = buildWriterSystemPrompt(
    bookFixture, genreFixture, null, "本书专属规则正文：禁止圣母。", "", "(文件尚未创建)", undefined, 9, "full",
  );

  // 分支全家桶：numerical + fullCast + 主角铁律 + 人称硬约束 + 禁忌/风格禁区。
  snapshot["writer.system.zh.full-cast-numerical"] = buildWriterSystemPrompt(
    bookFixture, numericalGenreFixture, writerRulesFull, "", "", "文风正文", undefined, 7, "full",
  );

  // 同人三段：canon 模式 + 允许偏离清单。
  snapshot["writer.system.zh.fanfic"] = buildWriterSystemPrompt(
    bookFixture, genreFixture, null, "", "", "", undefined, 5, "creative",
    { fanficCanon: "原作设定：林秋为杂役，腰牌来历不明。", fanficMode: "canon", allowedDeviations: ["口头禅可保留"] },
  );

  return snapshot;
}

describe("writing prompts golden snapshot (shared with engine)", () => {
  it("matches the committed snapshot", () => {
    const got = buildGoldenSnapshot();
    if (process.env.REGEN) {
      writeFileSync(goldenPath, `${JSON.stringify(got, null, 2)}\n`);
    }
    const want = JSON.parse(readFileSync(goldenPath, "utf-8")) as Record<string, string>;
    expect(got).toEqual(want);
  });
});
