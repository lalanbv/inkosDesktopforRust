#!/usr/bin/env node
// 审改循环六分支矩阵集成套件（659 号）：chapter-review-cycle 的继续/退出/回退
// 分支回归护栏——650–658 号手工 env 组合验证的自动化固化。
//
//   node scripts/review-cycle-matrix.mjs [--base-port 8797]
//
// 四场景（各起独立 walkthrough-env 实例，串行）：
//   B 达标退出    local "45,91"   retries=1  → 修复轮次 1/1→复审 91 达标退出，
//                                               落盘 PATCH 版（含替换句）
//   C 未净提升    local "45,47,91" retries=2 → 修复轮次 1 未净提升（45→47）退出，
//                                               落盘原稿（修订被丢弃）
//   D 净提升+多轮 local "45,55,91" retries=2  → 轮 1→轮 2（净提升继续）→复审 91
//                                               达标退出，落盘双 PATCH 痕迹
//   E restore     local "45,91" + REVISE_BLOAT→ 回退到最高分版本（45 vs 91），
//                                               落盘初稿（超长修订被回退）
// 基线直通形态（structural 无注入）由 gate:ts 的 node-fallback-smoke 常态覆盖；
// 「未产出新内容」分支以 650 首跑实录/655 红 1 为手工证据（需 mock 直通形态，
// 不入本套件）。任一场景断言失败退出码非零。

import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { existsSync, readdirSync, readFileSync, appendFileSync, mkdtempSync } from "node:fs";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");

const args = process.argv.slice(2);
const basePortIdx = args.indexOf("--base-port");
const basePort = basePortIdx >= 0 ? Number(args[basePortIdx + 1]) : 8797;

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

const results = [];
for (let i = 0; i < SCENARIOS.length; i++) {
  const sc = SCENARIOS[i];
  const port = basePort + i * 2;
  const mockPort = port + 1;
  const root = mkdtempSync(join("/tmp", `rcm-${i}-`));
  const logPath = join(root, "env.log");
  const startedAt = Date.now();
  process.stdout.write(`▶ [${sc.id}] port=${port} … `);

  const child = spawn(process.execPath, [join(scriptDir, "walkthrough-env.mjs"), "--engine", "node", "--port", String(port), "--mock-port", String(mockPort), "--root", root], {
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
  // 等 fixture 完成（数据就绪横幅）
  let ready = false;
  const deadline = Date.now() + 300_000;
  while (Date.now() < deadline) {
    try {
      if (existsSync(logPath)) {
        const log = readFileSync(logPath, "utf-8");
        if (log.includes("走查环境就绪")) { ready = true; break; }
        if (log.includes("✗ fixture")) break;
      }
    } catch { /* log not yet */ }
    await new Promise((r) => setTimeout(r, 5000));
  }

  const checks = [];
  if (ready) {
    // 断言 env 日志关键词
    const log = readFileSync(logPath, "utf-8");
    for (const kw of sc.expect.log) checks.push([`日志含「${kw.slice(0, 18)}…」`, log.includes(kw)]);
    for (const kw of sc.expect.absentLog ?? []) checks.push([`日志不含「${kw.slice(0, 14)}…」`, !log.includes(kw)]);
    // promises 断言（fixture 已断言，这里复核日志行）
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
    checks.push(["env 就绪（300s 内）", false]);
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

console.log("\n──────── 审改循环矩阵汇总 ────────");
let failed = 0;
for (const [id, ok, seconds, checks] of results) {
  console.log(`${ok ? "✓" : "✗"} ${id}（${seconds}s，${checks.length} 断言）`);
  if (!ok) failed += 1;
}
if (failed > 0) {
  console.error(`\n[rcm] ✗ ${failed} 场景失败`);
  process.exit(1);
}
console.log("\n[rcm] ✓ 审改循环矩阵全部通过");
