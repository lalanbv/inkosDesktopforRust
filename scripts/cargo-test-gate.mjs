#!/usr/bin/env node
// Rust 全目标测试门禁（561 号）。
//
// 对 engine-rs 与 src-tauri 两把 crate 各跑一次 `cargo test --all-targets`
// 并从落盘日志解析每个目标的 test result 汇总——任一 FAILED 即非零退出。
//
// 为什么需要它（560 号教训第二例）：`cargo test --lib` 只覆盖 lib 内嵌
// #[cfg(test)] 单测，tests/ 目录的独立 test 目标（e2e/golden/duel 全族）
// 永远在覆盖外——556 号的 research85 回归在 --lib 门禁全绿下潜伏了两轮，
// 直到 clippy --all-targets 触碰 test 编译才暴露（569 号「cargo check 不
// 覆盖 test 目标」同类教训）。本门禁把「跑过所有目标」从人肉纪律变成
// 一条命令。
//
//   node scripts/cargo-test-gate.mjs             # 两把 crate 全跑
//   node scripts/cargo-test-gate.mjs engine-rs   # 只跑指定 crate 目录
//
// 前置与环境：
// - engine-rs 侧必须带 INKOS_DUEL=1——strangler_duel 未设 env 时早退也计
//   pass（170/172 号假绿事故；--all-targets 会把它一并跑掉，故 env 在
//   本脚本内固定注入，不依赖调用方记得）。
// - 不含 doc-tests（口径与 `cargo test --lib` 计数惯例一致；538 号
//   576vs578 口径差备案）。
// - 已知代价：test 目标编译缓存冷时首轮耗时数分钟——门禁本义，与
//   clippy:gate 同纪律，禁止为提速注 skip/过滤洗门禁。

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");

const targets = process.argv.slice(2).filter((a) => !a.startsWith("-"));
const crates = targets.length ? targets : ["engine-rs", "src-tauri"];

function cargoAvailable() {
  try {
    execFileSync("cargo", ["--version"], { stdio: "pipe" });
    return true;
  } catch {
    return false;
  }
}

if (!cargoAvailable()) {
  console.error("✗ 未找到 cargo。请先安装 rustup（门禁缺失不应静默绿灯）");
  process.exit(3);
}

let failed = false;

for (const crate of crates) {
  const dir = join(repoRoot, crate);
  if (!existsSync(join(dir, "Cargo.toml"))) {
    console.error(`✗ ${crate}: 无 Cargo.toml（目录名写错？）`);
    failed = true;
    continue;
  }

  console.log(`\n== ${crate} ==`);
  // 统计落盘再解析——cargo 管道统计伪象两次实证（532/552 号），禁 |grep|awk。
  const logFile = join(mkdtempSync(join(tmpdir(), "cargo-test-gate-")), "output.log");
  const env = { ...process.env };
  if (crate === "engine-rs") env.INKOS_DUEL = "1";
  const run = spawnSync("cargo", ["test", "--all-targets"], {
    cwd: dir,
    encoding: "utf8",
    env,
    maxBuffer: 64 * 1024 * 1024,
  });
  const output = `${run.stdout ?? ""}\n${run.stderr ?? ""}`;
  try {
    writeFileSync(logFile, output);
  } catch {
    // 落盘失败不阻断——解析走内存副本
  }

  const resultLines = output.split("\n").filter((l) => /^test result: /.test(l));
  const oks = resultLines.filter((l) => l.includes("ok.")).length;
  const failedLines = resultLines.filter((l) => l.includes("FAILED"));
  const passedTotal = resultLines.reduce(
    (sum, l) => sum + (Number(/^test result: ok\.\s+(\d+) passed/.exec(l)?.[1] ?? 0)),
    0,
  );

  if (run.status !== 0 || failedLines.length > 0 || oks === 0) {
    const lines = output.split("\n");
    const picked = lines.filter(
      (l) => /^test result: (ok\.\s+\d+ passed)?/.test(l) === false && /panicked|FAILED|^test .* \.\.\. FAILED|^error/.test(l),
    );
    console.error(picked.slice(0, 60).join("\n") || lines.slice(-40).join("\n"));
    console.error(`✗ ${crate}: 测试门禁拦截（${failedLines.length}/${resultLines.length} 目标 FAILED；详见 ${logFile}）`);
    failed = true;
    continue;
  }

  console.log(`✓ ${crate}: ${oks} 个目标全绿，累计 ${passedTotal} passed（日志 ${logFile}）`);
}

process.exit(failed ? 1 : 0);
