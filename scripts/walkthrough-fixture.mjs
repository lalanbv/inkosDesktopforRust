#!/usr/bin/env node
// 走查数据脚手架（405/406 号）：生产引擎走查前的 fixture 书一键准备与验证。
//
//   node scripts/walkthrough-fixture.mjs [projectRoot] [enginePort]
//   # 默认 root=/tmp/inkos-walk、enginePort=8787；mock LLM 固定 1234
//
// 前置（两进程已由人工/脚本启动——或直接用 walkthrough-env.mjs 一键编排）：
//   node scripts/walkthrough-mock.mjs 1234
//   INKOS_PROJECT_ROOT=<root> INKOS_LLM_BASE_URL=http://127.0.0.1:1234/v1 \
//     INKOS_STATIC_DIR=packages/studio/dist INKOS_PORT=8787 inkos-engine-server
//   （/v1 必须带：模型列表与 chat/completions 都走 mock 的 /v1 面——436 号实测）
//
// 步骤（幂等，可重复跑）：
//   1. 写 inkos.json + .inkos/secrets.json（custom:Mock → walkthrough-mock）
//   2. 写 fixture 书「镜花水月」：book.json + 14 列台账（带分类）+ 3 章摘要 +
//      current_state + chapters/0001 + genres 资产
//   3. POST /resync/1（settle→validate 链，memory.db 前置）+ POST /write-next
//      （全链 → memory.db 投影 + run-log 真实调用记录）
//   4. 断言：/promises 的 timeline kind 全补齐、/run-log total 增长
//   5. 打印浏览器走查地址与四个 UI 面的点验要点

