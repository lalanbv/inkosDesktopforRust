/**
 * W-C2（172 号）：ts-rs 绑定导出健康门禁。
 *
 * Usage:
 *   node scripts/verify-engine-bindings.mjs
 *   pnpm verify:engine-bindings
 *
 * 实施期复核（对齐总体方案 v1 的修正先例）：原案为「导出产物 vs 入库
 * bindings 的 git diff」，但 bindings 目录被 engine-rs/.gitignore 有意
 * 忽略且前端暂零消费——diff 门禁没有对象。本脚本改为导出健康门禁：
 *
 * 1. `cargo test --features export-bindings -- export`（按名过滤 ts-rs
 *    生成的 export_bindings_* 测试）——验证全部类型定义仍可导出为
 *    合法 TS（类型/依赖/版本兼容性回归在此暴露）；
 * 2. 导出后 engine-rs/bindings 文件数非空（产出物完整落盘）。
 *
 * 「入库 + git diff」形态登记为条件项：待前端开始消费 bindings 时启用
 * （届时移除 engine-rs/.gitignore 的 /bindings/ 条目并改为 diff 检查）。
 */

import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");
const manifestPath = join(repoRoot, "engine-rs", "Cargo.toml");
const bindingsDir = join(repoRoot, "engine-rs", "bindings");

process.stderr.write("== ts-rs binding export gate (W-C2) ==\n");

// 1) 导出测试（名字过滤只跑 export_bindings_*，避免全量 1500 用例）。
const started = Date.now();
const result = spawnSync(
  "cargo",
  [
    "test",
    `--manifest-path=${manifestPath}`,
    "--features=export-bindings",
    "--lib",
    "--",
    "export",
  ],
  { stdio: "inherit", env: process.env },
);
if (result.status !== 0) {
  process.stderr.write(
    "FAIL: cargo test --features export-bindings -- export exited non-zero — 绑定导出测试未全绿（类型定义或 ts-rs 兼容性回归）\n",
  );
  process.exit(result.status ?? 1);
}

// 2) 产出物完整性（ts-rs 在上述测试内写盘）。
let tsFiles = [];
try {
  tsFiles = readdirSync(bindingsDir).filter((f) => f.endsWith(".ts"));
} catch {
  // 目录不存在（从未导出）——按空产出处理。
}
if (tsFiles.length === 0) {
  process.stderr.write(`FAIL: ${bindingsDir} 无 .ts 产出 — 导出测试通过但未落盘？\n`);
  process.exit(1);
}

const seconds = ((Date.now() - started) / 1000).toFixed(1);
process.stderr.write(`OK: ${tsFiles.length} bindings 导出全绿（${seconds}s）\n`);
