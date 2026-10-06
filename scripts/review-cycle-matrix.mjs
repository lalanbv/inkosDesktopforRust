#!/usr/bin/env node
// 审改循环六分支矩阵集成套件（659 号）：chapter-review-cycle 的继续/退出/回退
// 分支回归护栏——650–658 号手工 env 组合验证的自动化固化。
//
//   node scripts/review-cycle-matrix.mjs [--engine rust|node] [--base-port 8797]
//
// --engine node（默认）= TS 管线（studio 回退端）；--engine rust = Rust 引擎
// （680 号：673 备案「Rust 腿六分支未逐一注入」的套件化清偿——审改循环
// 日志经 on_log 管道落 inkos.log（674b），rust 腿断言面=inkos.log 而非
// stdout env.log；rust 腿前置 ensure-rust-bin 新鲜度闸（641 教训：先重建
// 再判读；682 号收紧：uncertified/skipped 亦拒绝出具活体证据））。
//
// 七场景（各起独立 walkthrough-env 实例，串行；双腿同一场景表）：
//   B 达标退出    local "45,91"   retries=1  → 修复轮次 1/1→复审 91 达标退出，
//                                               落盘 PATCH 版（含替换句）
//   C 未净提升    local "45,47,91" retries=2 → 修复轮次 1 未净提升（45→47）退出，
//                                               落盘原稿（修订被丢弃）
//   D 净提升+多轮 local "45,55,91" retries=2  → 轮 1→轮 2（净提升继续）→复审 91
//                                               达标退出，落盘双 PATCH 痕迹
//   F 未产出      local "45,91"+PASSTHROUGH retries=1 → 直通回传原文→未产出退出，
//                                               落盘原稿（669 前以手工实录为证）
//   G 三轮净提升  local "45,55,70,91" retries=3 → 三轮全续→91 达标退出（667 号）
//   G2 多轮未产出 local "45,55,55,91"+PASSTHROUGH_FROM=2 retries=2 → 轮 1 PATCH
//                                               落盘、轮 2 直通未产出退出（669 号）
//   E restore     local "45,91" + REVISE_BLOAT→ 回退到最高分版本（45 vs 91），
//                                               落盘初稿（超长修订被回退）
// 基线直通形态（structural 无注入）由 gate:ts 的 node-fallback-smoke 常态覆盖；
// 任一场景断言失败退出码非零。

import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { existsSync, readdirSync, readFileSync, appendFileSync, mkdtempSync } from "node:fs";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");

const args = process.argv.slice(2);
const basePortIdx = args.indexOf("--base-port");
const basePort = basePortIdx >= 0 ? Number(args[basePortIdx + 1]) : 8797;
const engineIdx = args.indexOf("--engine");
const engine = engineIdx >= 0 ? args[engineIdx + 1] : "node";
if (engine !== "rust" && engine !== "node") {
  console.error(`[rcm] ✗ --engine 仅支持 rust|node（收到 ${engine}）`);
  process.exit(2);
}

// rust 腿前置新鲜度闸（641 教训：testgate 的 cargo test 不编译 bin 目标，
// 活体腿消费陈旧二进制=测旧代码零信号）。682 号收紧（681 P3 备案）：rust 腿
// 的产品是「二进制与源码同源可证」的活体证据——build-failed/missing 既有拒绝
// 外，uncertified（cargo 不可用，新鲜度无法自证）与 skipped
// （GATE_TS_SKIP_RUST_BIN=1 显式豁免）同样拒绝出具；override
// （INKOS_SMOKE_RUST_BIN，517 形态外部产物）=显式声明新鲜度调用方自管，放行。
if (engine === "rust") {
  const { ensureRustBinFresh } = await import(pathToFileURL(join(scriptDir, "ensure-rust-bin.mjs")).href);
  const verdict = ensureRustBinFresh(repoRoot);
  console.log(`[rcm] rust-bin 新鲜度闸：${verdict.status}${verdict.detail ? `（${verdict.detail}）` : ""}`);
  const refusedDetail = verdict.status === "skipped"
    ? "（GATE_TS_SKIP_RUST_BIN=1——rust 证据需 cargo 指纹同源，请清除该 env 重跑）"
    : verdict.status === "uncertified"
      ? "（cargo 不可用，二进制新鲜度无法自证）"
      : "";
  if (["build-failed", "missing", "uncertified", "skipped"].includes(verdict.status)) {
    console.error(`[rcm] ✗ rust 腿拒绝在 ${verdict.status} 状态下出具活体证据${refusedDetail}`);
    process.exit(1);
  }
}

