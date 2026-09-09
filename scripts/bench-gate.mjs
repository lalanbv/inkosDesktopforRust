#!/usr/bin/env node
// 177 号 W-D4：bench 常态化本地门禁。
//
// 跑 engine-rs 热路径基准（criterion），与入库基线 scripts/bench-baseline.json
// 对比，任一 bench 均值超阈值（默认 +30%）即非零退出。
//
//   node scripts/bench-gate.mjs              # 跑 bench + 对比门禁
//   node scripts/bench-gate.mjs --update     # 跑 bench + 刷新基线（优化落地后显式执行）
//   node scripts/bench-gate.mjs --threshold 1.5
//   node scripts/bench-gate.mjs --list       # 只看基线不跑 bench
//
// 负载前置检查（257 号遗留 #2）：criterion 采样对 CPU 竞争极敏感——实测
// load≈4.6/18 核（约 0.23/核）时满跑门禁即出现 +190%~+520% 的假回退，且每轮
// 漂移的基准各不相同。故 1 分钟 loadavg 占核数比 > maxLoad（默认 0.20）时拒绝
// 执行：静默时段重跑，或 --ignore-load 强跑（结果自担）。Windows 无 loadavg
// （恒 [0,0,0]）自动通过。阈值可 `--max-load 0.3` 或 env BENCH_GATE_MAX_LOAD 调整。
//
// 基线是机器相关的（同机对比才有意义）——换机后请 --update 重建。
// criterion 采样默认较慢（数分钟）；接受它，门禁的意义就在可信均值。

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");
const engineDir = join(repoRoot, "engine-rs");
const baselinePath = join(scriptDir, "bench-baseline.json");
const criterionDir = join(engineDir, "target", "criterion");

const args = process.argv.slice(2);
const doUpdate = args.includes("--update");
const listOnly = args.includes("--list");
const thresholdIdx = args.indexOf("--threshold");
const threshold = thresholdIdx >= 0 ? Number(args[thresholdIdx + 1]) : 1.3;
// `--` 之后的参数透传 cargo bench（如 `-- --quick`、
// `-- --warm-up-time 1 --measurement-time 1`——快速自检用，基线请全量跑）。
const dashIdx = args.indexOf("--");
const benchPassThrough = dashIdx >= 0 ? args.slice(dashIdx + 1) : [];

if (!Number.isFinite(threshold) || threshold <= 1) {
  console.error(`无效阈值：${args[thresholdIdx + 1]}（须 > 1）`);
  process.exit(2);
}

// 负载前置检查（257 号遗留 #2）：满载下 criterion 均值不可信，拒绝比污染基线好。
const ignoreLoad = args.includes("--ignore-load");
const maxLoadIdx = args.indexOf("--max-load");
const maxLoad = maxLoadIdx >= 0
  ? Number(args[maxLoadIdx + 1])
  : Number(process.env.BENCH_GATE_MAX_LOAD ?? 0.2);
if (!Number.isFinite(maxLoad) || maxLoad <= 0) {
  console.error(`无效负载阈值：${maxLoadIdx >= 0 ? args[maxLoadIdx + 1] : process.env.BENCH_GATE_MAX_LOAD}（须 > 0）`);
  process.exit(2);
}

function checkMachineLoad() {
  if (ignoreLoad) return;
  const [load1] = os.loadavg();
  const cores = os.cpus().length || 1;
  const ratio = load1 / cores;
  if (ratio <= maxLoad) return;
  console.error(
    `✗ 机器负载过高：1 分钟 loadavg ${load1.toFixed(2)} / ${cores} 核 = ${(ratio * 100).toFixed(0)}%/核，超过阈值 ${(maxLoad * 100).toFixed(0)}%（--max-load）。`,
  );
  console.error("  criterion 采样对 CPU 竞争极敏感，满载下会出现假回退/假通过（257 号实测漂移 +190%~+520%）。");
  console.error("  请在静默时段重跑；确要强跑加 --ignore-load。");
  process.exit(3);
}

// criterion 目录名 = bench id 的路径形态（group/function[/value]）；
// estimates.json 只存在于叶子的 new/ 上一级。
function discoverBenchIds() {
  const ids = [];
  const walk = (dir, prefix) => {
    let entries;
    try {
      entries = readdirSync(dir);
    } catch {
      return;
    }
    for (const entry of entries) {
      if (entry === "report" || entry.startsWith(".")) continue;
      const full = join(dir, entry);
      const id = prefix ? `${prefix}/${entry}` : entry;
      if (existsSync(join(full, "new", "estimates.json"))) {
        ids.push(id);
      } else {
        walk(full, id);
      }
    }
  };
  walk(criterionDir, "");
  return ids.sort();
}

