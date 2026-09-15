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
import { existsSync, mkdtempSync, rmSync, writeFileSync, mkdirSync, openSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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
  const child = spawn(cmd, cmdArgs, { ...opts, stdio: ["ignore", out, out], detached: false });
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
const apiFor = (base) => async (path) => {
  const res = await fetch(base + path);
  let body = null;
  try { body = await res.json(); } catch { body = null; }
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

/**
 * 已知分歧豁免表（487 号）：路径段数组，"*" 匹配任意单段；命中子树整体剪除。
 * 每条必须注明备案依据——豁免=已知双端行为差异，不是错误默认放行。
 */
const WAIVERS = new Map([
  // resync 双端架构分歧（Rust 直写 vs TS 结构化状态机）——483/484 号备案
  [`/api/v1/books/${BOOK}/promises`, { reason: "resync 架构分歧备案（483/484）", paths: [["currentChapter"]] }],
  [`/api/v1/books/${BOOK}/context-lens`, { reason: "resync 架构分歧备案（483/484）：TS resync 产出 ch1 意图文件", paths: [["chapters"]] }],
  // 审计内部计数：双端机检维度实现差异，非契约面（issueCount 随审计轮次波动）
  [`/api/v1/books/${BOOK}/quality-trend`, { reason: "审计计数波动 + null-vs-缺键序列化（359 号先例）", paths: [["trend"]] }],
  // 种子内容双端各自撰写（R4/364），canonical 化需产品决策——内容分叉备案
  [`/api/v1/asset-library/genre-base`, { reason: "种子内容双端各自撰写（364 号），canonical 化需产品决策", paths: [["assets"]] }],
  // 章节审计 issue 明细随双端机检维度差异波动
  [`/api/v1/books/${BOOK}`, { reason: "审计 issue 明细随双端机检维度差异波动", paths: [["chapters", "*", "auditIssues"], ["nextChapter"]] }],
]);
const ENDPOINTS = [
  "/api/v1/books",
  `/api/v1/books/${BOOK}`,
  `/api/v1/books/${BOOK}/timeline`,
  `/api/v1/books/${BOOK}/promises`,
  `/api/v1/books/${BOOK}/quality-trend`,
  `/api/v1/books/${BOOK}/tension-curve`,
  `/api/v1/books/${BOOK}/context-lens`,
  `/api/v1/books/${BOOK}/director`,
  `/api/v1/books/${BOOK}/codex`,
  `/api/v1/books/${BOOK}/anti-ai-rules`,
  `/api/v1/books/${BOOK}/experience`,
  "/api/v1/asset-library/genre-base",
  "/api/v1/task-routing",
  "/api/v1/project/notify",
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
await waitUntil(async () => (await fetch(`http://127.0.0.1:${mockPort}/v1/models`)).ok, 15_000, "mock 启动");

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
      },
    },
    join(root, "server.log"),
  );
  engines.push({ name: "rust", port: rustPort, root });
}

try {
  for (const engine of engines) {
    const base = `http://127.0.0.1:${engine.port}`;
    await waitUntil(async () => (await fetch(`${base}/api/v1/books`)).ok, 30_000, `${engine.name} 启动`);
    execFileSync("node", [join(scriptDir, "walkthrough-fixture.mjs"), engine.root, engine.port], {
      cwd: repoRoot,
      stdio: ["ignore", "ignore", "inherit"],
    });
  }

  // ── 2. 逐端点归一化深比对 ──
  const [nodeApi, rustApi] = [apiFor(`http://127.0.0.1:${nodePort}`), apiFor(`http://127.0.0.1:${rustPort}`)];
  let divergences = 0;
  let compared = 0;
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
    const waiver = WAIVERS.get(endpoint);
    const aNode = pruneWaivedPaths(prune(nodeRes.body), waiver?.paths ?? []);
    const aRust = pruneWaivedPaths(prune(rustRes.body), waiver?.paths ?? []);
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
  console.log(`\n[diff] 对照 ${compared} 个端点，分歧 ${divergences} 个`);
  if (divergences > 0) process.exitCode = 1;
} catch (error) {
  console.error(`[diff] 异常中断：${error?.message ?? error}`);
  process.exitCode = 1;
} finally {
  for (const child of children) {
    try { child.kill("SIGTERM"); } catch { /* exited */ }
  }
  if (!keep) {
    for (const root of Object.values(roots)) rmSync(root, { recursive: true, force: true });
  } else {
    console.log(`[diff] --keep：根保留 ${Object.values(roots).join(" , ")}`);
  }
}
