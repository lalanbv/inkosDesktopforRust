#!/usr/bin/env node
// 双引擎 GET 契约活体差分（487 号）：
//   node scripts/engine-contract-diff.mjs [--rust-port 8901] [--node-port 8902] [--mock-port 1234]
//   node scripts/engine-contract-diff.mjs --probe "<path1,path2,...>"   # 候选端点探测（684 号）
//
// 同时起 Rust 引擎与 TS 回退端（各自独立临时根 + 同一 mock LLM），各跑一遍
// fixture 保证数据形态一致，然后对一组稳定 GET 端点做**归一化深比对**：
//   - 递归剪除易变键（时间戳/耗时/计数等内部实现差异）
//   - 键集合与剩余值必须逐字节一致
// 输出 DIVERGE 清单；退出码非零 = 存在契约漂移。供双引擎改动后回归。

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync, mkdirSync, openSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
// 643 号：rust 活体腿二进制新鲜度闸（与 gate:ts rust-bin 步骤同源实现）。
import { ensureRustBinFreshOrExit } from "./ensure-rust-bin.mjs";

// 515 号：本地引擎挂起时快速失败——统一 20s 超时（SSE 长连接除外）。
const fetchT = (input, init = {}) => fetch(input, { ...init, signal: AbortSignal.timeout(20_000) });


const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const studioDir = join(repoRoot, "packages", "studio");
const rustBinary = process.env.INKOS_SMOKE_RUST_BIN
  ?? join(repoRoot, "engine-rs", "target", "debug", "inkos-engine-server");

const args = process.argv.slice(2);
const argOf = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const rustPort = argOf("--rust-port", "8901");
const nodePort = argOf("--node-port", "8902");
const mockPort = argOf("--mock-port", "1234");
const keep = args.includes("--keep");
// 684 号：探测模式一等公民化——488/634 的「新增端点先活体探测双端 200 再入列」
// 原为临时手搓，现固化为 --probe "<path1,path2,...>"。起齐 mock+双引擎+fixture
// 后对候选 GET 逐个报状态码对与响应形状，不比对不计数；$BOOK=书名编码、
// $SESSION=双端各自会话清单首项 id。
const probePaths = argOf("--probe", null);
const PROBE_PATHS = probePaths
  ? probePaths.split(",").map((s) => s.trim()).filter(Boolean)
  : null;

/** 探测模式完成后的早退哨兵：finally 清理必须照常执行，故不能 process.exit。 */
class ProbeEarlyExit extends Error {}

// 643 号：差分器的存在意义就是双引擎对照——先过活体腿新鲜度闸（默认 debug
// 路径 cargo build 核验/重建；env 外部产物 517 形态仅核在，新鲜度调用方自管）。
// 二进制缺失时快速失败并给可行动指引（旧形态 spawn 失败后要等 30s 启动超时才崩）。
ensureRustBinFreshOrExit(repoRoot);
if (!existsSync(rustBinary)) {
  console.error(`[diff] ✗ Rust 二进制缺失（${rustBinary}）——先在 engine-rs 下 cargo build，或设 INKOS_SMOKE_RUST_BIN 指定外部产物`);
  process.exit(1);
}

const children = [];
const startChild = (cmd, cmdArgs, opts, logPath) => {
  const out = openSync(logPath, "a");
  const child = spawn(cmd, cmdArgs, { ...opts, stdio: ["ignore", out, out], detached: true });
  children.push(child);
  return child;
};
const waitUntil = async (fn, timeoutMs, label) => {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      if (await fn()) return true;
    } catch { /* not ready */ }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`等待超时：${label}`);
};
const apiFor = (base, root) => async (path, init) => {
  const res = await fetchT(base + path, init);
  const text = await res.text();
  // 根路径归一化：doctor 等端点会回显各自的临时根绝对路径。
  const normalized = root ? text.split(root).join("%ROOT%") : text;
  let body = null;
  try { body = JSON.parse(normalized); } catch { body = null; }
  return { status: res.status, body, text: normalized, contentType: res.headers.get("content-type") ?? "" };
};

/** 递归剪除易变键并做键排序规范化（保证两侧同构可比）。 */
const VOLATILE = new Set([
  "ts", "createdAt", "updatedAt", "timestamp", "durationMs", "elapsedMs",
  "lastAdvanced", "lastAdvancedChapter", "recordedAt", "registeredAt",
  "lastAppliedChapter", "chapterNumber", "nextChapter", "savedChapters",
  "total", "kept", "tookOverCount", "failureCount",
  // 684 号：interaction/session 惰性建会话，sessionId=创建时刻毫秒戳——
  // 双腿各自起引擎天然不同（run-6 活体捕获 1ms 之差假分叉）。
  "sessionId",
]);
const prune = (node) => {
  if (Array.isArray(node)) return node.map(prune);
  if (node && typeof node === "object") {
    const out = {};
    for (const key of Object.keys(node).sort()) {
      if (VOLATILE.has(key)) continue;
      out[key] = prune(node[key]);
    }
    return out;
  }
  return node;
};

/** 按路径模式剪除豁免子树（"*" 匹配任意单段；完整模式命中即整体剪除）。 */
const pruneWaivedPaths = (node, patterns) => {
  if (!patterns.length) return node;
  const walk = (value, seg) => {
    if (Array.isArray(value)) return value.map((item, i) => walk(item, [...seg, String(i)]));
    if (value && typeof value === "object") {
      const out = {};
      for (const key of Object.keys(value).sort()) {
        if (patterns.some((pattern) => pattern.length === seg.length + 1
          && (pattern[seg.length] === "*" || pattern[seg.length] === key))) {
          continue;
        }
        out[key] = walk(value[key], [...seg, key]);
      }
      return out;
    }
    return value;
  };
  return walk(node, []);
};

/** 返回首个分叉点的 JSON 指针（用于差分报告定位）。 */
const firstDiffPath = (a, b, path = "") => {
  if (a === b) return null;
  if (typeof a !== typeof b || a === null || b === null || Array.isArray(a) !== Array.isArray(b)) return path || "$";
  if (Array.isArray(a)) {
    if (a.length !== b.length) return `${path}（长度 ${a.length} vs ${b.length}）`;
    for (let i = 0; i < a.length; i += 1) {
      const hit = firstDiffPath(a[i], b[i], `${path}/${i}`);
      if (hit) return hit;
    }
    return null;
  }
  if (typeof a === "object") {
    for (const key of Object.keys(a).sort()) {
      const hit = firstDiffPath(a[key], b[key], `${path}/${key}`);
      if (hit) return hit;
    }
    return null;
  }
  return path || "$";
};

// 契约端点清单（GET；:id=镜花水月）。
const BOOK = encodeURIComponent("镜花水月");

/** 新增端点先经活体探测确认双端 200 再入列（488 号扩至 26 端点、634 号扩至 36）。
 *
 * 634 号探测备案（双端设计内超集，不入对照）：
 * - `/api/v1/writing-stats-rows`：仅 Rust 有——WritingStatsCard 在 TS 聚合面
 *   `/writing-stats` 不可达时的行级回退（组件注释明示），TS 404 是设计。
 * - `/api/v1/asset-library/:kind/assets` GET：Rust 有列表 GET；TS 同路径仅 PUT
 *   批量 upsert，列表走 `GET /api/v1/asset-library/:kind`——读面形态差异，
 *   前端各走各的等价读。
 */

/**
 * 已知分歧豁免表（487 号）：路径段数组，"*" 匹配任意单段；命中子树整体剪除。
 * 每条必须注明备案依据——豁免=已知双端行为差异，不是错误默认放行。
 */