const SCENARIOS = [
  {
    id: "B-达标退出",
    env: { WALKTHROUGH_MOCK_STREAM_DELAY_MS: "20", WALKTHROUGH_MOCK_AUDIT_SCORES: "45,91", WALKTHROUGH_MOCK_AUDIT_SCOPE: "local", WALKTHROUGH_REVIEW_RETRIES: "1" },
    expect: {
      log: ["修复轮次 1/1（当前 45 分）", "修复后达到通过线（91 分）"],
      absentLog: ["未净提升", "回退到最高分版本"],
      chapterFileContains: "缓缓收紧五指",
      promisesTimeline: 4,
    },
  },
  {
    id: "C-未净提升退出",
    env: { WALKTHROUGH_MOCK_STREAM_DELAY_MS: "20", WALKTHROUGH_MOCK_AUDIT_SCORES: "45,47,91", WALKTHROUGH_MOCK_AUDIT_SCOPE: "local", WALKTHROUGH_REVIEW_RETRIES: "2" },
    expect: {
      log: ["修复轮次 1/2（当前 45 分）", "修复轮次 1 未净提升（45 → 47），退出循环"],
      absentLog: ["达到通过线", "回退到最高分版本"],
      chapterFileContains: null, // 修订被丢弃：落原稿
      chapterFileNotContains: "缓缓收紧五指",
      promisesTimeline: 4,
    },
  },
  {
    id: "D-净提升与多轮上限",
    env: { WALKTHROUGH_MOCK_STREAM_DELAY_MS: "20", WALKTHROUGH_MOCK_AUDIT_SCORES: "45,55,91", WALKTHROUGH_MOCK_AUDIT_SCOPE: "local", WALKTHROUGH_REVIEW_RETRIES: "2" },
    expect: {
      log: ["修复轮次 1/2（当前 45 分）", "修复轮次 2/2（当前 55 分）", "修复后达到通过线（91 分）"],
      absentLog: ["未净提升", "未产出新内容"],
      chapterFileContains: "缓缓收紧五指",
      chapterFileAlsoContains: "纵马溅了他满身泥水",
      promisesTimeline: 4,
    },
  },
  {
    id: "F-未产出新内容",
    env: { WALKTHROUGH_MOCK_STREAM_DELAY_MS: "20", WALKTHROUGH_MOCK_AUDIT_SCORES: "45,91", WALKTHROUGH_MOCK_AUDIT_SCOPE: "local", WALKTHROUGH_MOCK_REVISE_PASSTHROUGH: "1", WALKTHROUGH_REVIEW_RETRIES: "1" },
    expect: {
      log: ["修复轮次 1/1（当前 45 分）", "修复轮次 1 未产出新内容，退出循环"],
      absentLog: ["达到通过线", "回退到最高分版本"],
      chapterFileNotContains: "缓缓收紧五指", // 直通回传原文：落盘保持原稿
      promisesTimeline: 4,
    },
  },
  {
    id: "G-三轮净提升链",
    env: { WALKTHROUGH_MOCK_STREAM_DELAY_MS: "20", WALKTHROUGH_MOCK_AUDIT_SCORES: "45,55,70,91", WALKTHROUGH_MOCK_AUDIT_SCOPE: "local", WALKTHROUGH_REVIEW_RETRIES: "3" },
    expect: {
      log: ["修复轮次 1/3（当前 45 分）", "修复轮次 2/3（当前 55 分）", "修复轮次 3/3（当前 70 分）", "修复后达到通过线（91 分）"],
      absentLog: ["未净提升", "未产出新内容", "回退到最高分版本"],
      chapterFileContains: "缓缓收紧五指",
      chapterFileAlsoContains: "掌纹里渗出细密的汗意",
      promisesTimeline: 4,
    },
  },
  {
    id: "G2-多轮未产出",
    env: { WALKTHROUGH_MOCK_STREAM_DELAY_MS: "20", WALKTHROUGH_MOCK_AUDIT_SCORES: "45,55,55,91", WALKTHROUGH_MOCK_AUDIT_SCOPE: "local", WALKTHROUGH_MOCK_REVISE_PASSTHROUGH: "1", WALKTHROUGH_MOCK_REVISE_PASSTHROUGH_FROM: "2", WALKTHROUGH_REVIEW_RETRIES: "2" },
    expect: {
      log: ["修复轮次 1/2（当前 45 分）", "修复轮次 2/2（当前 55 分）", "修复轮次 2 未产出新内容，退出循环"],
      absentLog: ["达到通过线", "回退到最高分版本"],
      chapterFileContains: "缓缓收紧五指", // 轮 1 PATCH 版落盘（净提升继续）
      chapterFileNotContains: "纵马溅了他满身泥水", // 轮 2 直通未产出：PATCH2 不落盘
      promisesTimeline: 4,
    },
  },
  {
    id: "E-restore回退",
    env: { WALKTHROUGH_MOCK_STREAM_DELAY_MS: "20", WALKTHROUGH_MOCK_AUDIT_SCORES: "45,91", WALKTHROUGH_MOCK_AUDIT_SCOPE: "local", WALKTHROUGH_MOCK_REVISE_BLOAT: "1", WALKTHROUGH_REVIEW_RETRIES: "1" },
    expect: {
      log: ["修复轮次 1/1（当前 45 分）", "回退到最高分版本（45 分 vs 当前 91 分）"],
      absentLog: ["达到通过线", "未净提升"],
      chapterFileContains: null, // 超长修订被回退：落初稿
      chapterFileNotContains: "缓缓收紧五指",
      promisesTimeline: 4,
    },
  },
];