import { mkdirSync, writeFileSync, existsSync, copyFileSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");
const root = process.argv[2] ?? "/tmp/inkos-walk";
const enginePort = process.argv[3] ?? "8787";
const mockPort = process.argv[4] ?? "1234";
const base = `http://127.0.0.1:${enginePort}`;
const BOOK = "镜花水月";
const bookDir = join(root, "books", BOOK);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function api(path, init) {
  const res = await fetch(base + path, init);
  const text = await res.text();
  let body;
  try { body = JSON.parse(text); } catch { body = text; }
  return { status: res.status, body };
}

function die(message) {
  console.error(`[walkthrough-fixture] ${message}`);
  process.exit(1);
}

// ── 0. 前置健康检查 ──
const health = await api("/api/v1/run-log").catch(() => null);
if (!health || health.status !== 200) {
  die(`引擎不可达（${base}）。请先启动 walkthrough-mock 与 inkos-engine-server（见文件头注释）。`);
}
const mockModels = await fetch(`http://127.0.0.1:${mockPort}/v1/models`)
  .then((r) => r.ok)
  .catch(() => false);
if (!mockModels) {
  die(`LLM mock 不可达（http://127.0.0.1:${mockPort}）。请先启动：node scripts/walkthrough-mock.mjs ${mockPort}`);
}

// ── 1. 项目配置 ──
mkdirSync(join(root, ".inkos"), { recursive: true });
writeFileSync(join(root, "inkos.json"), JSON.stringify({
  name: "walkthrough",
  version: "0.1.0",
  language: "zh",
  llm: {
    service: "custom:Mock",
    defaultModel: "lm-mock-model",
    services: [{ service: "custom", name: "Mock", baseUrl: `http://127.0.0.1:${mockPort}/v1` }],
  },
}, null, 2));
writeFileSync(join(root, ".inkos", "secrets.json"), JSON.stringify({
  services: { "custom:Mock": { apiKey: "sk-mock" } },
}));

// ── 2. fixture 书 ──
mkdirSync(join(bookDir, "story"), { recursive: true });
mkdirSync(join(bookDir, "chapters"), { recursive: true });
mkdirSync(join(root, "genres"), { recursive: true });

writeFileSync(join(bookDir, "book.json"), JSON.stringify({
  id: BOOK, title: BOOK, platform: "qidian", genre: "xuanhuan", status: "active",
  targetChapters: 12, chapterWordCount: 3000, language: "zh",
  createdAt: "2026-09-14T00:00:00Z", updatedAt: "2026-09-14T00:00:00Z", version: 1,
}));

// 完整性判定 complete_book_exists = book.json + story/story_bible.md 双存在
// （TS completeBookExists 同判定）；缺 story_bible 时同名建书会按「不完整目录」
// 整目录删除重建，fixture 数据被静默清空（440 号真机实测，加固后实测 409）。
// 注意：story_bible.md 在场时会以受保护来源 story/story_bible.md#story-bible
// 进入 write-next 装配（440 号实测 12→13 条、+84 tokens）——对 mock 走查无碍，
// 但 lens 面板条目数以此为準，勿再用旧基线断言。
writeFileSync(join(bookDir, "story", "story_bible.md"), `# story_bible

- 镜中世界反向修行：镜外越弱，镜内越强。
- 苏檀贴身碎镜为镜界钥匙，母亲遗留。
- 镜宗与皇室盟约是第一卷底层冲突。
`);

// 14 列台账（R23 分类列）——伏笔池 chips 与 promises kind 的数据源。
writeFileSync(join(bookDir, "story", "pending_hooks.md"), `# 伏笔池

| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 升级 | 备注 | 分类 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| H01 | 1 | 身世 | progressing | 3 | 第10章 | slow-burn | 无 | 第一卷 | 是 | 10 | 是 | 镜中世界的来历真相。 | 悬念 |
| H02 | 2 | 情感线 | open | 3 | 第8章 | near-term | 无 | 第一卷 | 否 | 8 | 否 | 苏檀与镜灵的婚约誓言。 | 情感 |
| H03 | 2 | 信物 | pressured | 3 | 第6章 | near-term | H01 | 第一卷 | 否 | 6 | 否 | 母亲留下的碎镜在镜界发光。 | 物品 |
| H04 | 3 | 背景 | open | 0 | | slow-burn | 无 | 第二卷 | 否 | | 是 | 镜宗与皇室的隐秘盟约。 | 世界观 |
`);

writeFileSync(join(bookDir, "story", "chapter_summaries.md"), `# chapter_summaries

| 章 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 | 冲突 | 揭示 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 镜中醒来 | 苏檀 | 苏檀坠入镜中世界，发现镜外越弱镜内越强。 | 主角进入镜界 | H01 埋设 | 紧张 | 开局章 | 6 | 4 |
`);

writeFileSync(join(bookDir, "story", "current_state.md"), `# current_state

- 苏檀位于镜宗外门，身份为新晋弟子。
- 碎镜贴身收着，夜里会发微光。
- 皇子赵珩视苏檀为眼中钉。
`);

writeFileSync(join(bookDir, "chapters", "0001-镜中醒来.md"),
  "# 第1章 镜中醒来\n\n苏檀睁开眼，头顶是一面巨大的铜镜。\n\n镜中的世界倒悬着，他向下跌落，却落进了镜面深处。\n\n碎镜贴在胸口，微微发烫。\n");

// genres 资产（project 级查找路径 = <root>/genres/；rules_reader 三级查找）。
writeFileSync(join(root, "genres", "xuanhuan.md"),
  '---\nname: 玄幻\nid: xuanhuan\nchapterTypes: ["推进章","高潮章"]\nfatigueWords: ["震惊"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n');
writeFileSync(join(root, "genres", "other.md"),
  '---\nname: 通用\nid: other\nchapterTypes: ["推进章"]\nfatigueWords: []\nauditDimensions: [1]\nnumericalSystem: false\n---\n正文指导\n');
// 兼容引擎 INKOS_BUILTIN_GENRES_DIR 相对 CWD 的缺省形态（assets/genres）。
const builtinDir = join(root, "assets", "genres");
if (!existsSync(join(builtinDir, "xuanhuan.md"))) {
  mkdirSync(builtinDir, { recursive: true });
  copyFileSync(join(root, "genres", "xuanhuan.md"), join(builtinDir, "xuanhuan.md"));
  copyFileSync(join(root, "genres", "other.md"), join(builtinDir, "other.md"));
}

// 雷达历史 fixture（走查要点 4：历史回放 → 推荐卡「从该选题开书」）。
mkdirSync(join(root, "radar"), { recursive: true });
writeFileSync(join(root, "radar", "scan-20260914-walkthrough.json"), JSON.stringify({
  timestamp: "2026-09-14T03:00:00.000Z",
  marketSummary: "都市异能赛道近 30 天热度上升，稳定流占比高；玄幻修真竞争激烈但女主向存在空缺。",
  recommendations: [
    {
      platform: "起点",
      genre: "都市异能",
      concept: "外卖骑手觉醒「看见他人剩余寿命」的异能，在一次送单中发现自己的寿命栏是乱码",
      reasoning: "稳定流+单元案结构适合长线连载，异能设定自带钩子密度",
      confidence: 0.82,
      crowding: "medium",
      differentiation: "寿命乱码悬念可做全书总钩子，同类作尚未出现",
      benchmarkTitles: ["寿命算无遗策", "我真是良民"],
    },
    {
      platform: "番茄",
      genre: "玄幻",
      concept: "镜中世界反向修行的少年，镜外越弱镜内越强",
      reasoning: "番茄读者偏好快节奏爽点，镜像反差开局即高潮",
      confidence: 0.65,
      crowding: "low",
      benchmarkTitles: ["镜花水月"],
    },
  ],
}, null, 2));

// ── 3. resync（settle→validate 链 → memory.db 前置）+ write-next（全链）──
// 复用根守卫（440 号）：write-next 章已落盘（nextChapter ≥ 3）时 resync/1 会被
// 「Only the latest persisted chapter can be synced safely」拒绝——而 env 缺省
// root 复用（见 walkthrough-env 头注），二次起停必须走只读断言路径。
const enc = encodeURIComponent(BOOK);
const bookState = (await api(`/api/v1/books/${enc}`)).body;
// 复用根（440）之上再分两种（442 号）：fixture 全链要求 run-log 同进程增长——
// 而 env 重启后引擎内存 run-log 从零，跳过路径不得苛求该断言。
const chainDone = (bookState?.nextChapter ?? 0) >= 3;
let chainRan = false;
if (chainDone) {
  console.log(`[fixture] write-next 章已落盘（nextChapter=${bookState.nextChapter}）——复用根，跳过 resync/write-next 走只读断言`);
} else {
  chainRan = true;
  console.log("[fixture] POST resync/1 …");
  const resync = await api(`/api/v1/books/${enc}/resync/1`, { method: "POST" });
  if (resync.status !== 200) die(`resync 失败：${JSON.stringify(resync.body).slice(0, 300)}`);

  // 基线取 resync 之后的值——write-next 的完成信号 = 在基线上持续增长后趋稳。
  const baseline = (await api("/api/v1/run-log")).body.total ?? 0;

  console.log(`[fixture] POST write-next …（resync 后基线 = ${baseline}）`);
  await api(`/api/v1/books/${enc}/write-next`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: "{}",
  });

  let runLogAfter = baseline;
  let stable = 0;
  for (let i = 0; i < 30; i += 1) {
    await sleep(5000);
    const current = (await api("/api/v1/run-log")).body.total ?? 0;
    process.stdout.write(`[fixture] run-log total = ${current}\r`);
    if (current > baseline && current === runLogAfter) {
      stable += 1;
      if (stable >= 2) { runLogAfter = current; break; } // 连续两轮不变=链路结束
    } else {
      stable = 0;
    }
    runLogAfter = current;
  }
  console.log("");
}

