#!/usr/bin/env node
// 双引擎 GET 契约活体差分（487 号）：
//   node scripts/engine-contract-diff.mjs [--rust-port 8901] [--node-port 8902] [--mock-port 1234]
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

// 515 号：本地引擎挂起时快速失败——统一 20s 超时（SSE 长连接除外）。
const fetchT = (input, init = {}) => fetch(input, { ...init, signal: AbortSignal.timeout(20_000) });


const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const studioDir = join(repoRoot, "packages", "studio");
const rustBinary = join(repoRoot, "engine-rs", "target", "debug", "inkos-engine-server");

const args = process.argv.slice(2);
const argOf = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const rustPort = argOf("--rust-port", "8901");
const nodePort = argOf("--node-port", "8902");
const mockPort = argOf("--mock-port", "1234");
const keep = args.includes("--keep");

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
  return { status: res.status, body };
};

/** 递归剪除易变键并做键排序规范化（保证两侧同构可比）。 */
const VOLATILE = new Set([
  "ts", "createdAt", "updatedAt", "timestamp", "durationMs", "elapsedMs",
  "lastAdvanced", "lastAdvancedChapter", "recordedAt", "registeredAt",
  "lastAppliedChapter", "chapterNumber", "nextChapter", "savedChapters",
  "total", "kept", "tookOverCount", "failureCount",
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

/** 新增端点先经活体探测确认双端 200 再入列（488 号扩至 26 端点）。 */

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
  // 种子内容双端各自撰写（R4/364），canonical 化需产品决策——内容分叉备案
  // 章节审计 issue 明细随双端机检维度差异波动
  [`/api/v1/books/${BOOK}`, { reason: "审计 issue 明细随双端机检维度差异波动", paths: [["chapters", "*", "auditIssues"], ["nextChapter"]] }],
  // 488 号扩展腿发现：
  // skills/genres 内置清单双端不对齐（TS 16 技能/15 题材 vs Rust 1/2）——
  // 内置包镜像缺口 + 既有二进制早于 builtin 合并面，重建需 cargo 解阻后评估

  // 三库种子内容双端各自撰写（364 号）——canonical 化需产品决策
  // doctor 回退端缺 retrieval.chunkCount 键——Rust 侧修复需 cargo
  [`/api/v1/doctor`, { reason: "回退端多 retrieval.chunkCount（Rust 侧补齐需 cargo）", paths: [["retrieval"]] }],
  // lens rank 打分内部实现差异（展示面）。
  [`/api/v1/books/${BOOK}/context-lens/2`, { reason: "resync 管线内部装配差异（483/484 备案）：entry source/rank 随内部实现波动", paths: [["entries"]] }],
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
];

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

  console.log(`\n[diff] 对照 ${compared} 个端点，分歧 ${divergences} 个`);
  if (divergences > 0) process.exitCode = 1;
} catch (error) {
  console.error(`[diff] 异常中断：${error?.message ?? error}`);
  process.exitCode = 1;
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
