#!/usr/bin/env node
// Rust lint 门禁（433 号）。
//
// 对 engine-rs 与 src-tauri 两把 crate 各跑一次
// `cargo clippy --all-targets -- -D warnings`。任一 warning/error
// 即非零退出——零告警状态靠门禁锁住，而非靠人肉记得跑。
//
// 必须带 --all-targets：不带则 test/bench/example 目标永远在 lint
// 覆盖外（400 号实测教训：累积 38 处 items_after_test_module 全在测试目标）。
//
//   node scripts/clippy-gate.mjs             # 两把 crate 全扫
//   node scripts/clippy-gate.mjs engine-rs   # 只扫指定 crate 目录
//
// 前置：rustup 的 cargo + clippy 组件（默认 toolchain 自带；缺失时给出
// 提示并以码 3 退出——门禁缺失不应静默绿灯）。
//
// 已知代价：clippy 升级引入新 lint 时门禁可能突然转红——这是门禁的
// 本义（与 bench:gate 同纪律），修掉即可，禁止注 restore/allow 洗门禁。

import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");

const targets = process.argv.slice(2).filter((a) => !a.startsWith("-"));
const crates = targets.length ? targets : ["engine-rs", "src-tauri"];

function clippyAvailable() {
  try {
    execFileSync("cargo", ["clippy", "--version"], { stdio: "pipe" });
    return true;
  } catch {
    return false;
  }
}

if (!clippyAvailable()) {
  console.error("✗ 未找到 cargo-clippy。请先安装：rustup component add clippy");
  console.error("  （门禁缺失不应静默通过——如确认跳过，请显式删除本检查）");
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
  let output = "";
  try {
    execFileSync(
      "cargo",
      ["clippy", "--all-targets", "--", "-D", "warnings"],
      { cwd: dir, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    );
  } catch (err) {
    // -D warnings 下任一告警即非零退出，诊断在 stdout/stderr 各半
    output = `${err.stdout ?? ""}\n${err.stderr ?? ""}`;
  }

  if (output.trim() === "") {
    console.log(`✓ ${crate}: 0 告警`);
    continue;
  }

  const lines = output.split("\n");
  // 只摘诊断行（warning/error 头 + 位置行），避免整段代码块刷屏
  const picked = lines.filter((l) => /^(warning|error)(\[|:)|^\s+-->/.test(l));
  if (picked.length === 0) {
    // 无诊断行 = 编译失败等异常，整体打出末尾帮助定位
    console.error(lines.slice(-30).join("\n"));
  } else {
    console.error(picked.join("\n"));
  }
  console.error(`✗ ${crate}: clippy 门禁拦截（见上）`);
  failed = true;
}

process.exit(failed ? 1 : 0);
