#!/usr/bin/env node
// Rust 格式门禁（1487 号）。
//
// 对 engine-rs 与 src-tauri 两把 crate 各跑一次 `cargo fmt --check`。
// 任一格式漂移即非零退出——1487 全仓清偿（1478 备案的 6223 处历史
// 噪音）后零漂移状态靠门禁锁住，而非靠人肉记得跑。
//
//   node scripts/fmt-gate.mjs             # 两把 crate 全扫
//   node scripts/fmt-gate.mjs engine-rs   # 只查指定 crate 目录
//
// 前置：rustup 的 cargo + rustfmt 组件（缺失时提示并以码 3 退出——
// 门禁缺失不应静默绿灯，同 clippy-gate 纪律）。
//
// 已知边缘：文件头空行是 rustfmt 非幂等区（fmt 不修但 --check 报，
// 1487 实测 src/engine/node/tests.rs）——此类情况手改文件头即可，
// 禁止注 #[rustfmt::skip] 洗门禁。

import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");

const targets = process.argv.slice(2).filter((a) => !a.startsWith("-"));
const crates = targets.length ? targets : ["engine-rs", "src-tauri"];

function rustfmtAvailable() {
  try {
    execFileSync("cargo", ["fmt", "--version"], { stdio: "pipe" });
    return true;
  } catch {
    return false;
  }
}

if (!rustfmtAvailable()) {
  console.error("✗ 未找到 rustfmt。请先安装：rustup component add rustfmt");
  console.error("  （门禁缺失不应静默通过——如确认跳过，请显式删除本检查）");
  process.exit(3);
}

// 必红自检（847 纪律）：夹具 crate 故意未格式化，--check 必须检出——
// 夹具跑出绿 = 检测路径失效，门禁拒绝放行（码 3）。
const fixture = join(repoRoot, "scripts", "fixtures", "fmt-gate");
let fixtureSeesDrift = false;
try {
  execFileSync("cargo", ["fmt", "--check"], {
    cwd: fixture,
    stdio: ["ignore", "pipe", "pipe"],
  });
} catch {
  fixtureSeesDrift = true;
}
if (!fixtureSeesDrift) {
  console.error("✗ 必红自检失败：未格式化夹具未被检出——检测器失效，拒绝放行");
  process.exit(3);
}
console.log("== 必红自检（scripts/fixtures/fmt-gate） ==");
console.log("✓ 自检：未格式化夹具检出漂移——检测器活着");

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
    execFileSync("cargo", ["fmt", "--check"], {
      cwd: dir,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (err) {
    // 任一漂移即非零退出，diff 全量在 stdout
    output = err.stdout ?? "";
  }

  const drifts = output.match(/^Diff in .+$/gm) ?? [];
  if (drifts.length === 0) {
    console.log(`✓ ${crate}: 0 格式漂移`);
    continue;
  }

  // 摘漂移位置行（文件:行），完整 diff 略——本地跑 cargo fmt 即可清偿
  console.error(drifts.slice(0, 20).join("\n"));
  if (drifts.length > 20) console.error(`…共 ${drifts.length} 处`);
  console.error(`✗ ${crate}: ${drifts.length} 处格式漂移（跑 cargo fmt 清偿）`);
  failed = true;
}

process.exit(failed ? 1 : 0);