const WAIVERS = new Map([
  // 519 号裁决：resync 端点双端幂等等价（活体精测），483 备案的 currentChapter
  // 分歧实为 Rust write-next 投影段时序缺陷（persist 前读旧 state）——修复后
  // promises/roster-candidates 豁免撤销。仅存 context-lens 装配留痕差异：
  // TS resync 产出 ch1 留痕而 Rust 不产（透明回放完整性，不影响写作数据）。
  [`/api/v1/books/${BOOK}/context-lens`, { reason: "519 号裁决备案：TS resync 产出 ch1 装配留痕而 Rust 不产（仅 lens 回放面）", paths: [["chapters"]] }],
  // 审计内部计数：双端机检维度实现差异，非契约面（issueCount 随审计轮次波动）
  [`/api/v1/books/${BOOK}/quality-trend`, { reason: "审计计数波动 + null-vs-缺键序列化（359 号先例）", paths: [["trend"]] }],
  // 章节审计 issue 明细随双端机检维度差异波动（种子内容分叉叙事已废：
  // 505 号活体证实三库种子归一后逐字节一致，产品决策需求撤销——567/572 号
  // 勘误链把本注释对齐 505 裁决）
  [`/api/v1/books/${BOOK}`, { reason: "审计 issue 明细随双端机检维度差异波动", paths: [["chapters", "*", "auditIssues"], ["nextChapter"]] }],
  // 488 号扩展腿发现（501/505 号已清偿：skills/genres 内置清单镜像缺口=环境
  // 伪象双端等价、三库种子归一后逐字节一致——豁免已撤销，留注释存史）：

  // doctor 回退端缺 retrieval.chunkCount 键——Rust 侧修复需 cargo
  [`/api/v1/doctor`, { reason: "回退端多 retrieval.chunkCount（Rust 侧补齐需 cargo）", paths: [["retrieval"]] }],
  // lens rank 打分内部实现差异（展示面）。
  [`/api/v1/books/${BOOK}/context-lens/2`, { reason: "resync 管线内部装配差异（483/484 备案）：entry source/rank 随内部实现波动", paths: [["entries"]] }],
  // 684 号：backend 是引擎身份标识（node-fallback vs rust-engine）、version
  // 是双端独立版本号（studio 1.8.0 vs engine 0.1.0）——均为设计内身份面。
  [`/api/v1/health`, { reason: "684 号备案：backend/version=双端身份标识（node-fallback vs rust-engine；1.8.0 vs 0.1.0）", paths: [["backend"], ["version"]] }],
  // 684 号：run-log entries 是 LLM 调用记账——双端管线编排实现差异（调用
  // 次数/agent 序非镜像，如 planner 重试编排不同）；产出等价由工件树差分 +
  // 导出 TEXT 面锁定，记账明细非契约面。
  [`/api/v1/run-log`, { reason: "684 号备案：LLM 调用记账随双端编排实现差异（次数/agent 序），产出等价由工件树差分锁定", paths: [["entries"]] }],
]);
const ENDPOINTS = [
  "/api/v1/books",
  `/api/v1/books/${BOOK}`,
  `/api/v1/books/${BOOK}/timeline`,
  `/api/v1/books/${BOOK}/promises`,
  `/api/v1/books/${BOOK}/quality-trend`,
  `/api/v1/books/${BOOK}/tension-curve`,
  // 520 号扩展（write-next 投影消费面回归）：
  `/api/v1/books/${BOOK}/quality-debts`,
  `/api/v1/books/${BOOK}/context-lens`,
  `/api/v1/books/${BOOK}/director`,
  `/api/v1/books/${BOOK}/codex`,
  `/api/v1/books/${BOOK}/anti-ai-rules`,
  `/api/v1/books/${BOOK}/experience`,
  "/api/v1/asset-library/genre-base",
  "/api/v1/task-routing",
  "/api/v1/project/notify",
  // 488 号扩展（活体探测双端 200）：
  "/api/v1/sessions",
  "/api/v1/translations",
  "/api/v1/skills",
  "/api/v1/style-profiles",
  "/api/v1/radar/history",
  "/api/v1/genres",
  "/api/v1/asset-library/progression-mode",
  "/api/v1/asset-library/world-sample",
  "/api/v1/doctor",
  `/api/v1/books/${BOOK}/chapters/2`,
  `/api/v1/books/${BOOK}/context-lens/2`,
  `/api/v1/books/${BOOK}/roster-candidates`,
  // 634 号扩容（488 号协议第二轮：活体探测双端 200 后入列）：
  "/api/v1/genres/cozy",
  "/api/v1/prompt-packs",
  `/api/v1/books/${BOOK}/chapters/2/workspace`,
  `/api/v1/books/${BOOK}/timeline-auto-beats`,
  `/api/v1/books/${BOOK}/chapter-review-mode`,
  `/api/v1/books/${BOOK}/series-id`,
  `/api/v1/books/${BOOK}/best-of-n`,
  `/api/v1/books/${BOOK}/style-binding`,
  `/api/v1/books/${BOOK}/detect/stats`,
  // 684 号扩容（488 号协议第三轮：--probe 活体探测双端 200 后入列；探测协议
  // 自本轮起一等公民化）。project/services/interaction/health 全家 + 书级
  // analytics/eval/truth/fanfic/series-backfill 读面。
  "/api/v1/health",
  "/api/v1/daemon",
  "/api/v1/run-log",
  "/api/v1/services",
  "/api/v1/services/config",
  "/api/v1/services/models",
  "/api/v1/services/models/custom",
  "/api/v1/project",
  "/api/v1/project/detection",
  "/api/v1/project/chapter-review-mode",
  "/api/v1/project/default-model",
  "/api/v1/project/model-overrides",
  "/api/v1/project/research-search",
  "/api/v1/cover/config",
  "/api/v1/interactive-films",
  "/api/v1/interaction/session",
  `/api/v1/books/${BOOK}/analytics`,
  `/api/v1/books/${BOOK}/eval`,
  `/api/v1/books/${BOOK}/truth`,
  `/api/v1/books/${BOOK}/truth/story_bible.md`,
  `/api/v1/books/${BOOK}/series-backfill/existing`,
  `/api/v1/books/${BOOK}/fanfic`,
  // 684 号：导出正文面（TEXT 模式）——txt 载荷 = 书名 + 章节原文拼接，
  // 双端夹具同源必须逐字节一致（epub/tar.gz 二进制容器不在此列，见
  // STATUS_ONLY）。
  `/api/v1/books/${BOOK}/export?format=txt`,
];

// 684 号：非 JSON 端点的两种弱比对模式。
// - TEXT：响应为纯文本契约面（导出正文）——归一化后逐字节比对，强于仅状态码；
// - STATUS_ONLY：响应为二进制容器（gzip/tar/epub），容器内部时间戳与压缩
//   字典序天然非确定，字节级比对不可行——锁状态码 + content-type 前缀。
const TEXT_ENDPOINTS = new Set([
  `/api/v1/books/${BOOK}/export?format=txt`,
]);
const STATUS_ONLY_ENDPOINTS = new Set([
  `/api/v1/backup/export`,
]);

const preseed = (root) => {
  mkdirSync(join(root, ".inkos"), { recursive: true });
  writeFileSync(
    join(root, "inkos.json"),
    JSON.stringify({ name: "inkos-contract-diff", version: "0.1.0", services: [{ service: "custom", name: "Mock", baseUrl: `http://127.0.0.1:${mockPort}/v1` }] }),
  );
  writeFileSync(join(root, ".inkos", "secrets.json"), JSON.stringify({ services: { "custom:Mock": { apiKey: "sk-mock" } } }));
};

// ── 1. mock + 双引擎 + 双根 ──
startChild("node", [join(scriptDir, "walkthrough-mock.mjs"), mockPort], { cwd: repoRoot }, join(tmpdir(), "inkos-diff-mock.log"));
await waitUntil(async () => (await fetchT(`http://127.0.0.1:${mockPort}/v1/models`)).ok, 15_000, "mock 启动");

const roots = {};
const engines = [];
{
  const root = mkdtempSync(join(tmpdir(), "inkos-diff-node-"));
  preseed(root);
  roots.node = root;
  startChild(
    process.execPath,
    [join(studioDir, "node_modules", "tsx", "dist", "cli.mjs"), join(studioDir, "src", "api", "index.ts"), root],
    { cwd: studioDir, env: { ...process.env, INKOS_STUDIO_PORT: nodePort } },
    join(root, "server.log"),
  );
  engines.push({ name: "node", port: nodePort, root });
}
{
  const root = mkdtempSync(join(tmpdir(), "inkos-diff-rust-"));
  preseed(root);
  roots.rust = root;
  startChild(
    rustBinary,
    [],
    {
      cwd: repoRoot,
      env: {
        ...process.env,
        INKOS_PORT: rustPort,
        INKOS_PROJECT_ROOT: root,
        INKOS_STATIC_DIR: join(studioDir, "dist"),
        INKOS_LLM_BASE_URL: `http://127.0.0.1:${mockPort}/v1`,
        // 内置题材目录缺省相对 CWD——编排 cwd=repoRoot 与桌面壳不同，
        // 显式钉到 fixture 预置的 assets 目录（488 号）。
          INKOS_BUILTIN_GENRES_DIR: join(repoRoot, "packages", "core", "genres"),
          INKOS_BUILTIN_SKILLS_DIR: join(repoRoot, "packages", "core", "skills"),
      },
    },
    join(root, "server.log"),
  );
  engines.push({ name: "rust", port: rustPort, root });
}

