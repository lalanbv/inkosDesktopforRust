#!/usr/bin/env node
// Node 回退端一致性冒烟（485 号，483/484 手工冒烟的固化）：
//   node scripts/node-fallback-smoke.mjs [--port 8899] [--mock-port 1234] [--keep]
//
// 编排 walkthrough-mock（LLM 假端点）+ TS 回退端服务器（tsx src/api/index.ts，
// dist 静态面）+ walkthrough-fixture 一致性套件，并追加回退端特有断言：
//   - director PUT {patch} 合并 / GET 回读（478 号契约）
//   - task-routing PUT/GET 往返
//   - SPA 入口与 API 面 Cache-Control: no-store（469/445 号回退端对应面）
//   - run-log 调用计数 > 0（写链遥测在位）
// 任一断言失败退出码非零；--keep 保留服务器与临时根供人工排查。

import { spawn, execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync, openSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const studioDir = join(repoRoot, "packages", "studio");

const args = process.argv.slice(2);
const argOf = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const port = argOf("--port", "8899");
const mockPort = argOf("--mock-port", "1234");
const keep = args.includes("--keep");

const root = mkdtempSync(join(tmpdir(), "inkos-fallback-smoke-"));
const base = `http://127.0.0.1:${port}`;
const children = [];
let failures = 0;

const check = (name, ok, detail = "") => {
  console.log(`${ok ? "✓" : "✗"} ${name}${detail ? `（${detail}）` : ""}`);
  if (!ok) failures += 1;
};

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
    } catch {
      // not ready yet
    }
    await new Promise((resolveSleep) => setTimeout(resolveSleep, 500));
  }
  console.error(`[smoke] 等待超时：${label}`);
  return false;
};

const api = async (path, init) => {
  const res = await fetch(base + path, init);
  let body = null;
  try {
    body = await res.json();
  } catch {
    body = null;
  }
  return { status: res.status, headers: res.headers, body };
};

// ── 0. 环境预置：最小项目配置（与 walkthrough-fixture 同构）──
mkdirSync(join(root, ".inkos"), { recursive: true });
writeFileSync(
  join(root, "inkos.json"),
  JSON.stringify({ name: "inkos-fallback-smoke", version: "0.1.0", services: [{ service: "custom", name: "Mock", baseUrl: `http://127.0.0.1:${mockPort}/v1` }] }),
);
writeFileSync(join(root, ".inkos", "secrets.json"), JSON.stringify({ services: { "custom:Mock": { apiKey: "sk-mock" } } }));

try {
  // ── 1. 启动 mock + TS 服务器 ──
  startChild("node", [join(scriptDir, "walkthrough-mock.mjs"), mockPort], { cwd: repoRoot }, join(root, "mock.log"));
  const mockUp = await waitUntil(
    async () => (await fetch(`http://127.0.0.1:${mockPort}/v1/models`)).ok,
    15_000,
    "walkthrough-mock 启动",
  );
  check("mock LLM 启动", mockUp);
  if (!mockUp) throw new Error("mock failed to start");

  startChild(
    process.execPath,
    // .bin/tsx 是 shell 包装，直接喂 node 会语法崩——用真实 CLI 入口。
    [join(studioDir, "node_modules", "tsx", "dist", "cli.mjs"), join(studioDir, "src", "api", "index.ts"), root],
    { cwd: studioDir, env: { ...process.env, INKOS_STUDIO_PORT: port } },
    join(root, "server.log"),
  );
  const serverUp = await waitUntil(
    async () => (await fetch(`${base}/api/v1/books`)).ok,
    30_000,
    "TS 回退端服务器启动",
  );
  check("TS 服务器启动", serverUp);
  if (!serverUp) throw new Error("server failed to start");

  // ── 2. fixture 一致性套件（resync → write-next → 压缩留痕 → promises）──
  execFileSync("node", [join(scriptDir, "walkthrough-fixture.mjs"), root, port], {
    cwd: repoRoot,
    stdio: "inherit",
  });
  check("fixture 一致性套件（resync/write-next/promises/run-log）", true);

  // ── 3. director PUT{patch} 合并 / GET 回读（478 号契约）──
  const putDirector = await api(`/api/v1/books/${encodeURIComponent("镜花水月")}/director`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      patch: { runMode: "range", stage: "writing", inspiration: { premise: "镜中世界反向修行", keywords: ["悬疑"] } },
    }),
  });
  const getDirector = await api(`/api/v1/books/${encodeURIComponent("镜花水月")}/director`);
  check(
    "director PUT{patch} 合并并回读",
    putDirector.status === 200
      && getDirector.body?.session?.runMode === "range"
      && getDirector.body?.session?.inspiration?.keywords?.[0] === "悬疑"
      && typeof getDirector.body?.savedChapters === "number"
      && typeof getDirector.body?.resumeAdvice === "string",
  );

  // ── 4. task-routing PUT/GET 往返 ──
  await api("/api/v1/task-routing", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ routing: { defaults: { model: "lm-mock-model" }, tasks: { writing: { model: "m-w" } } } }),
  });
  const routing = await api("/api/v1/task-routing");
  check(
    "task-routing 往返",
    routing.body?.routing?.defaults?.model === "lm-mock-model"
      && routing.body?.routing?.tasks?.writing?.model === "m-w",
  );

  // ── 5. 缓存头（469/445 号回退端对应面）──
  const spaRes = await fetch(`${base}/`);
  check("SPA 入口 no-store", spaRes.headers.get("cache-control")?.includes("no-store") === true);
  const apiRes = await fetch(`${base}/api/v1/books`);
  check("API 面 no-store", apiRes.headers.get("cache-control")?.includes("no-store") === true);

  // ── 6. run-log 调用计数（写链遥测在位）──
  const runLog = await api("/api/v1/run-log?limit=50");
  check("run-log 调用计数 > 0", (runLog.body?.total ?? 0) > 0);
} catch (error) {
  failures += 1;
  console.error(`[smoke] 异常中断：${error?.message ?? error}`);
} finally {
  for (const child of children) {
    try {
      child.kill("SIGTERM");
    } catch {
      // already exited
    }
  }
  if (keep) {
    console.log(`[smoke] --keep：服务器已停止，临时根保留在 ${root}`);
  } else {
    rmSync(root, { recursive: true, force: true });
  }
}

if (failures > 0) {
  console.error(`[smoke] ✗ ${failures} 项断言失败`);
  process.exit(1);
}
console.log("[smoke] ✓ 回退端一致性冒烟全部通过");
