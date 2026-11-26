// R63/1691 号：增量变异门（首版口径=只报告不拦；对标 S1 cargo-mutants
// `--in-diff`、`--baseline=skip`）。取 git diff 触及的 engine-rs/src 区域（或
// --files 显式清单）跑 cargo mutants，输出 caught/missed/unviable 报告。
// missed 数只许下降（1664 号「基线只许缩短」惯例）——基线文件由 --baseline 传，
// 缺省不做基线比对（spike 期）。
//
// 用法：
//   node scripts/mutation-gate.mjs                     # git diff 触及区域
//   node scripts/mutation-gate.mjs --files a.rs,b.rs   # 显式文件清单
//   node scripts/mutation-gate.mjs --baseline out.json # 与上次报告比对
//
// 依赖：cargo-mutants（开发机工具，cargo install cargo-mutants——不进 Cargo.lock）。
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const args = process.argv.slice(2);
const flag = (name) => {
  const index = args.indexOf(`--${name}`);
  return index >= 0 ? args[index + 1] : undefined;
};
const filesFlag = flag("files");
const baselinePath = flag("baseline");
const timeoutSecs = Number(flag("timeout") ?? 1800);

// ── 触及区域：显式清单优先，否则 git diff（未提交+HEAD 一笔）──
const touched = new Set();
if (filesFlag) {
  for (const file of filesFlag.split(",")) touched.add(file.trim());
} else {
  const diff = (args2) => {
    try {
      return execFileSync("git", ["diff", ...args2, "--name-only", "--", "engine-rs/src"], { encoding: "utf8" });
    } catch {
      return "";
    }
  };
  for (const line of [...diff([]).split("\n"), ...diff(["HEAD"]).split("\n")]) {
    const file = line.trim();
    if (file.endsWith(".rs")) touched.add(file);
  }
}
const targets = [...touched].filter((file) => existsSync(file));
if (targets.length === 0) {
  console.log("[mutation-gate] 触及区域为空（无 engine-rs/src 的 .rs 变更）——无事可做，exit 0。");
  process.exit(0);
}
console.log(`[mutation-gate] 触及文件 ${targets.length} 个：\n  ${targets.join("\n  ")}`);

// ── cargo mutants（--baseline skip：未覆盖=不算 caught，与只报告口径一致；
// v27 用 -f glob 过滤——每触及文件一个 -f；--cargo-test-arg=--lib 限定测试
// 目标提速 spike——mutants 默认全目标测试）──
console.log("[mutation-gate] cargo mutants 运行中（--baseline skip，超时上限 " + timeoutSecs + "s）……");
const mutants = spawnSync(
  "cargo",
  [
    "mutants",
    "--baseline",
    "skip",
    "--no-shuffle",
    // -f glob 相对 spawn cwd（engine-rs）——剥仓库根前缀。
    ...targets.flatMap((file) => ["-f", file.replace(/^engine-rs\//u, "")]),
    // --in-place：在原树变异（include_str! 的跨仓 golden 相对路径在 mutants
    // 的拷贝临时树里断裂——全部变异 Unviable；--in-place 原树路径完好）。
    "--in-place",
    "--timeout",
    String(Math.min(timeoutSecs, 300)),
    "--cargo-test-arg=--lib",
  ],
  {
    cwd: "engine-rs",
    encoding: "utf8",
    timeout: timeoutSecs * 1000,
    maxBuffer: 64 * 1024 * 1024,
  },
);
// v27 结果：mutants.out/outcomes.json = { outcomes: [{ scenario: { Mutant: {
//   name, file, function: { function_name } } }, outcome: "Caught"|"Missed"|...
// }], ... }（被 timeout 打断时已出结果者照写）。调用约定：node 必须从仓库根
// 运行（targets 的 existsSync 与 mutants.out 读取都按仓库根相对解析）。
const report = { generatedAt: new Date().toISOString(), targets, caught: [], missed: [], unviable: [], timeout: mutants.status !== 0, incomplete: false };
try {
  const parsed = JSON.parse(readFileSync("engine-rs/mutants.out/outcomes.json", "utf8"));
  const outcomes = Array.isArray(parsed) ? parsed : parsed.outcomes ?? [];
  let withOutcome = 0;
  for (const entry of outcomes) {
    const mutant = entry.scenario?.Mutant;
    // v27 的结果键是 summary（值带 Mutant 后缀：CaughtMutant/MissedMutant/…；
    // Baseline 行跳过）。
    const outcome = String(entry.summary ?? "");
    if (!mutant || outcome === "Baseline" || outcome === "") continue;
    withOutcome += 1;
    const row = { file: mutant.file, function: mutant.function?.function_name ?? "?", mutation: mutant.name };
    if (outcome.startsWith("Caught")) report.caught.push(row);
    else if (outcome.startsWith("Missed")) report.missed.push(row);
    else report.unviable.push({ ...row, outcome });
  }
  if (withOutcome < report.caught.length + report.missed.length + report.unviable.length || report.timeout) {
    report.incomplete = true;
  }
} catch (error) {
  console.error(`[mutation-gate] mutants.out/outcomes.json 读取失败（${error.message}）——原始 stdout 尾部：\n${(mutants.stdout ?? "").slice(-4000)}`);
}

// ── 报告 ──
console.log(`\n[mutation-gate] 报告：caught=${report.caught.length} missed=${report.missed.length} unviable/timeout=${report.unviable.length}${report.incomplete ? "（跑未完整——超时打断，已出结果者入账）" : ""}`);
for (const row of report.missed) {
  console.log(`  MISSED ${row.file} :: ${row.function} :: ${row.mutation}`);
}
if (baselinePath && existsSync(baselinePath)) {
  const baseline = JSON.parse(readFileSync(baselinePath, "utf8"));
  const key = (row) => `${row.file}::${row.function}::${row.mutation}`;
  const baselineKeys = new Set(baseline.missed.map(key));
  const newMissed = report.missed.filter((row) => !baselineKeys.has(key(row)));
  console.log(`\n[mutation-gate] 基线比对：上次 missed=${baseline.missed.length} 本次=${report.missed.length}（只许下降）`);
  for (const row of newMissed) console.log(`  NEW MISSED ${key(row)}`);
  if (newMissed.length > 0) console.log("[mutation-gate] 有新增 missed——报告（首版口径不拦，但须归三类处置）");
} else {
  writeFileSync("mutation-report.json", JSON.stringify(report, null, 1));
  console.log("[mutation-gate] 报告已写 mutation-report.json（下次可用 --baseline mutation-report.json 比对）");
}
// 首版口径：只报告不拦——恒 exit 0。
process.exit(0);
