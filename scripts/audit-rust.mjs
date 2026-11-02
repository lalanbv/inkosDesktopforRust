#!/usr/bin/env node
// Rust 依赖漏洞审计（267 号）。
//
// 对 engine-rs 与 src-tauri 两把 Cargo.lock 各跑一次 `cargo audit`
//（RustSec advisory-db），任一 crate 报 vulnerabilities 即非零退出。
// unmaintained/yanked 等 warning 不拦截（多为传递依赖、无直接修复路径，
// 随上游；是否容忍在调用侧用 --strict 决定）。
//
//   node scripts/audit-rust.mjs                   # 两把锁文件全扫 + 必红自检
//   node scripts/audit-rust.mjs --strict          # warning 也计失败
//   node scripts/audit-rust.mjs --allow-stale-db  # DB 拉取失败时改用本地 DB 扫描（离线逃生门）
//   node scripts/audit-rust.mjs --self-check-only # 只跑必红自检（秒级、离线）
//   node scripts/audit-rust.mjs engine-rs         # 只扫指定 crate 目录（仍跑自检）
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
//
// 847 号必红自检：恒绿与真绿在日志上同形，唯一可靠的区分是喂一个已知红的输入。
// 每次运行末尾用 `cargo audit -n --no-yanked -f fixtures/audit-rust/known-vulnerable.Cargo.lock`
//（离线、秒级）走**同一条 runAudit/parseAudit 路径**，断言检出 ≥1 漏洞；否则判定
// 检测器已退化，本次全部「0 漏洞」结论作废（码 1）。夹具两组独立 crate 防单条
// advisory 被撤回；条目须带 `source = registry+…` 否则 cargo-audit 视为本地包跳过
//（847 号实测夹具自身曾因此恒绿——自检也需要自己的必红证明，见
// INKOS_AUDIT_SELFCHECK_FIXTURE 测试钩子）。

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
const selfCheckOnly = args.includes("--self-check-only");
const targets = args.filter((a) => !a.startsWith("--"));
const crates = targets.length ? targets : ["engine-rs", "src-tauri"];

// 必红夹具；环境变量覆盖**仅供测试自检本身**（喂干净锁证明自检会失败），生产不设。
const SELF_CHECK_FIXTURE =
  process.env.INKOS_AUDIT_SELFCHECK_FIXTURE
  ?? join(scriptDir, "fixtures", "audit-rust", "known-vulnerable.Cargo.lock");

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

