#!/usr/bin/env node
// R47/1640 号：全局搜索夹具种子——引擎契约差分器（engine-contract-diff.mjs）
// search 通道数据面（792 号 legacy-fixture.mjs 同惯用形）。
//
// 双根布局播种（works 双根正典惯例）：同一内容写两处——
//   - books/search-book/……（Rust 引擎腿布局：books/<id>）
//   - works/search-book/source/……（TS studio 腿布局：works/<id>/source，
//     StateManager.bookDir 正典——walkthrough-fixture 230 行同形态）
// 播种面：
//   - 章节产物 0001-0004 + chapters/index.json（chapters 轴 BM25 命中序）
//   - story/entity_codex.json（文件卡）+ entity_roster.md（派生卡）——entity 轴
//   - 真相面 story_bible.md（双端 truth 枚举交集文件）——truth 轴
//   - hook 轴**不播种**：798 号备案双端源结构差（TS=state 正典派生 vs
//     Rust=memory.db 投影），轴逻辑由双端单测 + golden vectors 覆盖。
// 纯文件播种，零 API/零 LLM 依赖；幂等（已存在即跳过写）。

import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const root = process.argv[2];
if (!root) {
  console.error("[search-fixture] 用法：node scripts/search-fixture.mjs <projectRoot>");
  process.exit(2);
}

const SEARCH_BOOK = "search-book";

const BOOK_JSON = {
  id: SEARCH_BOOK,
  title: "搜索校准书",
  platform: "qidian",
  genre: "xuanhuan",
  status: "active",
  targetChapters: 8,
  chapterWordCount: 1600,
  language: "zh",
  createdAt: "2026-11-21T00:00:00.000Z",
  updatedAt: "2026-11-21T00:00:00.000Z",
};
// chapters 轴：0002「灵气」词频 2 > 0001 词频 1（BM25 排序锚）；0003 标题带
// 「林动」（标题权重 5.0 锚）；0004 零命中面。
const CHAPTERS = [
  ["0001_风起.md", "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。\n", { number: 1, title: "风起", wordCount: 18 }],
  ["0002_灵脉.md", "# 第2章 灵脉\n\n灵气紊乱，林动被迫闭关，灵气再度翻涌。\n", { number: 2, title: "灵脉", wordCount: 20 }],
  ["0003_林动传.md", "# 第3章 林动传\n\n与主角无关的旁支剧情。\n", { number: 3, title: "林动传", wordCount: 12 }],
  ["0004_收束.md", "# 第4章 收束\n\n故事在此收束。\n", { number: 4, title: "收束", wordCount: 8 }],
];
// entity 轴：文件卡（name/aliases/summary/facts 权重梯度）+ 名册派生卡
// （青阳镇不在文件卡——派生面同构，golden entity-codex-vectors 已锁）。
const CODEX_JSON = {
  version: 1,
  cards: [
    {
      name: "林动",
      aliases: ["武祖"],
      kind: "character",
      summary: "主角，修炼天才。",
      facts: ["曾在青阳镇外门试炼", "佩祖符玉佩"],
      relationships: [{ target: "青阳镇", note: "出身地" }],
    },
  ],
};
const ROSTER_MD =
  "# 实体名册（Entity Roster）\n\n## entity-qingyang\n- name: 青阳镇\n- aliases: \n- kind: location\n- registeredAt: 0\n";
// truth 轴：current_state.md 在双端 truth 枚举交集内（Rust flat 白名单 + TS
// TRUTH_FLAT_FILES 均含），承载查询词「之谜」；story_bible.md 只在 Rust flat
// 侧（TS 白名单无此文件）——其文本须避开查询词，否则 Rust 多出 TS 不列文件的
// 命中（entity_codex.json/entity_roster.md 同理只许 Rust-only 静默）。
const BIBLE_MD = "# 故事圣经\n\n主角林动，出身青阳镇，随身玉佩藏祖符来历。\n";
const CURRENT_STATE_MD = "| 字段 | 值 |\n|---|---|\n| 当前位置 | 碎镜崖之谜待解 |\n";

function seedBook(base) {
  const bookDir = join(base, "books", SEARCH_BOOK);
  if (existsSync(join(bookDir, "book.json"))) return false;
  mkdirSync(join(bookDir, "chapters"), { recursive: true });
  mkdirSync(join(bookDir, "story"), { recursive: true });
  writeFileSync(join(bookDir, "book.json"), JSON.stringify(BOOK_JSON));
  for (const [name, content] of CHAPTERS) {
    writeFileSync(join(bookDir, "chapters", name), content);
  }
  writeFileSync(
    join(bookDir, "chapters", "index.json"),
    JSON.stringify(
      CHAPTERS.map(([, , meta]) => ({
        ...meta,
        status: "ready-for-review",
        createdAt: "2026-11-21T00:00:00.000Z",
        updatedAt: "2026-11-21T00:00:00.000Z",
        auditIssues: [],
        lengthWarnings: [],
      })),
    ),
  );
  writeFileSync(
    join(bookDir, "story", "entity_codex.json"),
    JSON.stringify(CODEX_JSON),
  );
  writeFileSync(join(bookDir, "story", "entity_roster.md"), ROSTER_MD);
  writeFileSync(join(bookDir, "story", "story_bible.md"), BIBLE_MD);
  writeFileSync(join(bookDir, "story", "current_state.md"), CURRENT_STATE_MD);
  return true;
}

function seedWorkSource() {
  // TS studio 腿正典布局：works/<id>/source/（StateManager.bookDir）。
  const sourceDir = join(root, "works", SEARCH_BOOK, "source");
  if (existsSync(join(sourceDir, "chapters", "index.json"))) return false;
  mkdirSync(join(sourceDir, "chapters"), { recursive: true });
  mkdirSync(join(sourceDir, "story"), { recursive: true });
  for (const [name, content] of CHAPTERS) {
    writeFileSync(join(sourceDir, "chapters", name), content);
  }
  writeFileSync(join(sourceDir, "story", "entity_codex.json"), JSON.stringify(CODEX_JSON));
  writeFileSync(join(sourceDir, "story", "entity_roster.md"), ROSTER_MD);
  writeFileSync(join(sourceDir, "story", "story_bible.md"), BIBLE_MD);
  writeFileSync(join(sourceDir, "story", "current_state.md"), CURRENT_STATE_MD);
  return true;
}

const seededBooks = seedBook(root);
const seededWorks = seedWorkSource();
for (const [name, dir, seeded] of [
  ["books", join(root, "books", SEARCH_BOOK), seededBooks],
  ["works/source", join(root, "works", SEARCH_BOOK, "source"), seededWorks],
]) {
  if (seeded) console.log(`[search-fixture] 已播种 ${name}：${dir}`);
}
if (!seededBooks && !seededWorks) console.log("[search-fixture] 已存在，跳过");
