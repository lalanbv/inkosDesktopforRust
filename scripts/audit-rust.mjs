#!/usr/bin/env node
// Rust 依赖漏洞审计（267 号）。
//
// 对 engine-rs 与 src-tauri 两把 Cargo.lock 各跑一次 `cargo audit`
//（RustSec advisory-db），任一 crate 报 vulnerabilities 即非零退出。
// unmaintained/yanked 等 warning 不拦截（多为传递依赖、无直接修复路径，
// 随上游；是否容忍在调用侧用 --strict 决定）。
//
//   node scripts/audit-rust.mjs                 # 两把锁文件全扫
//   node scripts/audit-rust.mjs --strict        # warning 也计失败
//   node scripts/audit-rust.mjs --allow-stale-db  # DB 拉取失败时降级为显式警告（离线逃生门）
//   node scripts/audit-rust.mjs engine-rs       # 只扫指定 crate 目录
//
// 前置：cargo-audit（`cargo install cargo-audit`；本机未装时给出提示并以
// 码 3 退出——审计缺失不应静默绿灯）。
//
// 846 号恒绿事故修补：此前 `sh -c "cargo audit 2>/dev/null"` 整段丢弃 stderr，
// 而 cargo-audit 0.22 把汇总行 `error: N vulnerabilities found!` 打在 **stderr**
//（stdout 只有条目细节）——vulnMatch 永远匹配不到，任何情况下都「✓ 0 漏洞」；
// advisory DB 经代理/镜像拉取失败（stderr `error: couldn't fetch advisory
// database`，不扫描直接非零退出）同样被吞成绿灯。zip ≤8 四条 2026-10 advisory
//（含 9.1 critical）挂在网上数周，直到巡检直连刷新 DB 手跑 cargo audit 才撞出。
// 现：汇总行从 stderr 解析；非零退出永不放行；DB 拉取失败=未知≠绿灯，默认硬
// 失败（码 1），`--allow-stale-db` 显式逃生（改用 -n 本地 DB 扫描并标注窗口）；
// 输出 advisory-db 最新提交日期作为新鲜度证据。yank 联网核验超时属已知噪声，折叠计数。

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");

const args = process.argv.slice(2);
const strict = args.includes("--strict");
const allowStaleDb = args.includes("--allow-stale-db");
const targets = args.filter((a) => !a.startsWith("--"));
const crates = targets.length ? targets : ["engine-rs", "src-tauri"];

// stderr 里与 DB 拉取无关的已折叠噪声（yank 联网核验超时；镜像/离线环境常见）。
const YANK_NOISE_RE = /couldn't check if the package is yanked/g;
const DB_FETCH_FAILURE_RE = /couldn't (?:fetch|update) advisory database/;

function advisoryDbAgeDays() {
  const dbDir = join(homedir(), ".cargo", "advisory-db");
  if (!existsSync(dbDir)) return null;
  try {
    const date = execFileSync("git", ["-C", dbDir, "log", "-1", "--format=%cI"], {
      encoding: "utf8",
    }).trim();
    const seconds = (Date.now() - Date.parse(date)) / 1000;
    return Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds / 86400)) : null;
  } catch {
    return null;
  }
}

function cargoAuditAvailable() {
  try {
    execFileSync("cargo", ["audit", "--version"], { stdio: "pipe" });
    return true;
  } catch {
    return false;
  }
}

if (!cargoAuditAvailable()) {
  console.error("✗ 未找到 cargo-audit。请先安装：cargo install cargo-audit");
  console.error("  （审计缺失不应静默通过——如确认跳过，请显式删除本检查）");
  process.exit(3);
}

let failed = false;