const makeEventCollector = (base) => {
  const names = new Set();
  const controller = new AbortController();
  const task = (async () => {
    try {
      const res = await fetch(`${base}/api/v1/events`, {
        headers: { Accept: "text/event-stream" },
        signal: controller.signal,
      });
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = "";
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        let idx;
        while ((idx = buf.indexOf("\n")) >= 0) {
          const line = buf.slice(0, idx);
          buf = buf.slice(idx + 1);
          if (line.startsWith("event:")) {
            const name = line.slice(6).trim();
            if (name && name !== "ping") names.add(name);
          }
        }
      }
    } catch {
      // 流中断 = 收集结束
    }
  })();
  return { names, stop: async () => { controller.abort(); await task.catch(() => {}); } };
}
try {
  for (const engine of engines) {
    const base = `http://127.0.0.1:${engine.port}`;
    await waitUntil(async () => (await fetchT(`${base}/api/v1/books`)).ok, 30_000, `${engine.name} 启动`);
  }

  // SSE 收集器：双引擎就绪后先挂流，再跑各腿 fixture（492 号活体事件差分）。
  const collectors = [
    { name: "node", ...makeEventCollector(`http://127.0.0.1:${nodePort}`) },
    { name: "rust", ...makeEventCollector(`http://127.0.0.1:${rustPort}`) },
  ];
  await new Promise((r) => setTimeout(r, 800));

  for (const engine of engines) {
    execFileSync("node", [join(scriptDir, "walkthrough-fixture.mjs"), engine.root, engine.port], {
      cwd: repoRoot,
      stdio: ["ignore", "ignore", "inherit"],
    });
  }

  // ── 1.5 探测模式（684 号）：候选端点双端状态码对，入列前置协议 ──
  if (PROBE_PATHS) {
    const resolvePath = async (port, raw) => {
      let p = raw.replaceAll("$BOOK", BOOK);
      if (p.includes("$SESSION")) {
        const res = await fetchT(`http://127.0.0.1:${port}/api/v1/sessions`);
        const body = await res.json().catch(() => null);
        const first = body?.sessions?.[0]?.id ?? body?.[0]?.id ?? null;
        if (!first) return null;
        p = p.replaceAll("$SESSION", encodeURIComponent(String(first)));
      }
      return p;
    };
    const shapeOf = (x) => {
      const text = x.text.split(roots.node).join("%ROOT%").split(roots.rust).join("%ROOT%");
      let b = null;
      try { b = JSON.parse(text); } catch { return `${x.ct} ${x.text.length}B 非JSON·首64=${text.slice(0, 64)}`; }
      if (Array.isArray(b)) return `JSON[${b.length}] 首=${JSON.stringify(b[0] ?? null).slice(0, 200)}`;
      return `JSON{${Object.keys(b).join(",")}}`;
    };
    console.log(`[probe] 探测 ${PROBE_PATHS.length} 个候选端点（状态码对 + 响应形状；入列先探测——634 号协议）`);
    for (const raw of PROBE_PATHS) {
      const np = await resolvePath(nodePort, raw);
      const rp = await resolvePath(rustPort, raw);
      if (!np || !rp) {
        console.log(`[probe] $SESSION-解析失败（会话清单空） ${raw}`);
        continue;
      }
      const [n, r] = await Promise.all([
        fetchT(`http://127.0.0.1:${nodePort}${np}`).then(async (res) => ({ status: res.status, ct: res.headers.get("content-type") ?? "", text: await res.text() })).catch((e) => ({ status: -1, ct: "", text: String(e) })),
        fetchT(`http://127.0.0.1:${rustPort}${rp}`).then(async (res) => ({ status: res.status, ct: res.headers.get("content-type") ?? "", text: await res.text() })).catch((e) => ({ status: -1, ct: "", text: String(e) })),
      ]);
      const verdict = n.status === r.status
        ? (n.status === 200 ? "BOTH-200 " : `BOTH-${n.status}`)
        : `✗DIVERGE node=${n.status} rust=${r.status}`;
      console.log(`[probe] ${verdict} ${raw}`);
      console.log(`    node : ${shapeOf(n)}`);
      console.log(`    rust : ${shapeOf(r)}`);
    }
    throw new ProbeEarlyExit();
  }

  // ── 2. SSE 事件面活体收集（492 号）──
;

// ── 3. 逐端点归一化深比对 ──
  const [nodeApi, rustApi] = [
    apiFor(`http://127.0.0.1:${nodePort}`, roots.node),
    apiFor(`http://127.0.0.1:${rustPort}`, roots.rust),
  ];
  let divergences = 0;
  let compared = 0;

  // ── 写后读对照（490 号）：481 号静态 body 审计的活体升级——变更端点
  // 双端写入同一载荷，断言状态码一致；持久化等价随后由 GET 差分面覆盖。
  const WRITE_SET = [
    ["/api/v1/task-routing", { routing: { defaults: { model: "lm-mock-model" }, tasks: { writing: { model: "m-w" } } } }],
    [`/api/v1/books/${BOOK}/codex`, { cards: [{ id: "card_wen", kind: "character", name: "苏檀", summary: "镜宗外门新晋弟子，随身碎镜。", facts: ["佩带母亲遗留碎镜"] }] }],
    [`/api/v1/books/${BOOK}/anti-ai-rules`, { rules: [{ id: "rule_probe", type: "phrase", pattern: "须发皆张", isRegex: false, severity: "warning", message: "避免使用陈词套语「须发皆张」。", enabled: true }] }],
    [`/api/v1/books/${BOOK}/experience`, { entries: [{ id: "exp_probe", chapter: 0, kind: "technique", text: "冷开场探针：环境先于人声。", enabled: true, createdAt: "2026-09-16T00:00:00.000Z" }] }],
    [`/api/v1/books/${BOOK}/timeline-auto-beats`, { enabled: true }],
    [`/api/v1/books/${BOOK}/best-of-n`, { enabled: true, candidates: 3, minScore: 70 }],
    [`/api/v1/books/${BOOK}/chapter-review-mode`, { mode: "manual" }],
    [`/api/v1/books/${BOOK}/series-id`, { seriesId: null }],
  ];
  // 错误面契约（495 号）：非法载荷双端一致 422——Rust typed-extractor 拒绝 /
  // TS 显式校验（此前 node=500 vs rust=422 分歧，已对齐）。
  const ERROR_FACE = [
    [`/api/v1/books/${BOOK}/experience`, { entries: [{ id: "bad_probe", kind: "technique" }] }],
  ];
  for (const [path, body] of ERROR_FACE) {
    const payload = JSON.stringify(body);
    const nodeStatus = (await nodeApi(path, { method: "PUT", headers: { "Content-Type": "application/json" }, body: payload })).status;
    const rustStatus = (await rustApi(path, { method: "PUT", headers: { "Content-Type": "application/json" }, body: payload })).status;
    compared += 1;
    if (nodeStatus === rustStatus && nodeStatus === 422) {
      console.log(`✓ PUT ${path} 非法载荷双端 422`);
    } else {
      divergences += 1;
      console.log(`✗ PUT ${path} 非法载荷：node=${nodeStatus} rust=${rustStatus}（期望双端 422）`);
    }
  }

  // ── DELETE 面对照（502 号）：单条删除后读面等价 ──
  // experience：PUT 两条 → DELETE 一条 → 双端 GET 均只剩保留条；
  const expPath = `/api/v1/books/${BOOK}/experience`;
  const expEntries = {
    entries: [
      { id: "exp_del", chapter: 0, kind: "technique", text: "待删除探针条目。", enabled: true, createdAt: "2026-09-16T00:00:00.000Z" },
      { id: "exp_keep", chapter: 0, kind: "hook", text: "保留探针条目：章末钩回落。", enabled: true, createdAt: "2026-09-16T00:00:00.000Z" },
    ],
  };
  for (const api of [nodeApi, rustApi]) {
    await api(expPath, { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify(expEntries) });
  }
  const delExp = async (base) => {
    const res = await fetchT(`${base}${expPath}/exp_del`, { method: "DELETE" });
    return res.status;
  };
  compared += 1;
  {
    const s1 = await delExp(`http://127.0.0.1:${nodePort}`);
    const s2 = await delExp(`http://127.0.0.1:${rustPort}`);
    if (s1 === s2 && s1 < 400) {
      console.log(`✓ DELETE ${expPath}/exp_del（双端 ${s1}）`);
    } else {
      divergences += 1;
      console.log(`✗ DELETE ${expPath}/exp_del：node=${s1} rust=${s2}`);
    }
  }

  // asset-library：PUT 单资产 → DELETE → 双端 GET 均回到种子态
  const assetPath = "/api/v1/asset-library/world-sample/assets";
  const probeAsset = { asset: { id: "probe_ws", kind: "world-sample", name: "差分探针世界样本", body: "探针正文", expectations: [], taboos: [], samples: [] } };
  for (const api of [nodeApi, rustApi]) {
    await api(assetPath, { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify(probeAsset) });
  }
  {
    const s1 = (await fetchT(`http://127.0.0.1:${nodePort}${assetPath}/probe_ws`, { method: "DELETE" })).status;
    const s2 = (await fetchT(`http://127.0.0.1:${rustPort}${assetPath}/probe_ws`, { method: "DELETE" })).status;
    compared += 1;
    if (s1 === s2 && s1 < 400) {
      console.log(`✓ DELETE ${assetPath}/probe_ws（双端 ${s1}）`);
    } else {
      divergences += 1;
      console.log(`✗ DELETE ${assetPath}/probe_ws：node=${s1} rust=${s2}`);
    }
  }

  for (const [path, body] of WRITE_SET) {
    const payload = JSON.stringify(body);
    const nodeStatus = (await nodeApi(path, { method: "PUT", headers: { "Content-Type": "application/json" }, body: payload })).status;
    const rustStatus = (await rustApi(path, { method: "PUT", headers: { "Content-Type": "application/json" }, body: payload })).status;
    compared += 1;
    if (nodeStatus === rustStatus && nodeStatus < 400) {
      console.log(`✓ PUT ${path}（${nodeStatus}）`);
    } else {
      divergences += 1;
      console.log(`✗ PUT ${path}：状态码分歧 node=${nodeStatus} rust=${rustStatus}`);
    }
  }

  for (const endpoint of ENDPOINTS) {
    const nodeRes = await nodeApi(endpoint);
    const rustRes = await rustApi(endpoint);
    if (nodeRes.status === 404 && rustRes.status === 404) {
      console.log(`- ${endpoint}：双端 404，跳过`);
      continue;
    }
    if (nodeRes.status !== rustRes.status) {
      console.log(`✗ ${endpoint}：状态码分歧 node=${nodeRes.status} rust=${rustRes.status}`);
      divergences += 1;
      compared += 1;
      continue;
    }
    // 684 号：文本契约面——归一化换行后逐字节比对（导出正文双端夹具同源）。
    if (TEXT_ENDPOINTS.has(endpoint)) {
      compared += 1;
      const a = nodeRes.text.replace(/\r\n/g, "\n").trimEnd();
      const b = rustRes.text.replace(/\r\n/g, "\n").trimEnd();
      if (a === b) {
        console.log(`✓ TEXT ${endpoint}（${nodeRes.status}，${a.length}B 逐字节一致）`);
      } else {
        divergences += 1;
        const la = a.split("\n");
        const lb = b.split("\n");
        const at = la.findIndex((line, i) => line !== lb[i]);
        console.log(`✗ TEXT ${endpoint}：首处分叉行 #${at + 1}`);
        console.log(`    node=${JSON.stringify(la[at] ?? "").slice(0, 200)}`);
        console.log(`    rust=${JSON.stringify(lb[at] ?? "").slice(0, 200)}`);
      }
      continue;
    }
    // 684 号：二进制容器面——只锁状态码 + content-type（gzip/tar 内部时间戳
    // 与压缩序天然非确定，字节级比对不可行；持久化内容等价由工件树差分覆盖）。
    if (STATUS_ONLY_ENDPOINTS.has(endpoint)) {
      compared += 1;
      const ct = (r) => (r.contentType ?? "").split(";")[0].trim();
      const nonEmpty = nodeRes.text.length > 0 && rustRes.text.length > 0;
      if (ct(nodeRes) === ct(rustRes) && nonEmpty) {
        console.log(`✓ STATUS-ONLY ${endpoint}（${nodeRes.status}，ct=${ct(nodeRes)}，bytes node=${nodeRes.text.length}/rust=${rustRes.text.length}）`);
      } else {
        divergences += 1;
        console.log(`✗ STATUS-ONLY ${endpoint}：node=${nodeRes.status}/${ct(nodeRes)} rust=${rustRes.status}/${ct(rustRes)} nonEmpty=${nonEmpty}`);
      }
      continue;
    }
    // asset-library 双端数组排序语义不同（node 按 id、rust 按定义序）——按 id 归一。
    let nodeBody = nodeRes.body;
    let rustBody = rustRes.body;
    if (endpoint.startsWith("/api/v1/asset-library/")) {
      const sortAssets = (o) => {
        if (o && Array.isArray(o.assets)) {
          return { ...o, assets: [...o.assets].sort((x, y) => String(x.id).localeCompare(String(y.id))) };
        }
        return o;
      };
      nodeBody = sortAssets(nodeBody);
      rustBody = sortAssets(rustBody);
    }
    // 684 号：truth 列表面归一化——①`runtime/` 工作产物为 node-only 备案
    // 不对称（工件树差分同源备案）②`.json` 条目 preview 是 200 字符截断的
    // 内部持久化原文：序列化风格（键序/默认键/信封）双端不同且截断后不可
    // 语义解析——内容等价由各自 API 面端点（anti-ai-rules/codex 等）锁定，
    // 此处只比文件名在场性 ③md/yaml 条目 name+size+preview 全量比对
    // ④readdir 序不稳定，按 name 排序后比对。
    if (endpoint === `/api/v1/books/${BOOK}/truth`) {
      const normalizeTruth = (o) => ({
        files: (o?.files ?? [])
          .filter((f) => !String(f.name).startsWith("runtime/"))
          .map((f) => (String(f.name).endsWith(".json") ? { name: f.name } : f))
          .sort((x, y) => String(x.name).localeCompare(String(y.name))),
      });
      nodeBody = normalizeTruth(nodeBody);
      rustBody = normalizeTruth(rustBody);
    }
    const waiver = WAIVERS.get(endpoint);
    const aNode = pruneWaivedPaths(prune(nodeBody), waiver?.paths ?? []);
    const aRust = pruneWaivedPaths(prune(rustBody), waiver?.paths ?? []);
    const a = JSON.stringify(aNode);
    const b = JSON.stringify(aRust);
    compared += 1;
    if (a === b) {
      console.log(`✓ ${endpoint}`);
    } else {
      divergences += 1;
      console.log(`✗ ${endpoint}：归一化后不一致`);
      console.log(`    首个分叉：${firstDiffPath(aNode, aRust) ?? "?"}`);
      console.log(`    node=${a.slice(0, 260)}`);
      console.log(`    rust=${b.slice(0, 260)}`);
    }
  }
  // ── resync POST 活体对照（518 号：483 架构分歧备案的差分器实证面）──
  // fixture 已跑过一次 resync/1（walkthrough-fixture 步骤 3），此处第二次 POST
  // 兼职验证幂等性。归一化：auditResult.summary 文案双端各自实现（剥）；
  // tokenUsage / lengthTelemetry 保存在性、剥 LLM 计数内部值（prompt 模板
  // 毫级差异会进计数）；contextTrace 为 TS 独有字段（剥）。
  {
    // 探针钉最新持久化章（fixture write-next 已生成 ch2）：TS 侧有
      // "仅最新章可同步"守卫，resync 非 latest 直接 500——探针走 latest 面。
      const resyncPath = `/api/v1/books/${BOOK}/resync/2`;
    const nodeRes = await nodeApi(resyncPath, { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" });
    const rustRes = await rustApi(resyncPath, { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" });
    compared += 1;
    if (nodeRes.status !== rustRes.status || nodeRes.status >= 400) {
      divergences += 1;
      console.log(`✗ POST ${resyncPath}：状态码 node=${nodeRes.status} rust=${rustRes.status}`);
      if (nodeRes.body && typeof nodeRes.body === "object") console.log(`    node body=${JSON.stringify(nodeRes.body).slice(0, 300)}`);
      if (rustRes.body && typeof rustRes.body === "object") console.log(`    rust body=${JSON.stringify(rustRes.body).slice(0, 300)}`);
    } else {
      const presence = (v) => (v === undefined || v === null ? null : "object");
      const normalize = (o) => {
        if (!o || typeof o !== "object") return o;
        const { tokenUsage, lengthTelemetry, contextTrace, auditResult, ...rest } = o;
        return {
          ...rest,
          auditResult: auditResult && typeof auditResult === "object"
            ? { passed: auditResult.passed ?? null, issues: auditResult.issues ?? [] }
            : presence(auditResult),
          tokenUsage: presence(tokenUsage),
          lengthTelemetry: presence(lengthTelemetry),
        };
      };
      // prune：键序归一 + VOLATILE 剥离——双端序列化键序不同（node 契约序
      // vs rust 结构体序），内容一致时裸 stringify 也会假分叉。
      const a = JSON.stringify(prune(normalize(nodeRes.body)));
      const b = JSON.stringify(prune(normalize(rustRes.body)));
      if (a === b) {
        console.log(`✓ POST ${resyncPath}（${nodeRes.status}，响应形状归一后一致；二次调用幂等）`);
      } else {
        divergences += 1;
        console.log(`✗ POST ${resyncPath}：归一化后不一致`);
        console.log(`    首个分叉：${firstDiffPath(normalize(nodeRes.body), normalize(rustRes.body)) ?? "?"}`);
        console.log(`    node=${a.slice(0, 300)}`);
        console.log(`    rust=${b.slice(0, 300)}`);
      }
      // 豁免面缩小复核：resync 关联三端点做无豁免裸比对（信息输出，不计数）。
      // promises/roster-candidates 豁免已随 519 修复撤销；context-lens 仍按
      // 519 裁决备案装配留痕差异（chapters 长度分叉为预期）。
      for (const endpoint of [`/api/v1/books/${BOOK}/promises`, `/api/v1/books/${BOOK}/context-lens`, `/api/v1/books/${BOOK}/roster-candidates`]) {
        const n = await nodeApi(endpoint);
        const r = await rustApi(endpoint);
        const an = JSON.stringify(prune(n.body));
        const br = JSON.stringify(prune(r.body));
        if (an === br) {
          console.log(`[diff] 豁免复核 ${endpoint}：裸比对一致 ✓`);
        } else {
          console.log(`[diff] 豁免复核 ${endpoint}：无豁免 ✗ 首个分叉：${firstDiffPath(prune(n.body), prune(r.body)) ?? "?"}`);
        }
      }
    }
  }

  // ── hybrid-search POST 活体对照（520 号：G1/349 混合召回投影消费面）──
  // 无 embedding 配置 → 双端 mode=fts5-fallback；BM25/RRF 分数为浮点，
  // 归一化仅比 id 序与 mode（分数面受实现精度影响，非契约）。
  {
    const searchPath = `/api/v1/books/${BOOK}/hybrid-search`;
    const searchBody = JSON.stringify({ query: "苏檀 碎镜", k: 5 });
    const nodeRes = await nodeApi(searchPath, { method: "POST", headers: { "Content-Type": "application/json" }, body: searchBody });
    const rustRes = await rustApi(searchPath, { method: "POST", headers: { "Content-Type": "application/json" }, body: searchBody });
    compared += 1;
    // 520 号：BM25 打分/次序随双端分词实现波动（lens rank 同型备案）——
    // 契约面 = mode + top 命中 + 双端交集（排序无关）；各自边缘命中输出观察。
    const idsOf = (o) => (o?.fts ?? []).map((h) => h.id);
    const nodeIds = idsOf(nodeRes.body);
    const rustIds = idsOf(rustRes.body);
    const a = JSON.stringify({ mode: nodeRes.body?.mode, top: nodeIds[0] ?? null, inter: nodeIds.filter((id) => rustIds.includes(id)).sort() });
    const b = JSON.stringify({ mode: rustRes.body?.mode, top: rustIds[0] ?? null, inter: rustIds.filter((id) => nodeIds.includes(id)).sort() });
    const onlyNode = nodeIds.filter((id) => !rustIds.includes(id));
    const onlyRust = rustIds.filter((id) => !nodeIds.includes(id));
    if (onlyNode.length || onlyRust.length) {
      console.log(`[diff] hybrid-search 边缘命中（BM25 排序抖动备案）：仅 node=[${onlyNode}] 仅 rust=[${onlyRust}]`);
    }
    console.log(`[diff][hs] a=${a}`);
    console.log(`[diff][hs] b=${b}`);
    // 528 号：BM25 分数绝对值观察（非契约，永久豁免定性）。双端公式同源
    // （FTS5 bm25(fts, 5.0, 1.0) 同参数），活体实测双端分差呈**恒定比例因子**
    // （node/rust ≈ 1.1646，各命中一致）——单一全局常数差（SQLite 版本间 bm25
    // 内部常量），单调缩放不影响排序与交集；若 df/avgdl 统计不同则各命中比例
    // 会发散，实测不发散即排除。token 序 CLDR 微差由 521 golden 向量锁主流面。
    // 排序一致性由交集契约覆盖；此处仅观察输出，不计数。
    const scoresOf = (o) => JSON.stringify((o?.fts ?? []).map((h) => [h.id, Number((h.score ?? 0).toFixed(4))]));
    console.log(`[diff][hs] score node=${scoresOf(nodeRes.body)} rust=${scoresOf(rustRes.body)}`);
    if (nodeRes.status === rustRes.status && nodeRes.status < 400 && a === b) {
      console.log(`✓ POST ${searchPath}（${nodeRes.status}，mode=${nodeRes.body?.mode}，fts 命中序一致）`);
    } else {
      divergences += 1;
      console.log(`✗ POST ${searchPath}：node=${nodeRes.status} rust=${rustRes.status}`);
      console.log(`    node=${JSON.stringify(nodeRes.body)?.slice(0, 240)}`);
      console.log(`    rust=${JSON.stringify(rustRes.body)?.slice(0, 240)}`);
    }
  }

  // ── 落盘工件内容面对照（522 号：API 响应面之下的内容面）──
  // mock 确定性下双端正文/真相/结构化 state 应一致；memory.db（二进制+WAL 态）、
  // audit_drift（时间线敏感）、runtime 留痕（trace/run/context 含耗时与时戳）豁免。
  {
    const bookDirOf = (root) => join(root, "books", decodeURIComponent(BOOK));
    const isVolatile = (rel) =>
      rel.startsWith("story/memory.db")
      || rel === "story/audit_drift.md"
      || rel.endsWith(".trace.json")
      || rel.endsWith(".run.json")
      || rel.endsWith(".context.json");
    const walkFiles = (dir, prefix = "") => {
      const out = [];
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
        if (entry.isDirectory()) out.push(...walkFiles(join(dir, entry.name), rel));
        else out.push(rel);
      }
      return out.sort();
    };
    const sortKeys = (v) => (Array.isArray(v) ? v.map(sortKeys)
      : v && typeof v === "object" ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, sortKeys(v[k])]))
      : v);
    // 525 号：非契约噪声归一。
    // ① runtime plan/intent 里的绝对 root 路径（双腿 tmp 目录天然不同）；
    // ② yaml 行首缩进（TS yaml 序列化两空格 vs serde_yaml 顶层列表——语义等价）。
    const stripVolatileText = (t, root) => t.split(root).join("<root>").split(tmpdir()).join("<tmp>")
      .split("\r\n").join("\n")
      .split("\n").map((l) => l.trim()).filter((l) => l.length > 0).join("\n");
    // 527 号裁决：内容备案清单全量清偿——522 号建立时 7 项备案（尾换行装置
    // 形态/settle tension 注入列/plan 措辞/rule-stack 序列化格式/status_raw
    // 形状），经 523–526 号修复与归一化后活体实测命中归零，清单出清；此后
    // 任何工件内容分歧（归一化后）直接计入 divergences 硬拦截，新分歧原则
    // 上定性修复，不再新增备案。
    const ALLOWED_CONTENT_DIFFS = new Set([]);
    // 527 号裁决（永久备案）：resync 留痕三件套仅 TS 侧产出。intent/plan/
    // rule-stack 在 TS 是 resolveGovernedPlan 的 planner memo 化缓存+人类可读
    // 留痕；Rust write-next 主链同等 memo 已存在（write_next.rs
    // load_persisted_plan 复用跳过 planner LLM），缺失仅影响 resync 后首章
    // write-next 多一次 planner 调用（成本面，非正确性面）+回放留痕缺失。
    // 补齐须给 Rust resync 直写链加 governed plan 阶段=改变 resync 的 LLM
    // 调用面，519 号已裁决 resync 架构维持现状——故永久备案，不清偿。
    const ALLOWED_ONLY_NODE = new Set([
      "story/runtime/chapter-0001.intent.md",
      "story/runtime/chapter-0001.plan.md",
      "story/runtime/chapter-0001.rule-stack.yaml",
    ]);
    const nodeFiles = walkFiles(bookDirOf(roots.node));
    const rustFiles = walkFiles(bookDirOf(roots.rust));
    const nodeCore = nodeFiles.filter((f) => !isVolatile(f));
    const rustCore = rustFiles.filter((f) => !isVolatile(f));
    const nodeSet = new Set(nodeCore);
    const rustSet = new Set(rustCore);
    const onlyNode = nodeCore.filter((f) => !rustSet.has(f));
    const onlyRust = rustCore.filter((f) => !nodeSet.has(f));
    compared += 1;
    // 527 号起工件对照全量硬门禁：内容分歧零备案（清单已清偿出清）；
    // 仅 resync 留痕三件套按 527 裁决永久备案（仅 node 存在性，见上）。
    const unknownOnlyNode = onlyNode.filter((f) => !ALLOWED_ONLY_NODE.has(f));
    const unknownOnlyRust = onlyRust.filter((f) => !ALLOWED_ONLY_NODE.has(f));
    if (unknownOnlyNode.length || unknownOnlyRust.length) {
      divergences += unknownOnlyNode.length + unknownOnlyRust.length;
      console.log(`✗ 落盘工件树（未备案）：仅 node=[${unknownOnlyNode}] 仅 rust=[${unknownOnlyRust}]`);
    } else if (onlyNode.length || onlyRust.length) {
      console.log(`[diff] 落盘工件树（备案）：仅 node=[${onlyNode}] 仅 rust=[${onlyRust}]`);
    } else {
      console.log(`✓ 落盘工件树存在性（${nodeCore.length} 个内容工件双端一致）`);
    }
    let contentDiffs = 0;
    let unexpected = 0;
    for (const rel of nodeCore.filter((f) => rustSet.has(f))) {
      let a = readFileSync(join(bookDirOf(roots.node), rel), "utf-8");
      let b = readFileSync(join(bookDirOf(roots.rust), rel), "utf-8");
      if (rel.endsWith(".json")) {
        // json 归一化：键序差异非契约（TS 对象序 vs serde 结构体序），缺键/值差异才是；
        // ISO 时间戳值归一（双腿独立起引擎，写入时刻天然不同，524 号）。
        const TS_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$/;
        const stamp = (v) => (typeof v === "string" && TS_RE.test(v) ? "<ts>" : v);
        const norm = (t) => JSON.stringify(sortKeys(JSON.parse(t)), (k, v) => stamp(v));
        try {
          a = norm(a);
          b = norm(b);
        } catch { /* 非法 json 保持裸文本比 */ }
      } else if (rel.endsWith(".md") || rel.endsWith(".yaml")) {
        a = stripVolatileText(a, roots.node);
        b = stripVolatileText(b, roots.rust);
      }
      if (a !== b) {
        if (ALLOWED_CONTENT_DIFFS.has(rel)) {
          contentDiffs += 1;
          console.log(`[diff] 工件内容分歧（备案）：${rel}`);
        } else {
          unexpected += 1;
          console.log(`✗ 工件内容分歧（未备案）：${rel}（node ${a.length}B / rust ${b.length}B）`);
        }
      }
    }
    if (contentDiffs > 0) {
      console.log(`[diff] 工件内容分歧共 ${contentDiffs} 个（备案清单内）`);
    } else {
      console.log(`✓ 落盘工件内容一致（归一化后；零备案硬门禁）`);
    }
    if (unexpected > 0) {
      divergences += unexpected;
      console.log(`✗ 未备案工件分歧 ${unexpected} 个——先定性修复（527 起内容面零备案）`);
    }
  }

  // 确定性广播触发：directions 端点必广播 director:start + complete|error。
  const trigger = apiFor("", "");
  const triggers = engines.map((engine) =>
    fetch(`http://127.0.0.1:${engine.port}/api/v1/director/directions`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ inspiration: { premise: "SSE 差分探针。", keywords: [] }, count: 2 }),
    }).then((r) => r.text()).catch(() => "fetch-error"),
  );
  await Promise.all(triggers);
  // 宽限期：node 侧 LLM 失败重试/回包可能迟于 rust（492 号观察），留 3s 再停表。
  await new Promise((r) => setTimeout(r, 3000));
  for (const collector of collectors) await collector.stop();

  // ── SSE 事件名集合比对（416 号静态对照的活体升级）──
  const [nodeCol, rustCol] = collectors;
  // 终态归一化（492 号）：directions 触发在双端必有且仅有一个终态事件，
  // 但容错口径不同——TS 对 mock 的 propose 兜底响应报 director:error，
  // Rust 容错空结果报 director:complete。等价类归并后再比集合。
  const normalize = (names) => new Set([...names].map((n) => (n === "director:complete" || n === "director:error" ? "director:terminal" : n)));
  const nodeNames = normalize(nodeCol.names);
  const rustNames = normalize(rustCol.names);
  const onlyNode = [...nodeNames].filter((n) => !rustNames.has(n));
  const onlyRust = [...rustNames].filter((n) => !nodeNames.has(n));
  console.log(`[diff] SSE 事件数：node=${nodeNames.size} rust=${rustNames.size}（director 终态归一等价）`);
  console.log(`[diff] node 事件：${[...nodeCol.names].sort().join(", ")}`);
  console.log(`[diff] rust 事件：${[...rustCol.names].sort().join(", ")}`);
  if (onlyNode.length) console.log(`✗ 仅 node 广播：${onlyNode.join(", ")}`);
  if (onlyRust.length) console.log(`✗ 仅 rust 广播：${onlyRust.join(", ")}`);
  if (onlyNode.length || onlyRust.length) divergences += 1;
  else console.log(`✓ SSE 事件名集合双端一致`);

  // ── 工具目录面差分（R38b/544 号施工图 §2.3；555 号转硬对照）──────────────
  // 双端 debug/tools 裸数组同形（条目 {name, description, parametersSha256}，
  // camelCase；Rust 侧 serde rename_all 550 号潜伏 bug 已修）。对照规则：
  // rust-only 必须空集（硬要求）；node-only 对照豁免表（设计内不对称备案）；
  // 交集逐件 description 逐字 + parametersSha256 相等 + 两键类型守卫。
  {
    // node-only 13 名（555 号活体实测实名；设计内不对称：Rust 不作为 agent
    // 工具注册——agent_route.rs 备案「Rust 走 propose→confirm + 端点」）。逐组 rationale：
    // - 四建书件：chat 会话确认意图一次性件，Rust 侧端点承载（354 号链）；
    // - 六一次性生产件：short/script/storyboard/interactive_film 确认意图件+
    //   短篇翻译+play_start，Rust 侧 play/production 端点承载；
    // - film 三一次性件：draft_structure/connect_choice/remove_node 确认意图
    //   直执件，Rust 侧 film 端点承载。
    const TOOL_CATALOG_ONLY_NODE = new Set([
      "fanfic_create", "continuation_import", "spinoff_create", "imitation_create",
      "short_fiction_run", "translation_create", "script_create",
      "storyboard_create", "interactive_film_create", "play_start",
      "draft_structure", "connect_choice", "remove_node",
    ]);
    // 哈希豁免单件（89 号偏差备案）：generate_cover 的 coverBaseUrl/Endpoint/
    // Model/Size/ApiKeyEnv 五键 Rust 链不支持（Rust json! 刻意未列入，book_edit_
    // tools.rs 备案注释），TS 侧在列——schema 差异是行为面真实缺口的忠实投影，
    // 补齐方向=Rust cover 覆盖链实现后出清。description 仍硬对照。
    const TOOL_CATALOG_HASH_EXEMPT = new Set(["generate_cover"]);
    const toolsOf = async (base) => {
      try {
        const res = await fetchT(`${base}/api/v1/debug/tools`);
        if (!res.ok) return null;
        return await res.json();
      } catch {
        return null;
      }
    };
    const [nodeTools, rustTools] = await Promise.all([
      toolsOf(`http://127.0.0.1:${engines[0].port}`),
      toolsOf(`http://127.0.0.1:${engines[1].port}`),
    ]);
    compared += 1;
    if (!Array.isArray(nodeTools) || !Array.isArray(rustTools)) {
      divergences += 1;
      console.log(`✗ 工具目录：双端 debug/tools 必须在场（node=${Array.isArray(nodeTools)} rust=${Array.isArray(rustTools)}）`);
    } else {
      const byName = (list) => new Map(list.map((t) => [t.name, t]));
      const a = byName(nodeTools);
      const b = byName(rustTools);
      const onlyA = [...a.keys()].filter((n) => !b.has(n));
      const onlyB = [...b.keys()].filter((n) => !a.has(n));
      const unknownOnlyNode = onlyA.filter((n) => !TOOL_CATALOG_ONLY_NODE.has(n));
      const malformed = [...a.values(), ...b.values()].filter((t) =>
        typeof t?.name !== "string" || typeof t?.description !== "string"
        || typeof t?.parametersSha256 !== "string" || t.parametersSha256.length !== 64);
      const mismatches = [...a.keys()].filter((n) => b.has(n)
        && (a.get(n).description !== b.get(n).description
          || (!TOOL_CATALOG_HASH_EXEMPT.has(n)
            && a.get(n).parametersSha256 !== b.get(n).parametersSha256)));
      const allowedOnlyNode = onlyA.filter((n) => TOOL_CATALOG_ONLY_NODE.has(n));
      if (onlyB.length || unknownOnlyNode.length || mismatches.length || malformed.length) {
        divergences += 1;
        if (onlyB.length) console.log(`✗ 工具目录仅 rust（硬要求空集）：[${onlyB}]`);
        if (unknownOnlyNode.length) console.log(`✗ 工具目录仅 node（未备案）：[${unknownOnlyNode}]`);
        if (mismatches.length) console.log(`✗ 工具目录漂移（description/parametersSha256）：[${mismatches}]`);
        if (malformed.length) console.log(`✗ 工具目录条目形状非法：${malformed.length} 件`);
      } else {
        const intersect = a.size - allowedOnlyNode.length;
        console.log(`✓ 工具目录双端一致（交集 ${intersect} 件逐件比对 + node-only 备案 ${allowedOnlyNode.length} 件）`);
      }
    }
  }

  // ── 会话事件日志面（R43/565 号）：双腿各建一 chat 会话 + 驱动一轮 agent
  // （mock 确定性纯文本回复——聊天 system 提示词含「同人」关键词走 CANON
  // 分支，无工具调用），读双腿 {root}/.inkos/sessions/{id}.jsonl 逐行取
  // type 比对请求族事件计数；再 GET 会话 derive 读面深比对。R43 写前落盘
  // 时序（request_started+user 先于 LLM 请求）下事件族双端同构。
  // session_metadata_updated 豁免：标题生成 prompt 双端未 golden 锁定，
  // mock 分支可能分歧（备案）。
  {
    const familyTypes = ["session_created", "request_started", "message", "request_committed", "request_failed"];
    const chatSession = {};
    for (const engine of engines) {
      const base = `http://127.0.0.1:${engine.port}`;
      let sessionId = null;
      try {
        const created = await fetchT(`${base}/api/v1/sessions`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ sessionKind: "chat" }),
        });
        sessionId = (await created.json())?.session?.sessionId ?? null;
        if (created.ok && sessionId) {
          const drive = await fetchT(`${base}/api/v1/agent`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              instruction: "你好，介绍一下这个项目。",
              sessionId,
              sessionKind: "chat",
            }),
          });
          if (!drive.ok) {
            console.log(`✗ 会话日志面：${engine.name} 聊天驱动失败 status=${drive.status}`);
            sessionId = null;
          }
        } else {
          console.log(`✗ 会话日志面：${engine.name} 建会话失败 status=${created.status}`);
        }
      } catch (error) {
        console.log(`✗ 会话日志面：${engine.name} 驱动异常：${error?.message ?? error}`);
        sessionId = null;
      }
      if (sessionId) chatSession[engine.name] = sessionId;
    }
    if (chatSession.node && chatSession.rust) {
      const legCounts = {};
      for (const engine of engines) {
        const counts = {};
        const file = join(engine.root, ".inkos", "sessions", `${chatSession[engine.name]}.jsonl`);
        try {
          for (const line of readFileSync(file, "utf-8").split(/\r?\n/)) {
            if (!line.trim()) continue;
            const type = JSON.parse(line)?.type;
            if (typeof type === "string") counts[type] = (counts[type] ?? 0) + 1;
          }
        } catch (error) {
          console.log(`✗ 会话日志面：${engine.name} 读取事件日志失败：${error?.message ?? error}`);
        }
        legCounts[engine.name] = counts;
      }
      const project = (counts) => Object.fromEntries(familyTypes.map((type) => [type, counts[type] ?? 0]));
      compared += 1;
      const nodeProjected = project(legCounts.node ?? {});
      const rustProjected = project(legCounts.rust ?? {});
      if (JSON.stringify(nodeProjected) === JSON.stringify(rustProjected)) {
        const total = familyTypes.reduce((sum, type) => sum + rustProjected[type], 0);
        console.log(`✓ 会话事件日志面双端一致（请求族 ${total} 事件逐类型计数相同 ${JSON.stringify(rustProjected)}；metadata_updated 豁免备案）`);
      } else {
        divergences += 1;
        console.log(`✗ 会话事件日志面分歧：node=${JSON.stringify(nodeProjected)} rust=${JSON.stringify(rustProjected)}`);
      }

      // derive 读面活体对照：GET /api/v1/sessions/:id 深比对（身份/易变键
      // 剪除后键集合+值逐字节）。
      const SESSION_IDENTITY = new Set(["sessionId", "id", "uuid"]);
      const pruneSession = (node) => {
        if (Array.isArray(node)) return node.map(pruneSession);
        if (node && typeof node === "object") {
          const out = {};
          for (const key of Object.keys(node).sort()) {
            if (VOLATILE.has(key) || SESSION_IDENTITY.has(key)) continue;
            out[key] = pruneSession(node[key]);
          }
          return out;
        }
        return node;
      };
      const sessionOf = async (engine) => {
        const res = await fetchT(`http://127.0.0.1:${engine.port}/api/v1/sessions/${chatSession[engine.name]}`);
        return res.ok ? res.json().catch(() => null) : null;
      };
      const [nodeSession, rustSession] = await Promise.all([sessionOf(engines[0]), sessionOf(engines[1])]);
      compared += 1;
      if (!nodeSession || !rustSession) {
        divergences += 1;
        console.log(`✗ 会话 derive 读面：双端 GET /sessions/:id 必须在场`);
      } else {
        const diffPath = firstDiffPath(pruneSession(nodeSession), pruneSession(rustSession));
        if (diffPath) {
          divergences += 1;
          console.log(`✗ 会话 derive 读面分歧 @ ${diffPath}`);
        } else {
          console.log(`✓ 会话 derive 读面双端一致（深比对含消息角色/内容/工具卡）`);
        }
      }

      // ── R36 branch 维度（636 号）：双腿各 branch 回首轮提交点 → 驱动第二轮
      // → per-leg 链完整性（branch_moved 恰 1 条 / 第二轮 started.parentSeq ==
      // toSeq / 全事件带 parentSeq）→ 跨腿 derive 深比对。head 是绝对 seq，
      // metadata_updated 计数豁免可致双端 seq 漂移——branch 比对面 head 按
      // volatile 剪除（branchCount/消息面结构等价仍硬比，备案）。
      {
        const branchToCommit = async (engine) => {
          const file = join(engine.root, ".inkos", "sessions", `${chatSession[engine.name]}.jsonl`);
          let commitSeq = null;
          try {
            for (const line of readFileSync(file, "utf-8").split(/\r?\n/)) {
              if (!line.trim()) continue;
              const event = JSON.parse(line);
              if (event.type === "request_committed") {
                commitSeq = event.seq;
                break;
              }
            }
          } catch {
            commitSeq = null;
          }
          if (commitSeq === null) return null;
          const res = await fetchT(`http://127.0.0.1:${engine.port}/api/v1/sessions/${chatSession[engine.name]}/branch`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ toSeq: commitSeq }),
          });
          return res.ok ? res.json().catch(() => null) : null;
        };
        const [nodeBranch, rustBranch] = await Promise.all([branchToCommit(engines[0]), branchToCommit(engines[1])]);
        compared += 1;
        if (!nodeBranch?.ok || !rustBranch?.ok) {
          divergences += 1;
          console.log(`✗ branch 面：双腿 branch 必须成功 node=${JSON.stringify(nodeBranch)} rust=${JSON.stringify(rustBranch)}`);
        } else {
          const drive2 = async (engine) => fetchT(`http://127.0.0.1:${engine.port}/api/v1/agent`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              instruction: "再简短介绍一遍这个项目。",
              sessionId: chatSession[engine.name],
              sessionKind: "chat",
            }),
          });
          const [dNode, dRust] = await Promise.all([drive2(engines[0]), drive2(engines[1])]);
          const chainOk = (engine) => {
            const file = join(engine.root, ".inkos", "sessions", `${chatSession[engine.name]}.jsonl`);
            try {
              const events = readFileSync(file, "utf-8")
                .split(/\r?\n/)
                .filter((line) => line.trim())
                .map((line) => JSON.parse(line));
              const moves = events.filter((event) => event.type === "branch_moved");
              const started2 = events.find(
                (event) => event.type === "request_started" && event.input === "再简短介绍一遍这个项目。",
              );
              return moves.length === 1
                && !!started2
                && started2.parentSeq === moves[0].toSeq
                && events.every((event) => "parentSeq" in event);
            } catch {
              return false;
            }
          };
          compared += 1;
          if (!dNode.ok || !dRust.ok || !chainOk(engines[0]) || !chainOk(engines[1])) {
            divergences += 1;
            console.log(`✗ branch 面：第二轮驱动/链完整性不过 node=${dNode.status}/${chainOk(engines[0])} rust=${dRust.status}/${chainOk(engines[1])}`);
          } else {
            const BRANCH_VOLATILE = new Set([...VOLATILE, "head"]);
            const pruneBranchSession = (node) => {
              if (Array.isArray(node)) return node.map(pruneBranchSession);
              if (node && typeof node === "object") {
                const out = {};
                for (const key of Object.keys(node).sort()) {
                  if (BRANCH_VOLATILE.has(key) || SESSION_IDENTITY.has(key)) continue;
                  out[key] = pruneBranchSession(node[key]);
                }
                return out;
              }
              return node;
            };
            const sessionAfter = async (engine) => {
              const res = await fetchT(`http://127.0.0.1:${engine.port}/api/v1/sessions/${chatSession[engine.name]}`);
              return res.ok ? res.json().catch(() => null) : null;
            };
            const [nodeAfter, rustAfter] = await Promise.all([sessionAfter(engines[0]), sessionAfter(engines[1])]);
            compared += 1;
            const diffPathBranch = nodeAfter && rustAfter
              ? firstDiffPath(pruneBranchSession(nodeAfter), pruneBranchSession(rustAfter))
              : "(双端 GET 必须在场)";
            if (diffPathBranch) {
              divergences += 1;
              console.log(`✗ branch 后 derive 读面分歧 @ ${diffPathBranch}`);
            } else {
              console.log(`✓ branch 面双端一致（branch+第二轮+链完整性+derive 深比对；head 绝对 seq 豁免备案）`);
            }

            // ── 分支点读面（638 号）：双腿 GET /branches 必须在场且等价。
            // 点位 seq/timestamp/head 为绝对序数（同 head 豁免因果链），
            // 结构面（点位数/preview 文本/在链标记/branchCount）硬比。
            const branchesOf = async (engine) => {
              const res = await fetchT(`http://127.0.0.1:${engine.port}/api/v1/sessions/${chatSession[engine.name]}/branches`);
              return res.ok ? res.json().catch(() => null) : null;
            };
            const [nodePoints, rustPoints] = await Promise.all([branchesOf(engines[0]), branchesOf(engines[1])]);
            compared += 1;
            const normalizeBranches = (payload) => payload && typeof payload === "object"
              ? {
                branchCount: payload.branchCount,
                points: (Array.isArray(payload.points) ? payload.points : []).map((point) => ({
                  preview: point.preview,
                  onActiveChain: point.onActiveChain,
                })),
              }
              : null;
            const nodeNorm = normalizeBranches(nodePoints);
            const rustNorm = normalizeBranches(rustPoints);
            const pointsShapeOk = (payload) => payload && Array.isArray(payload.points) && payload.points.length >= 2
              && payload.points.every((point) => typeof point.preview === "string" && typeof point.onActiveChain === "boolean");
            if (!pointsShapeOk(nodePoints) || !pointsShapeOk(rustPoints)) {
              divergences += 1;
              console.log(`✗ 分支点读面：双腿 GET /branches 形态不过 node=${JSON.stringify(nodePoints)?.slice(0, 200)} rust=${JSON.stringify(rustPoints)?.slice(0, 200)}`);
            } else if (JSON.stringify(nodeNorm) !== JSON.stringify(rustNorm)) {
              divergences += 1;
              console.log(`✗ 分支点读面分歧 node=${JSON.stringify(nodeNorm)} rust=${JSON.stringify(rustNorm)}`);
            } else {
              console.log(`✓ 分支点读面双端一致（${nodeNorm.points.length} 点位；seq/timestamp/head 绝对序数豁免备案）`);
            }
          }
        }
      }

      // ── 上下文计量快照面（564 备案清偿，581 号）：会话面驱动的 chat 轮
      // 落幕后双腿 GET /context-meter 必有快照。语义面锁死（键集/来源/
      // 锚点态/数值符号）；tokens 数值不深比——聊天 system prompt 双端未
      // golden 锁定，估算基数可差（启发式同源但输入面不同源，备案）。
      {
        const meterOf = async (engine) => {
          const res = await fetchT(
            `http://127.0.0.1:${engine.port}/api/v1/context-meter?sessionId=${chatSession[engine.name]}`,
          );
          return res.ok ? res.json().catch(() => null) : null;
        };
        const [nodeMeter, rustMeter] = await Promise.all([
          meterOf(engines[0]),
          meterOf(engines[1]),
        ]);
        compared += 1;
        const shapeOk = (m) =>
          m && typeof m === "object"
          && typeof m.heuristicTokens === "number" && m.heuristicTokens > 0
          && m.source === "estimate"
          && m.anchorValid === false
          && typeof m.surfaceNodes === "number" && m.surfaceNodes > 0
          && typeof m.inputWindow === "number" && m.inputWindow > 0;
        if (!shapeOk(nodeMeter) || !shapeOk(rustMeter)) {
          divergences += 1;
          console.log(`✗ 上下文计量快照面：形态不符 node=${JSON.stringify(nodeMeter)} rust=${JSON.stringify(rustMeter)}`);
        } else {
          const keysMatch =
            JSON.stringify(Object.keys(nodeMeter).sort()) === JSON.stringify(Object.keys(rustMeter).sort());
          const windowMatch = nodeMeter.inputWindow === rustMeter.inputWindow;
          if (!keysMatch || !windowMatch) {
            divergences += 1;
            if (!keysMatch) console.log(`✗ 上下文计量快照面键集分歧：node=${Object.keys(nodeMeter).sort()} rust=${Object.keys(rustMeter).sort()}`);
            if (!windowMatch) console.log(`✗ 上下文计量快照面窗口分歧：node=${nodeMeter.inputWindow} rust=${rustMeter.inputWindow}`);
          } else {
            console.log(`✓ 上下文计量快照面双端一致（键集+estimate 来源+无锚点态+窗口相等 inputWindow=${rustMeter.inputWindow}+surfaceNodes 计数非零；tokens 数值备案不深比——聊天 system prompt 双端未 golden 锁定）`);
          }
        }
      }
    }
  }

  // ── stream 偏好面（622 号）：层 1 显式 service+model 双腿各驱动两轮——
  // 非流式轮（custom:MockNF，stream:false）+ 流式轮（custom:Mock，缺省流式）。
  // 每轮独立 chat 会话 + 会话过滤 SSE 收集器，对照有序事件名序列与
  // draft:delta 载荷文本。此前该链仅单端单测（613/621 号），活体双端从未
  // 同场对照——606 备案的 Node 线上恒流式（pi-ai completeSimple 内部恒
  // streamSimple）与 Rust 线上真非流式（streaming_client 108 号整体 JSON）
  // 在 mock 协议保真（622 号按请求 stream 旗标返回形态）后首次活体对齐。
  // session_metadata_updated 沿会话面豁免备案剔除；ping 为协议保活非合同。
  {
    // 622 号：驱动前活体补丁——第二服务 MockNF（stream:false）。在 fixture
    // 跑完之后追加（fixture 会整体重写 inkos.json，preseed 会被覆盖），配置
    // 双端均逐请求加载无启动缓存，热补丁即刻生效；fixture 环境对其他消费者
    // （走查/冒烟）保持单服务不变。追加在 Mock 之后，首个有 key 服务仍是
    // custom:Mock，既有维度的层 2/3 解析行为不变。
    for (const engine of engines) {
      const configPath = join(engine.root, "inkos.json");
      const config = JSON.parse(readFileSync(configPath, "utf-8"));
      // 层 1 显式服务解析只读标准布局 llm.services（593 号教训：顶层 services
      // 为非标布局，层 2/3 靠 INKOS_LLM_BASE_URL 环境兜底才工作）。
      const llm = (config.llm && typeof config.llm === "object" && !Array.isArray(config.llm))
        ? config.llm
        : (config.llm = {});
      llm.services = Array.isArray(llm.services) ? llm.services : [];
      if (!llm.services.some((s) => `${s.service ?? "custom"}:${s.name}` === "custom:MockNF")) {
        llm.services.push({ service: "custom", name: "MockNF", baseUrl: `http://127.0.0.1:${mockPort}/v1`, stream: false });
      }
      writeFileSync(configPath, JSON.stringify(config));
      const secretsPath = join(engine.root, ".inkos", "secrets.json");
      const secrets = JSON.parse(readFileSync(secretsPath, "utf-8"));
      secrets.services = secrets.services ?? {};
      secrets.services["custom:MockNF"] = { apiKey: "sk-mock" };
      writeFileSync(secretsPath, JSON.stringify(secrets));
    }

    const makeSessionCollector = (base) => {
      const events = [];
      const controller = new AbortController();
      const task = (async () => {
        try {
          const res = await fetch(`${base}/api/v1/events`, {
            headers: { Accept: "text/event-stream" },
            signal: controller.signal,
          });
          const reader = res.body.getReader();
          const decoder = new TextDecoder();
          let buf = "";
          let lastEvent = null;
          for (;;) {
            const { done, value } = await reader.read();
            if (done) break;
            buf += decoder.decode(value, { stream: true });
            let idx;
            while ((idx = buf.indexOf("\n")) >= 0) {
              const line = buf.slice(0, idx);
              buf = buf.slice(idx + 1);
              if (line.startsWith("event:")) {
                lastEvent = line.slice(6).trim();
              } else if (line.startsWith("data:")) {
                const name = lastEvent;
                lastEvent = null;
                if (!name || name === "ping") continue;
                let data = null;
                try { data = JSON.parse(line.slice(5).trim()); } catch { /* 忽略非 JSON */ }
                if (data && typeof data.sessionId === "string") events.push({ name, data });
              }
            }
          }
        } catch { /* 流中断 = 收集结束 */ }
      })();
      return { events, stop: async () => { controller.abort(); await task.catch(() => {}); } };
    };

    const drivePreferenceTurn = async (engine, service) => {
      const base = `http://127.0.0.1:${engine.port}`;
      const created = await fetchT(`${base}/api/v1/sessions`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ sessionKind: "chat" }),
      });
      const sessionId = (await created.json().catch(() => null))?.session?.sessionId ?? null;
      if (!created.ok || !sessionId) return { error: `建会话失败 status=${created.status}` };
      const collector = makeSessionCollector(base);
      await new Promise((r) => setTimeout(r, 300));
      try {
        const drive = await fetchT(`${base}/api/v1/agent`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            instruction: "你好，介绍一下这个项目。",
            sessionId,
            sessionKind: "chat",
            service,
            model: "lm-mock-model",
          }),
        });
        if (!drive.ok) {
          const detail = await drive.text().catch(() => "");
          return { error: `驱动失败 status=${drive.status} ${detail.slice(0, 200)}` };
        }
      } catch (error) {
        return { error: `驱动异常：${error?.message ?? error}` };
      }
      await new Promise((r) => setTimeout(r, 800));
      await collector.stop();
      const names = collector.events.map((e) => e.name);
      const deltas = collector.events.filter((e) => e.name === "draft:delta").map((e) => e.data?.text ?? "");
      return { names, deltas };
    };

    const preferenceArms = [
      { key: "nonStream", service: "custom:MockNF" },
      { key: "stream", service: "custom:Mock" },
    ];
    const preferenceResults = {};
    for (const engine of engines) {
      preferenceResults[engine.name] = {};
      for (const arm of preferenceArms) {
        preferenceResults[engine.name][arm.key] = await drivePreferenceTurn(engine, arm.service);
      }
    }
    // 有序事件名序列对照（会话过滤后全序）。session:title 剔除备案：双端
    // 发射点有意不同——Rust 126 号「终态事件后补发」（agent:complete 之后），
    // Node 在跑轮前补发（agent:complete 之前）；标题事件在场性双端一致，
    // 相对终态的时序非合同（前端两类事件独立消费，序无关）。
    const projectPreferenceNames = (turn) => Array.isArray(turn?.names)
      ? turn.names.filter((n) => n !== "session:title" && n !== "session_metadata_updated")
      : null;
    for (const arm of preferenceArms) {
      compared += 1;
      const nodeTurn = preferenceResults.node?.[arm.key];
      const rustTurn = preferenceResults.rust?.[arm.key];
      if (nodeTurn?.error || rustTurn?.error) {
        divergences += 1;
        console.log(`✗ stream 偏好面[${arm.key}]：驱动失败 node=${nodeTurn?.error ?? "-"} rust=${rustTurn?.error ?? "-"}`);
        continue;
      }
      const nodeNames = projectPreferenceNames(nodeTurn);
      const rustNames = projectPreferenceNames(rustTurn);
      if (JSON.stringify(nodeNames) !== JSON.stringify(rustNames)) {
        divergences += 1;
        console.log(`✗ stream 偏好面[${arm.key}]事件序分歧：node=${JSON.stringify(nodeNames)} rust=${JSON.stringify(rustNames)}`);
        continue;
      }
      // draft:delta 形态：恰一次 + 载荷文本双端逐字节一致。不锚 mock CANON
      // 常量——差分器对照双端等价，mock 内容正确性是 mock 自身的事。
      const nodeDeltas = nodeTurn.deltas ?? [];
      const rustDeltas = rustTurn.deltas ?? [];
      const deltaShapeOk = nodeDeltas.length === 1 && rustDeltas.length === 1;
      const deltaTextOk = deltaShapeOk && nodeDeltas[0].length > 0 && nodeDeltas[0] === rustDeltas[0];
      if (!deltaShapeOk || !deltaTextOk) {
        divergences += 1;
        console.log(`✗ stream 偏好面[${arm.key}]draft:delta 形态/文本分歧：node=${JSON.stringify(nodeDeltas.map((t) => t.slice(0, 40)))} rust=${JSON.stringify(rustDeltas.map((t) => t.slice(0, 40)))}`);
      } else {
        console.log(`✓ stream 偏好面[${arm.key}]双端一致（事件序 ${JSON.stringify(rustNames)} + draft:delta 恰一次且载荷文本逐字节相等）`);
      }
    }
  }

  // ── 技能写面周期（576 号）：导入 → 用户面在册 → 删除 → 出册。566 号
  // skills 写端点此前无活体双端对照（574 号仅 Node 单腿真机）；周期同时
  // 驱动双腿 broadcast skills:change（SSE 事件名集合随上文收集器进入比对）。
  {
    const md = [
      "---",
      "name: diff-skill-probe",
      "description: 576 号差分器技能写面探针。",
      "---",
      "探针正文。",
    ].join("\n");
    const dataUrl = `data:text/markdown;base64,${Buffer.from(md, "utf8").toString("base64")}`;
    const results = {};
    for (const engine of engines) {
      const base = `http://127.0.0.1:${engine.port}`;
      const steps = {};
      try {
        const imported = await fetchT(`${base}/api/v1/skills/import`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ files: [{ path: "diff-skill-probe/SKILL.md", dataUrl }] }),
        });
        const importedBody = await imported.json().catch(() => null);
        steps.importStatus = imported.status;
        steps.importId = importedBody?.skill?.id ?? null;
        const listed = await fetchT(`${base}/api/v1/skills`);
        const listedBody = await listed.json().catch(() => null);
        const probe = (listedBody?.skills ?? []).find((s) => s.id === "diff-skill-probe");
        steps.listedAfterImport = Boolean(probe);
        steps.listedSource = probe?.source ?? null;
        const removed = await fetchT(`${base}/api/v1/skills/diff-skill-probe`, { method: "DELETE" });
        steps.deleteStatus = removed.status;
        const relisted = await fetchT(`${base}/api/v1/skills`);
        const relistedBody = await relisted.json().catch(() => null);
        steps.listedAfterDelete = (relistedBody?.skills ?? []).some((s) => s.id === "diff-skill-probe");
      } catch (error) {
        steps.error = error?.message ?? String(error);
      }
      results[engine.name] = steps;
    }
    compared += 1;
    const expected = (steps) =>
      steps.importStatus === 200 && steps.importId === "diff-skill-probe"
      && steps.listedAfterImport === true && steps.listedSource === "project"
      && steps.deleteStatus === 200 && steps.listedAfterDelete === false;
    if (expected(results.node) && expected(results.rust)) {
      console.log(`✓ 技能写面周期双端一致（导入→在册 project→删除→出册；skills:change 事件名面由 STUDIO_SSE_EVENTS golden+双端 broadcast 路由锁定，不在本表——收集器已于会话面比对面停表）`);
    } else {
      divergences += 1;
      console.log(`✗ 技能写面周期分歧：node=${JSON.stringify(results.node)} rust=${JSON.stringify(results.rust)}`);
    }
  }

  console.log(`\n[diff] 对照 ${compared} 个端点，分歧 ${divergences} 个`);
  if (divergences > 0) process.exitCode = 1;
} catch (error) {
  if (error instanceof ProbeEarlyExit) {
    // 探测模式正常收尾（finally 负责杀子进程/清根）。
  } else {
    console.error(`[diff] 异常中断：${error?.message ?? error}`);
    process.exitCode = 1;
  }
} finally {
  for (const child of children) {
    try { try { process.kill(-child.pid, "SIGKILL"); } catch { try { child.kill("SIGKILL"); } catch {} }; } catch { /* exited */ }
  }
  if (!keep) {
    for (const root of Object.values(roots)) rmSync(root, { recursive: true, force: true });
  } else {
    console.log(`[diff] --keep：根保留 ${Object.values(roots).join(" , ")}`);
  }
}