function readMeanNs(benchId) {
  const estimatesPath = join(criterionDir, ...benchId.split("/"), "new", "estimates.json");
  const raw = JSON.parse(readFileSync(estimatesPath, "utf-8"));
  return raw.mean.point_estimate;
}

function fmtNs(ns) {
  if (ns >= 1e6) return `${(ns / 1e6).toFixed(2)} ms`;
  if (ns >= 1e3) return `${(ns / 1e3).toFixed(2)} µs`;
  return `${ns.toFixed(2)} ns`;
}

try {
  const baseline = existsSync(baselinePath)
    ? JSON.parse(readFileSync(baselinePath, "utf-8"))
    : null;

  if (listOnly) {
    if (!baseline) {
      console.log("（无基线文件——先跑一次门禁自动采集）");
      process.exit(0);
    }
    console.log(`基线采集于 ${baseline.collectedAt}（${baseline.host}）`);
    for (const [id, ns] of Object.entries(baseline.benches)) {
      console.log(`  ${id.padEnd(46)} ${fmtNs(ns)}`);
    }
    process.exit(0);
  }

  checkMachineLoad();
  console.log("跑 bench：cargo bench --bench hot_paths（engine-rs，criterion 采样需数分钟）…");
  execFileSync("cargo", ["bench", "--bench", "hot_paths", "--", ...benchPassThrough], {
    cwd: engineDir,
    stdio: "inherit",
  });

  const benchIds = discoverBenchIds();
  if (!benchIds.length) {
    console.error("未发现任何 criterion 结果（target/criterion 为空）");
    process.exit(2);
  }

  const current = Object.fromEntries(benchIds.map((id) => [id, readMeanNs(id)]));

  if (!baseline || doUpdate) {
    writeFileSync(
      baselinePath,
      `${JSON.stringify(
        {
          collectedAt: new Date().toISOString(),
          host: `${os.platform()}/${os.arch()}`,
          benches: current,
        },
        null,
        2,
      )}\n`,
    );
    console.log(`\n${doUpdate ? "基线已刷新" : "首次基线已写入"}：${baselinePath}`);
    for (const [id, ns] of Object.entries(current)) {
      console.log(`  ${id.padEnd(46)} ${fmtNs(ns)}`);
    }
    process.exit(0);
  }

  // 对比门禁。
  const rows = [];
  const regressions = [];
  const additions = [];
  for (const id of benchIds) {
    if (!(id in baseline.benches)) {
      additions.push(id);
      continue;
    }
    const base = baseline.benches[id];
    const now = current[id];
    const ratio = now / base;
    rows.push({ id, base, now, ratio });
    if (ratio > threshold) regressions.push({ id, base, now, ratio });
  }
  const removed = Object.keys(baseline.benches).filter((id) => !benchIds.includes(id));

  console.log(`\n基线采集于 ${baseline.collectedAt}（${baseline.host}），阈值 +${Math.round((threshold - 1) * 100)}%`);
  for (const { id, base, now, ratio } of rows) {
    const pct = ((ratio - 1) * 100).toFixed(1);
    const mark = ratio > threshold ? " ✗" : ratio < 1 ? `（${pct}%）` : `（+${pct}%）`;
    console.log(`  ${id.padEnd(46)} ${fmtNs(base)} → ${fmtNs(now)}  ${mark}`);
  }
  for (const id of additions) console.log(`  + 新增 bench（未在基线，跑 --update 登记）：${id}`);
  for (const id of removed) console.log(`  - 基线含此 bench 但已不存在（跑 --update 清理）：${id}`);

  if (regressions.length) {
    console.error(`\n✗ ${regressions.length} 个 bench 超阈值回退：`);
    for (const { id, base, now, ratio } of regressions) {
      console.error(`    ${id}: ${fmtNs(base)} → ${fmtNs(now)}（+${((ratio - 1) * 100).toFixed(1)}%）`);
    }
    console.error("确属优化后请 node scripts/bench-gate.mjs --update 刷新基线。");
    process.exit(1);
  }
  console.log("\n✓ bench 门禁通过");
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(2);
}