/** 跑一次 cargo audit，stdout/stderr 分开捕获（汇总行与拉取失败只在 stderr）。 */
function runAudit(cwd, extraArgs) {
  return spawnSync("cargo", ["audit", ...extraArgs], {
    cwd,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
}

/** 真审计与必红自检共用的唯一解析路径——自检证明的就是这段代码还能看见红。 */
function parseAudit(run) {
  const stdout = run.stdout ?? "";
  const stderr = run.stderr ?? "";
  const vulnMatch = stderr.match(/error: (\d+) vulnerabilities? found/);
  const warnMatch = stderr.match(/warning: (\d+) allowed warnings? found/);
  // 只摘漏洞条目（条目在 stdout，以空行分块；含 `Warning:` 的块是 unmaintained/yanked 警告，剔除）。
  const pickedVulnLines = stdout
    .split(/\n\s*\n/)
    .filter((block) => !/^\s*Warning:/m.test(block))
    .flatMap((block) => block.split("\n"))
    .filter((l) => /^(Crate|Title|ID|Severity|Solution|URL):/.test(l.trim()));
  return {
    status: run.status,
    stderr,
    vulns: vulnMatch ? Number(vulnMatch[1]) : 0,
    warns: warnMatch ? Number(warnMatch[1]) : 0,
    dbFetchFailed: DB_FETCH_FAILURE_RE.test(stderr),
    yankedFlakes: (stderr.match(YANK_NOISE_RE) ?? []).length,
    pickedVulnLines,
  };
}

if (!cargoAuditAvailable()) {
  console.error("✗ 未找到 cargo-audit。请先安装：cargo install cargo-audit");
  console.error("  （审计缺失不应静默通过——如确认跳过，请显式删除本检查）");
  process.exit(3);
}

let failed = false;

if (!selfCheckOnly) {
  for (const crate of crates) {
    const dir = join(repoRoot, crate);
    if (!existsSync(join(dir, "Cargo.lock"))) {
      console.error(`✗ ${crate}: 无 Cargo.lock（跳过非 Rust 目录？）`);
      failed = true;
      continue;
    }

    console.log(`\n== ${crate} ==`);
    // yank 核验保留（yanked=维护者撤包，常含安全/健全性原因；本脚本按 warning 处理）。
    let result = parseAudit(runAudit(dir, []));
    const ageDays = advisoryDbAgeDays();
    const ageLabel = ageDays === null ? "未知" : `${ageDays} 天前`;
    console.log(`  advisory-db 最新提交：${ageLabel}`);

    // DB 拉取失败时 cargo-audit 不扫描直接非零退出——结论未知≠绿灯。
    if (result.dbFetchFailed) {
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
      result = parseAudit(runAudit(dir, ["-n"]));
    }

    if (result.vulns > 0) {
      console.log(result.pickedVulnLines.join("\n"));
      console.error(`✗ ${crate}: ${result.vulns} 个漏洞`);
      failed = true;
      continue;
    }
    if (result.status !== 0) {
      // 非零退出但无漏洞汇总=非预期失败（工具错误/锁文件损坏/…）——绝不静默放行。
      const tail = result.stderr
        .split("\n")
        .filter((l) => l.trim() && !YANK_NOISE_RE.test(l))
        .slice(-6)
        .join("\n    ");
      console.error(`✗ ${crate}: cargo audit 非零退出（code=${result.status}）且无漏洞汇总——\n    ${tail}`);
      failed = true;
      continue;
    }
    console.log(`✓ ${crate}: 0 漏洞${result.warns ? `（${result.warns} 条 unmaintained/yanked 警告）` : ""}`);
    if (result.yankedFlakes > 0) {
      console.log(`  （yanked 联网核验超时 ${result.yankedFlakes} 次——网络抖动，非漏洞；离线环境属正常）`);
    }
    if (strict && result.warns > 0) {
      console.error(`✗ --strict：warning 计失败`);
      failed = true;
    }
  }
}

// ── 必红自检（847 号）：跑在真审计之后（DB 已由上面拉取），-n 离线、--no-yanked 免联网。
console.log(`\n== 必红自检（${SELF_CHECK_FIXTURE.replace(repoRoot + "/", "")}） ==`);
if (!existsSync(SELF_CHECK_FIXTURE)) {
  console.error(`✗ 自检夹具缺失：${SELF_CHECK_FIXTURE}——检测器无法自证，门禁结论不可信`);
  failed = true;
} else {
  const check = parseAudit(runAudit(repoRoot, ["-n", "--no-yanked", "-f", SELF_CHECK_FIXTURE]));
  const sawIds = check.pickedVulnLines.filter((l) => /^ID:\s+RUSTSEC-/.test(l.trim())).length;
  if (check.vulns >= 1 && sawIds >= 1) {
    console.log(`✓ 自检：已知漏洞锁检出 ${check.vulns} 个漏洞（${sawIds} 条 RUSTSEC 条目）——检测器活着`);
  } else {
    const tail = check.stderr.split("\n").filter((l) => l.trim()).slice(-4).join("\n    ");
    console.error(
      `✗ 自检失败：已知漏洞锁未报红（vulns=${check.vulns} ids=${sawIds} code=${check.status}）——`
      + `检测器可能已退化为恒绿（846 号事故同型：工具改了输出流/夹具失配/本地 DB 过旧），`
      + `本次所有「0 漏洞」结论作废\n    ${tail}`,
    );
    failed = true;
  }
}

process.exit(failed ? 1 : 0);