// ── 3.5 伪造第 3 章压缩留痕工件（441 号）──
// 上下文透视的「压缩留痕」分支需要超预算章才在真实链路触发（缺省模型卡
// 128k 窗口远大于走查包），纯读面板的显示验证用作者化工件承担：以真 0002
// 工件为模板，可压来源替换为单条编译产物 + trace 增 compression 留痕。
// 链路级压缩逻辑由 composer 单测覆盖（budget_compiles_and_records_trace）。
const runtimeDir = join(bookDir, "story", "runtime");
const ctx2Path = join(runtimeDir, "chapter-0002.context.json");
const trace2Path = join(runtimeDir, "chapter-0002.trace.json");
if (existsSync(ctx2Path) && existsSync(trace2Path)) {
  const COMPRESSED_SRC = "runtime/compiled-compressible-context";
  const ctx2 = JSON.parse(readFileSync(ctx2Path, "utf8"));
  const trace2 = JSON.parse(readFileSync(trace2Path, "utf8"));
  const compressible = trace2.contextTiers?.compressibleSources ?? [];
  const kept = ctx2.selectedContext.filter((entry) => !compressible.includes(entry.source));
  const excerpt = [
    "## 编译摘要（mock 压缩产物）",
    "",
    "- 近章标题史：镜中醒来。",
    "- 情绪轨迹：紧张开局。",
    "- 上章结尾形态：苏檀坠入镜面深处，碎镜发烫。",
    "- 未兑现承诺：H01 身世悬念推进中。",
  ].join("\n");
  const compiledEntry = {
    source: COMPRESSED_SRC,
    reason: "Compressible sources compiled into one summary entry (fixture-authored).",
    excerpt,
  };
  const estimateTokens = (entry) => {
    const text = [entry.source, entry.reason, entry.excerpt ?? ""].filter(Boolean).join("\n");
    const cjk = (text.match(/[\u3400-\u9fff]/g) ?? []).length;
    return Math.ceil(cjk + (text.length - cjk) / 4);
  };
  const perSourceTokens = Object.fromEntries(ctx2.selectedContext.map((entry) => [entry.source, estimateTokens(entry)]));
  const protectedTokens = kept.reduce((sum, entry) => sum + estimateTokens(entry), 0);
  const compiledTokens = estimateTokens(compiledEntry);
  const ctx3 = { ...ctx2, chapter: 3, selectedContext: [...kept, compiledEntry] };
  const trace3 = {
    ...trace2,
    chapter: 3,
    selectedSources: [...kept.map((entry) => entry.source), COMPRESSED_SRC],
    contextTiers: {
      protectedSources: trace2.contextTiers?.protectedSources ?? [],
      compressibleSources: [COMPRESSED_SRC],
    },
    tokenBudget: {
      protectedTokens,
      compressibleTokens: compiledTokens,
      totalSelectedTokens: protectedTokens + compiledTokens,
    },
    compression: {
      compiledSource: COMPRESSED_SRC,
      budgetTokens: 1500,
      protectedTokens,
      compressibleTokens: trace2.tokenBudget?.compressibleTokens ?? 0,
      sourceTokens: compressible.map((source) => ({ source, tokens: perSourceTokens[source] ?? 0 })),
    },
  };
  writeFileSync(join(runtimeDir, "chapter-0003.context.json"), JSON.stringify(ctx3, null, 2));
  writeFileSync(join(runtimeDir, "chapter-0003.trace.json"), JSON.stringify(trace3, null, 2));
  console.log("[fixture] 已伪造第 3 章压缩留痕工件（压缩留痕分支走查数据）");
}

