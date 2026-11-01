#!/usr/bin/env node
// 792 号：legacy 夹具种子——引擎契约差分器（engine-contract-diff.mjs）的
// legacy 通道数据面。向 <root>/books/<id>/ 播种一本「legacy 书」：
//   - 章节产物 0001/0002（有文件）
//   - markdown 真相面（pending_hooks/current_state/chapter_summaries）
//   - **无 story/state/ 四件、无 chapters/index.json**——1.x 迁移书形态，
//     触发 788（snapshot bootstrap）/789（durable 容忍降级）/791（索引重建）
//     三面收敛路径，对双引擎做全端点动态认证。
// 纯文件播种，零 API/零 LLM 依赖；幂等（已存在即跳过写）。

import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");
const root = process.argv[2];
const enginePort = process.argv[3] ?? "8787";
if (!root) {
  console.error("[legacy-fixture] 用法：node scripts/legacy-fixture.mjs <projectRoot> [enginePort]");
  process.exit(2);
}

const LEGACY_BOOK = "legacy-book";
const bookDir = join(root, "books", LEGACY_BOOK);

if (!existsSync(join(bookDir, "book.json"))) {
  mkdirSync(join(bookDir, "chapters"), { recursive: true });
  mkdirSync(join(bookDir, "story"), { recursive: true });
  writeFileSync(
    join(bookDir, "book.json"),
    JSON.stringify({
      id: LEGACY_BOOK,
      title: "残卷拾遗",
      platform: "qidian",
      genre: "xuanhuan",
      status: "active",
      targetChapters: 12,
      chapterWordCount: 1600,
      language: "zh",
      createdAt: "2025-01-01T00:00:00.000Z",
      updatedAt: "2025-01-01T00:00:00.000Z",
    }),
  );
  writeFileSync(join(bookDir, "chapters", "0001_风起.md"), "# 第1章 风起\n\n林动握紧手中的祖符玉佩，坊市的喧嚣在身后渐渐远去。\n");
  writeFileSync(join(bookDir, "chapters", "0002_推进.md"), "# 第2章 推进\n\n山道上的风带着初秋的凉意，他回望这座生活了十六年的小城。\n");
  // 14 列台账形态（R23/393），单 hook：
  writeFileSync(
    join(bookDir, "story", "pending_hooks.md"),
    "| hook_id | 章号 | 类型 | 状态 | 最近推进章 | 预期兑现 | notes |\n|---|---|---|---|---|---|---|\n| H01 | 1 | mystery | open | 1 | 玉符来历揭开 | 祖符之谜 |\n",
  );
  writeFileSync(
    join(bookDir, "story", "current_state.md"),
    "| 字段 | 值 |\n|---|---|\n| 当前章节 | 2 |\n| 当前位置 | 坊市 |\n",
  );
  writeFileSync(
    join(bookDir, "story", "chapter_summaries.md"),
    "| 1 | 风起 | 林动 | 醒来入市 | 无 | 无 | 紧张 | 开篇章 |\n",
  );
  console.log(`[legacy-fixture] 已播种 legacy 书：${bookDir}`);

  // 697 号桥镜像（walkthrough-fixture §2.5 同款）：2.0 TS 腿的 /books 列表走
  // works 清单，books/<id> 须经 migrateLegacyWorks 注册才可见（fork 引擎原生
  // 列 books 目录无需迁移；prompt-packs 可达=fork 契约腿，跳过迁移）。
  // 迁移后 node 腿读 works/<id>/source——788（snapshot bootstrap）/789（durable
  // 容忍降级）/791（索引重建）的收敛路径照常被触发认证。
  const forkContract = await fetch(`http://127.0.0.1:${enginePort}/api/v1/prompt-packs`)
    .then((r) => r.ok)
    .catch(() => false);
  if (!forkContract) {
    const core = await import(join(repoRoot, "packages", "core", "dist", "index.js"))
      .catch((error) => {
        console.error(`[legacy-fixture] 无法加载 packages/core/dist（先 build core）：${error.message}`);
        return null;
      });
    if (core?.migrateLegacyWorks) {
      const results = await core.migrateLegacyWorks(root, { apply: true });
      for (const item of results) {
        console.log(`[legacy-fixture] work migrate: ${item.source} | ${item.status}${item.error ? ` | ${item.error}` : ""}`);
      }
    }
  }
  console.log(`[legacy-fixture] 已播种 legacy 书：${bookDir}`);
} else {
  console.log(`[legacy-fixture] 已存在，跳过：${bookDir}`);
}