console.log(`[rcm] 审改循环矩阵·${engine} 腿（${SCENARIOS.length} 场景，串行）`);

const results = [];
for (let i = 0; i < SCENARIOS.length; i++) {
  const sc = SCENARIOS[i];
  const port = basePort + i * 2;
  const mockPort = port + 1;
  const root = mkdtempSync(join("/tmp", `rcm-${i}-`));
  const logPath = join(root, "env.log");
  const startedAt = Date.now();
  process.stdout.write(`▶ [${sc.id}] port=${port} … `);

  const child = spawn(process.execPath, [join(scriptDir, "walkthrough-env.mjs"), "--engine", engine, "--port", String(port), "--mock-port", String(mockPort), "--root", root], {
    cwd: repoRoot,
    env: { ...process.env, ...sc.env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.on("data", (chunk) => {
    try { appendFileSync(logPath, chunk); } catch {}
  });
  child.stderr.on("data", (chunk) => {
    try { appendFileSync(logPath, chunk); } catch {}
  });
  // 等 fixture 完成（数据就绪横幅）；walkthrough-env 硬失败（二进制缺失/
  // mock 未就绪等 "[env] ✗" 前缀行）即早退，不等满 300s。
  let ready = false;
  let envFailed = false;
  const deadline = Date.now() + 300_000;
  while (Date.now() < deadline) {
    try {
      if (existsSync(logPath)) {
        const log = readFileSync(logPath, "utf-8");
        if (log.includes("走查环境就绪")) { ready = true; break; }
        if (log.includes("[env] ✗")) { envFailed = true; break; }
      }
    } catch { /* log not yet */ }
    await new Promise((r) => setTimeout(r, 5000));
  }

  const checks = [];
  if (ready) {
    // 断言审改循环日志关键词：node 腿走 stdout（env.log）；rust 腿走
    // on_log→inkos.log（674b：Rust 侧审改循环日志不经 stdout，阶段行落
    // projectRoot/inkos.log JSON 行，中文原文不转义=关键词逐字可匹配）。
    const keywordLogPath = engine === "rust" ? join(root, "inkos.log") : logPath;
    if (existsSync(keywordLogPath)) {
      const kwLog = readFileSync(keywordLogPath, "utf-8");
      for (const kw of sc.expect.log) checks.push([`日志含「${kw.slice(0, 18)}…」`, kwLog.includes(kw)]);
      for (const kw of sc.expect.absentLog ?? []) checks.push([`日志不含「${kw.slice(0, 14)}…」`, !kwLog.includes(kw)]);
    } else {
      checks.push([`${engine} 腿日志面（${engine === "rust" ? "inkos.log" : "env.log"}）存在`, false]);
    }
    // promises 断言（fixture 已断言，这里复核日志行；fixture stdout 恒在 env.log）
    const log = readFileSync(logPath, "utf-8");
    const pm = log.match(/promises timeline = (\d+) 条/);
    checks.push([`promises timeline ≥ ${sc.expect.promisesTimeline}`, pm ? Number(pm[1]) >= sc.expect.promisesTimeline : false]);
    // 落盘痕迹
    const chaptersDir = join(root, "books", "镜花水月", "chapters");
    if (existsSync(chaptersDir)) {
      const files = readdirSync(chaptersDir).filter((f) => f.endsWith(".md") && /^000[23]/.test(f));
      const content = files.map((f) => readFileSync(join(chaptersDir, f), "utf-8")).join("\n");
      if (sc.expect.chapterFileContains) checks.push([`落盘含「${sc.expect.chapterFileContains.slice(0, 8)}…」`, content.includes(sc.expect.chapterFileContains)]);
      if (sc.expect.chapterFileNotContains) checks.push([`落盘不含「${sc.expect.chapterFileNotContains.slice(0, 8)}…」（修订被丢弃）`, !content.includes(sc.expect.chapterFileNotContains)]);
      if (sc.expect.chapterFileAlsoContains) checks.push([`落盘含「${sc.expect.chapterFileAlsoContains.slice(0, 8)}…」（双 PATCH）`, content.includes(sc.expect.chapterFileAlsoContains)]);
    } else {
      checks.push(["chapters 目录存在", false]);
    }
  } else {
    checks.push([envFailed ? "env 启动失败（见 env.log）" : "env 就绪（300s 内）", false]);
  }

  try { child.kill("SIGTERM"); } catch {}
  await new Promise((r) => setTimeout(r, 1500));
  try {
    const { execFileSync } = await import("node:child_process");
    execFileSync("lsof", ["-ti", ":" + port, "-ti", ":" + mockPort], { timeout: 5000, encoding: "utf-8" }).split("\n").filter(Boolean).forEach((pid) => {
      try { process.kill(Number(pid.trim()), "SIGKILL"); } catch {}
    });
  } catch { /* ports free */ }

  const seconds = ((Date.now() - startedAt) / 1000).toFixed(0);
  const ok = checks.every(([, pass]) => pass);
  results.push([sc.id, ok, seconds, checks]);
  process.stdout.write(`${ok ? "✓" : "✗"} ${seconds}s\n`);
  for (const [name, pass] of checks) {
    if (!pass) console.error(`    ✗ ${name}`);
  }
}

console.log(`\n──────── 审改循环矩阵汇总（${engine} 腿） ────────`);
let failed = 0;
for (const [id, ok, seconds, checks] of results) {
  console.log(`${ok ? "✓" : "✗"} ${id}（${seconds}s，${checks.length} 断言）`);
  if (!ok) failed += 1;
}
if (failed > 0) {
  console.error(`\n[rcm] ✗ ${failed} 场景失败`);
  process.exit(1);
}
console.log(`\n[rcm] ✓ 审改循环矩阵全部通过（${engine} 腿）`);