// ── 4. 断言 ──
const promises = await api(`/api/v1/books/${enc}/promises`);
const timeline = promises.body?.timeline ?? [];
const kinds = new Set(timeline.map((entry) => entry.kind).filter(Boolean));
const expectKinds = new Set(["suspense", "emotion", "artifact", "worldview"]);
const kindsOk = [...expectKinds].every((kind) => kinds.has(kind));

const runLog = (await api("/api/v1/run-log")).body;
// 442 号：复用根重启后引擎内存 run-log 从零——增长断言只约束同进程全链路径。
const runLogOk = chainRan ? (runLog.total > 0 && (runLog.entries ?? []).length > 0) : true;

console.log(`[fixture] promises timeline = ${timeline.length} 条，kinds = ${[...kinds].join(",") || "（无）"}`);
console.log(`[fixture] run-log total = ${runLog.total}，kept = ${runLog.kept}`);

let failed = false;
if (timeline.length === 0) {
  console.error("[fixture] ✗ promises timeline 断言失败（memory.db 投影重建缺失）");
  failed = true;
}
if (!runLogOk) {
  console.error("[fixture] ✗ run-log 断言失败（write-next 链未产生调用记录）");
  failed = true;
}
if (failed) process.exit(1);
// 回归哨兵（407 号已修）：settle 落盘投影 render_hooks_projection 现输出
// 第 14 列分类；若 kinds 再缺失即投影/落盘链回归。
if (!kindsOk) {
  console.warn(`[fixture] ⚠ promises kind 不全（${[...kinds].join(",") || "无"}）——投影/落盘链回归，请检查 render_hooks_projection 与台账写入面`);
}

console.log(`[fixture] ✓ 数据就绪。浏览器打开 ${base} 走查：`);
console.log(`  1. 书详情 ${base}/#/book/${enc} → 右栏「伏笔池」：分类 chips（全部/悬念/物品/情感/世界观）+ 点击筛选收窄`);
console.log(`  2. ${base}/#/book/${enc}/settings → 「承诺账本」：kind 徽标 + 分类过滤（open 置顶排序）`);
console.log(`  3. 同页顶部「运行遥测」：累计 ${runLog.total} · 明细 agent/model/耗时`);
console.log(`  4. ${base}/#/radar → 点击历史条目回放 → 推荐卡「从该选题开书」→ 跳建书且输入框预填`);
