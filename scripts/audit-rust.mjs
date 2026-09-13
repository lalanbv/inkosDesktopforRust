#!/usr/bin/env node
// Rust 依赖漏洞审计（267 号）。
//
// 对 engine-rs 与 src-tauri 两把 Cargo.lock 各跑一次 `cargo audit`
//（RustSec advisory-db），任一 crate 报 vulnerabilities 即非零退出。
// unmaintained/yanked 等 warning 不拦截（多为传递依赖、无直接修复路径，
// 随上游；是否容忍在调用侧用 --strict 决定）。
//
//   node scripts/audit-rust.mjs             # 两把锁文件全扫
//   node scripts/audit-rust.mjs --strict    # warning 也计失败
//   node scripts/audit-rust.mjs engine-rs   # 只扫指定 crate 目录
//
// 前置：cargo-audit（`cargo install cargo-audit`；本机未装时给出提示并以
// 码 3 退出——审计缺失不应静默绿灯）。

import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");

const args = process.argv.slice(2);
const strict = args.includes("--strict");
const targets = args.filter((a) => !a.startsWith("--"));
const crates = targets.length ? targets : ["engine-rs", "src-tauri"];

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
  let output;
  try {
    // 经 sh -c 在子进程内部丢弃 stderr：cargo-audit 的 yanked 联网核验会
    // 直写 fd2（穿透 execFileSync 管道），数 MB 噪声刷屏；advisory 结论全在
    // stdout，stderr 无可保留信息。
    output = execFileSync("sh", ["-c", "cargo audit 2>/dev/null"], {
      cwd: dir,
      encoding: "utf8",
    });
  } catch (err) {
    // cargo audit 发现漏洞时以非零退出，stdout 混在 err 里
    output = `${err.stdout ?? ""}`;
  }

  const vulnMatch = output.match(/error: (\d+) vulnerabilities? found/);
  const warnMatch = output.match(/warning: (\d+) allowed warnings? found/);
  const vulns = vulnMatch ? Number(vulnMatch[1]) : 0;
  const warns = warnMatch ? Number(warnMatch[1]) : 0;

  if (vulns > 0) {
    // 只摘漏洞条目（Crate/Title/ID/Solution 行），避免整段通告刷屏
    const lines = output.split("\n");
    const picked = lines.filter((l) =>
      /^(Crate|Title|ID|Solution|URL):/.test(l.trim()),
    );
    console.log(picked.join("\n"));
    console.error(`✗ ${crate}: ${vulns} 个漏洞`);
    failed = true;
  } else {
    // yanked 核验走 crates.io 网络，镜像/离线时常超时——折叠为一条提示，非漏洞
    const yankedFlakes = (output.match(/couldn't check if the package is yanked/g) ?? []).length;
    console.log(`✓ ${crate}: 0 漏洞${warns ? `（${warns} 条 unmaintained/yanked 警告）` : ""}`);
    if (yankedFlakes > 0) {
      console.log(`  （yanked 联网核验超时 ${yankedFlakes} 次——网络抖动，非漏洞；离线环境属正常）`);
    }
    if (strict && warns > 0) {
      console.error(`✗ --strict：warning 计失败`);
      failed = true;
    }
  }
}

process.exit(failed ? 1 : 0);
