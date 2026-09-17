#!/usr/bin/env node
// 双引擎一致性冒烟（485 号固化 + 486 号扩展为 Rust/Node 对照双跑）：
//   node scripts/node-fallback-smoke.mjs [--engine node|rust|both] [--port 8899] [--mock-port 1234] [--keep]
//
// 每个引擎腿：临时项目根 + walkthrough-mock（LLM 假端点）+ 被测引擎
// （node=tsx src/api/index.ts；rust=engine-rs/target/debug/inkos-engine-server）
// + walkthrough-fixture 一致性套件 + 共通断言：
//   - director PUT {patch} 合并 / GET 回读（478 号契约）
//   - task-routing PUT/GET 往返
//   - SPA 入口与 API 面 Cache-Control: no-store（469/445 号双端面）
//   - run-log 调用计数 > 0（写链遥测在位）
// 任一断言失败退出码非零；--keep 保留服务器与临时根供人工排查。
// Rust 腿需要 target/debug 二进制；cargo 链接受阻（如 Xcode 许可）时自动
// 降级为跳过并告警，不算失败。

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync, mkdirSync, openSync } from "node:fs";
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
const has = (name) => args.includes(name);
const engineMode = argOf("--engine", "both");
const port = argOf("--port", "8899");
const mockPort = argOf("--mock-port", "1234");
const keep = has("--keep");

const children = [];
const sharedChildren = [];
let failures = 0;

const check = (name, ok, detail = "") => {
  console.log(`${ok ? "✓" : "✗"} ${name}${detail ? `（${detail}）` : ""}`);
  if (!ok) failures += 1;
};

const startChild = (cmd, cmdArgs, opts, logPath, shared = false) => {
  const out = openSync(logPath, "a");
  const child = spawn(cmd, cmdArgs, { ...opts, stdio: ["ignore", out, out], detached: true });
  (shared ? sharedChildren : children).push(child);
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

const apiFor = (base) => async (path, init) => {
  const res = await fetchT(base + path, init);
  let body = null;
  try {
    body = await res.json();
  } catch {
    body = null;
  }
  return { status: res.status, headers: res.headers, body };
};

/** 单引擎腿：独立临时根 + 独立被测服务器 + 全套断言。 */
async function runEngineLeg(engine) {
  console.log(`\n[smoke] ── 引擎腿：${engine} ──`);
  const legPort = port;
  const base = `http://127.0.0.1:${legPort}`;
  const api = apiFor(base);
  const root = mkdtempSync(join(tmpdir(), `inkos-smoke-${engine}-`));

  // 环境预置：最小项目配置（与 walkthrough-fixture 同构）。
  mkdirSync(join(root, ".inkos"), { recursive: true });
  writeFileSync(
    join(root, "inkos.json"),
    JSON.stringify({ name: `inkos-smoke-${engine}`, version: "0.1.0", services: [{ service: "custom", name: "Mock", baseUrl: `http://127.0.0.1:${mockPort}/v1` }] }),
  );
  writeFileSync(join(root, ".inkos", "secrets.json"), JSON.stringify({ services: { "custom:Mock": { apiKey: "sk-mock" } } }));

  // 启动被测引擎。
  if (engine === "node") {
    startChild(
      process.execPath,
      // .bin/tsx 是 shell 包装，直接喂 node 会语法崩——用真实 CLI 入口。
      [join(studioDir, "node_modules", "tsx", "dist", "cli.mjs"), join(studioDir, "src", "api", "index.ts"), root],
      { cwd: studioDir, env: { ...process.env, INKOS_STUDIO_PORT: legPort } },
      join(root, "server.log"),
    );
  } else {
    startChild(
      rustBinary,
      [],
      {
        cwd: repoRoot,
        env: {
          ...process.env,
          INKOS_PORT: legPort,
          INKOS_PROJECT_ROOT: root,
          INKOS_STATIC_DIR: join(studioDir, "dist"),
          INKOS_LLM_BASE_URL: `http://127.0.0.1:${mockPort}/v1`,
        },
      },
      join(root, "server.log"),
    );
  }

  const serverUp = await waitUntil(
    async () => (await fetchT(`${base}/api/v1/books`)).ok,
    engine === "node" ? 90_000 : 15_000,
    `${engine} 引擎启动`,
  );
  check(`${engine} 引擎启动`, serverUp);
  if (!serverUp) throw new Error(`${engine} server failed to start`);

  // fixture 一致性套件（resync → write-next → 压缩留痕 → promises）。
  execFileSync("node", [join(scriptDir, "walkthrough-fixture.mjs"), root, legPort], {
    cwd: repoRoot,
    stdio: "inherit",
  });
  check(`[${engine}] fixture 一致性套件（resync/write-next/promises/run-log）`, true);

  // director PUT{patch} 合并 / GET 回读（478 号契约）。
  const book = encodeURIComponent("镜花水月");
  const putDirector = await api(`/api/v1/books/${book}/director`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      patch: { runMode: "range", stage: "writing", inspiration: { premise: "镜中世界反向修行", keywords: ["悬疑"] } },
    }),
  });
  const getDirector = await api(`/api/v1/books/${book}/director`);
  check(
    `[${engine}] director PUT{patch} 合并并回读`,
    putDirector.status === 200
      && getDirector.body?.session?.runMode === "range"
      && getDirector.body?.session?.inspiration?.keywords?.[0] === "悬疑"
      && typeof getDirector.body?.savedChapters === "number"
      && typeof getDirector.body?.resumeAdvice === "string",
  );

  // task-routing PUT/GET 往返。
  await api("/api/v1/task-routing", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ routing: { defaults: { model: "lm-mock-model" }, tasks: { writing: { model: "m-w" } } } }),
  });
  const routing = await api("/api/v1/task-routing");
  check(
    `[${engine}] task-routing 往返`,
    routing.body?.routing?.defaults?.model === "lm-mock-model"
      && routing.body?.routing?.tasks?.writing?.model === "m-w",
  );

  // 缓存头（469/445 号双端面）。
  const spaRes = await fetchT(`${base}/`);
  check(`[${engine}] SPA 入口 no-store`, spaRes.headers.get("cache-control")?.includes("no-store") === true);
  const apiRes = await fetchT(`${base}/api/v1/books`);
  check(`[${engine}] API 面 no-store`, apiRes.headers.get("cache-control")?.includes("no-store") === true);

  // run-log 调用计数（写链遥测在位）。
  const runLog = await api("/api/v1/run-log?limit=50");
  check(`[${engine}] run-log 调用计数 > 0`, (runLog.body?.total ?? 0) > 0);

  // 健康探针（493 号：node 腿补齐后双端统一探 this 面）。
  if (engine === "node") {
    const health = await api("/api/v1/health");
    check(
      `[${engine}] 健康探针 ok + backend 标识`,
      health.status === 200
        && health.body?.ok === true
        && health.body?.backend === "node-fallback"
        && typeof health.body?.version === "string",
    );
  }
}

