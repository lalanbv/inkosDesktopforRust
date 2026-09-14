#!/usr/bin/env node
// 走查环境一键编排（437 号，405 号遗留第 2 项收口）：mock + 引擎 + fixture 数据
// 三件套一条命令拉起，浏览器直接开走。
//
//   node scripts/walkthrough-env.mjs [--port 8787] [--mock-port 1234] [--root <dir>] [--build]
//
// 默认 root=/tmp/inkos-walk-env（已存在则复用，fixture 脚本幂等）。
// --build：引擎二进制或 studio dist 缺失时自动构建（默认只检查并给出命令）。
//
// 编排内容：
//   1. 启动 walkthrough-mock（LLM 假端点）；
//   2. 启动 inkos-engine-server（静态面 + INKOS_LLM_BASE_URL 须带 /v1——
//      436 号实测：模型列表走 mock 的 /v1/models，缺 /v1 则选择器空转）；
//   3. 双端就绪探测后调 walkthrough-fixture.mjs 写 fixture 书 + resync +
//      write-next + 断言（详见该脚本）；
//   4. 打印走查地址；Ctrl+C 一并停掉两进程（数据留在 root 供复跑）。
//
// 本脚本只编排不造数据——数据面单点在 walkthrough-fixture.mjs（405/406 号）。

import { spawn, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");

function parseArgs(argv) {
  const out = { port: "8787", mockPort: "1234", root: "/tmp/inkos-walk-env", build: false };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === "--port") out.port = argv[++i];
    else if (a === "--mock-port") out.mockPort = argv[++i];
    else if (a === "--root") out.root = argv[++i];
    else if (a === "--build") out.build = true;
    else if (a === "--help" || a === "-h") out.help = true;
    else {
      console.error(`未知参数：${a}`);
      process.exit(2);
    }
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
if (args.help) {
  console.log("用法：node scripts/walkthrough-env.mjs [--port 8787] [--mock-port 1234] [--root <dir>] [--build]");
  process.exit(0);
}

const engineBin = join(repoRoot, "engine-rs", "target", "debug", "inkos-engine-server");
const studioDist = join(repoRoot, "packages", "studio", "dist");
const mockScript = join(scriptDir, "walkthrough-mock.mjs");
const fixtureScript = join(scriptDir, "walkthrough-fixture.mjs");

function run(cmd, cmdArgs, cwd, label) {
  console.log(`[env] ${label}: ${cmd} ${cmdArgs.join(" ")}`);
  const r = spawnSync(cmd, cmdArgs, { cwd, stdio: "inherit" });
  if (r.status !== 0) {
    console.error(`[env] ✗ ${label} 失败`);
    process.exit(1);
  }
}

// ── 前置检查（--build 时自动补齐）──
if (!existsSync(engineBin)) {
  if (args.build) run("cargo", ["build"], join(repoRoot, "engine-rs"), "构建引擎");
  else {
    console.error(`[env] ✗ 引擎二进制缺失：${engineBin}`);
    console.error("    先构建：cd engine-rs && cargo build（或本脚本加 --build）");
    process.exit(1);
  }
}
if (!existsSync(join(studioDist, "index.html"))) {
  if (args.build) run("pnpm", ["--filter", "./packages/studio", "build"], repoRoot, "构建 studio");
  else {
    console.error(`[env] ✗ studio dist 缺失：${studioDist}`);
    console.error("    先构建：pnpm --filter ./packages/studio build（或本脚本加 --build）");
    process.exit(1);
  }
}
for (const f of [mockScript, fixtureScript]) {
  if (!existsSync(f)) {
    console.error(`[env] ✗ 配套脚本缺失：${f}`);
    process.exit(1);
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitReady(name, url, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const ok = await fetch(url).then((r) => r.ok).catch(() => false);
    if (ok) {
      console.log(`[env] ✓ ${name} 就绪（${url}）`);
      return true;
    }
    await sleep(400);
  }
  return false;
}

const children = [];
let shuttingDown = false;
function shutdown(signal) {
  if (shuttingDown) return;
  shuttingDown = true;
  console.log(`\n[env] 收到 ${signal}，停止子进程…`);
  for (const child of children) {
    try { child.kill("SIGTERM"); } catch {}
  }
  process.exit(0);
}
process.on("SIGINT", () => shutdown("SIGINT"));
process.on("SIGTERM", () => shutdown("SIGTERM"));

// ── 1. mock ──
const mock = spawn(process.execPath, [mockScript, args.mockPort], { stdio: "inherit" });
children.push(mock);
mock.on("exit", (code) => {
  if (!shuttingDown) console.error(`[env] ✗ mock 意外退出（${code}）`);
});

if (!(await waitReady("mock", `http://127.0.0.1:${args.mockPort}/v1/models`, 15000))) {
  console.error("[env] ✗ mock 未就绪");
  shutdown("timeout");
}

// ── 2. 引擎 ──
const engine = spawn(engineBin, [], {
  stdio: "inherit",
  cwd: repoRoot,
  env: {
    ...process.env,
    INKOS_PROJECT_ROOT: args.root,
    // /v1 必须带：模型列表与 chat/completions 都走 mock 的 /v1 面（436 号实测）。
    INKOS_LLM_BASE_URL: `http://127.0.0.1:${args.mockPort}/v1`,
    INKOS_LLM_API_KEY: "sk-mock",
    INKOS_LLM_MODEL: "lm-mock-model",
    INKOS_STATIC_DIR: studioDist,
    INKOS_PORT: args.port,
    INKOS_BUILTIN_GENRES_DIR: join(args.root, "assets", "genres"),
  },
});
children.push(engine);
engine.on("exit", (code) => {
  if (!shuttingDown) console.error(`[env] ✗ 引擎意外退出（${code}）`);
});

const base = `http://127.0.0.1:${args.port}`;
if (!(await waitReady("引擎", `${base}/api/v1/run-log`, 30000))) {
  console.error("[env] ✗ 引擎未就绪");
  shutdown("timeout");
}

// ── 3. fixture 数据（幂等，可重复跑）──
console.log("[env] 写入 fixture 数据 + resync + write-next …");
const fixtureExit = await new Promise((resolve) => {
  const fixture = spawn(process.execPath, [fixtureScript, args.root, args.port, args.mockPort], {
    stdio: "inherit",
  });
  fixture.on("exit", resolve);
});
if (fixtureExit !== 0) {
  console.error(`[env] ✗ fixture 断言失败（exit ${fixtureExit}），环境保持运行供排查`);
  shutdown("fixture-failed");
}

// ── 4. 就绪提示，常驻直到 Ctrl+C ──
console.log("");
console.log("════════════════════════════════════════════════");
console.log(`[env] ✓ 走查环境就绪：${base}（项目根 ${args.root}）`);
console.log("[env]   四 UI 面点验要点见上方 fixture 输出；Ctrl+C 停止。");
console.log("════════════════════════════════════════════════");
await new Promise(() => {});