for (const crate of crates) {
  const dir = join(repoRoot, crate);
  if (!existsSync(join(dir, "Cargo.lock"))) {
    console.error(`✗ ${crate}: 无 Cargo.lock（跳过非 Rust 目录？）`);
    failed = true;
    continue;
  }

  console.log(`\n== ${crate} ==`);
  // stdout+stderr 分开捕获，且**汇总行在 stderr**（cargo-audit 0.22：advisory 条目细节走 stdout，
  // `error: N vulnerabilities found!` / `warning: N allowed warnings found` / DB 拉取失败走 stderr）。
  // 旧脚本整段丢弃 stderr 后 vulnMatch 永远匹配不到 → 任何情况下都「✓ 0 漏洞」（846 号恒绿事故）。
  // yank 核验保留（yanked=维护者撤包，常含安全/健全性原因；本脚本按 warning 处理）。
  const spawnAudit = (extra) =>
    spawnSync("cargo", ["audit", ...extra], {
      cwd: dir,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    });
  let run = spawnAudit([]);
  const ageDays = advisoryDbAgeDays();
  const ageLabel = ageDays === null ? "未知" : `${ageDays} 天前`;
  console.log(`  advisory-db 最新提交：${ageLabel}`);

  // DB 拉取失败时 cargo-audit 不扫描直接非零退出——结论未知≠绿灯。
  if (DB_FETCH_FAILURE_RE.test(run.stderr ?? "")) {
    if (!allowStaleDb) {
      console.error(
        `✗ ${crate}: advisory DB 拉取失败——未扫描（本地 DB 最新提交 ${ageLabel}），未知≠绿灯。`
        + `检查代理/镜像网络后重跑；确需离线放行请显式 --allow-stale-db（改用本地 DB 扫描）`,
      );
      failed = true;
      continue;
    }
    console.error(
      `⚠ ${crate}: advisory DB 拉取失败，--allow-stale-db 已显式开启——改用本地 DB`
      + `（最新提交 ${ageLabel}）扫描，结论仅在该窗口内可信`,
    );
    run = spawnAudit(["-n"]);
  }

  const stdout = run.stdout ?? "";
  const stderr = run.stderr ?? "";
  const yankedFlakes = (stderr.match(YANK_NOISE_RE) ?? []).length;
  const vulnMatch = stderr.match(/error: (\d+) vulnerabilities? found/);
  const warnMatch = stderr.match(/warning: (\d+) allowed warnings? found/);
  const vulns = vulnMatch ? Number(vulnMatch[1]) : 0;
  const warns = warnMatch ? Number(warnMatch[1]) : 0;

  if (vulns > 0) {
    // 只摘漏洞条目（Crate/Title/ID/Severity/Solution 行，条目在 stdout），避免整段通告刷屏；
    // 条目以空行分块，含 `Warning:` 的块是 unmaintained/yanked 警告而非漏洞，剔除。
    const picked = stdout
      .split(/\n\s*\n/)
      .filter((block) => !/^\s*Warning:/m.test(block))
      .flatMap((block) => block.split("\n"))
      .filter((l) => /^(Crate|Title|ID|Severity|Solution|URL):/.test(l.trim()));
    console.log(picked.join("\n"));
    console.error(`✗ ${crate}: ${vulns} 个漏洞`);
    failed = true;
    continue;
  }
  if (run.status !== 0) {
    // 非零退出但无漏洞汇总=非预期失败（工具错误/锁文件损坏/…）——绝不静默放行。
    const tail = stderr
      .split("\n")
      .filter((l) => l.trim() && !YANK_NOISE_RE.test(l))
      .slice(-6)
      .join("\n    ");
    console.error(`✗ ${crate}: cargo audit 非零退出（code=${run.status}）且无漏洞汇总——\n    ${tail}`);
    failed = true;
    continue;
  }
  console.log(`✓ ${crate}: 0 漏洞${warns ? `（${warns} 条 unmaintained/yanked 警告）` : ""}`);
  if (yankedFlakes > 0) {
    console.log(`  （yanked 联网核验超时 ${yankedFlakes} 次——网络抖动，非漏洞；离线环境属正常）`);
  }
  if (strict && warns > 0) {
    console.error(`✗ --strict：warning 计失败`);
    failed = true;
  }
}

process.exit(failed ? 1 : 0);