// ── 共享 mock（所有引擎腿共用一个 LLM 假端点）──
startChild("node", [join(scriptDir, "walkthrough-mock.mjs"), mockPort], { cwd: repoRoot }, join(tmpdir(), "inkos-smoke-mock.log"), true);
const mockUp = await waitUntil(
  async () => (await fetchT(`http://127.0.0.1:${mockPort}/v1/models`)).ok,
  15_000,
  "walkthrough-mock 启动",
);
check("mock LLM 启动", mockUp);

const legs =
  engineMode === "both"
    ? ["node", ...(existsSync(rustBinary) ? ["rust"] : [])]
    : [engineMode];
if (engineMode !== "node" && !legs.includes("rust")) {
  console.warn(`[smoke] ⚠ Rust 二进制缺失（${rustBinary}）——rust 腿跳过（cargo 链接受阻时属预期，不算失败）`);
}

for (const leg of legs) {
  try {
    await runEngineLeg(leg);
  } catch (error) {
    failures += 1;
    console.error(`[smoke] [${leg}] 异常中断：${error?.message ?? error}`);
  }
  // 腿间清理：杀掉本腿服务器，释放端口供下一腿复用。
  for (const child of children.splice(0)) {
    try {
      try { process.kill(-child.pid, "SIGKILL"); } catch { try { child.kill("SIGKILL"); } catch {} };
    } catch {
      // already exited
    }
  }
  await new Promise((resolveSleep) => setTimeout(resolveSleep, 1_000));
}

for (const child of children.splice(0)) {
  try {
    try { process.kill(-child.pid, "SIGKILL"); } catch { try { child.kill("SIGKILL"); } catch {} };
  } catch {
    // already exited
  }
}
// 共享 mock 收尾同样要杀——否则进程悬挂持有管道（487 号后台卡死根因）。
for (const child of sharedChildren.splice(0)) {
  try {
    try { process.kill(-child.pid, "SIGKILL"); } catch { try { child.kill("SIGKILL"); } catch {} };
  } catch {
    // already exited
  }
}

if (failures > 0) {
  console.error(`[smoke] ✗ ${failures} 项断言失败`);
  process.exit(1);
}
console.log("[smoke] ✓ 双引擎一致性冒烟全部通过");
