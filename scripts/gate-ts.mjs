#!/usr/bin/env node
// TS 侧统一门禁入口（509 号）：一条命令跑完全部非 Rust 门禁并汇总。
//   node scripts/gate-ts.mjs [--fast]     # --fast 跳过 build 与活体套件
// 步骤（任一失败即标红，最终退出码非零）：
//   1. pnpm -r typecheck          三包双 tsconfig 类型检查
//   2. pnpm -r build              发布产物链（core/cli tsc + studio vite7+server）
//   3. pnpm -r test               core+studio+cli 全量测试
//   4. node scripts/audit-npm.mjs npm 依赖审计（白名单语义）
//   5. node scripts/node-fallback-smoke.mjs   双引擎一致性（mock+fixture+端点）
//   6. node scripts/engine-contract-diff.mjs  GET/写入/DELETE 契约差分
//   7. node scripts/export-epub-smoke.mjs     EPUB 导出结构冒烟
// Rust 门禁（clippy:gate / audit:rust / cargo test / duel / bench）不受本脚本
// 管理——Xcode 许可阻断期间亦不影响本脚本可用性。

import { spawnSync } from "node:child_process";

const args = process.argv.slice(2);
const fast = args.includes("--fast");
const repoRoot = process.cwd();

const steps = [
  ["typecheck", ["pnpm", ["-r", "typecheck"]]],
  ["test", ["pnpm", ["-r", "test"]]],
  ["audit:npm", ["node", ["scripts/audit-npm.mjs"]]],
  ...(fast
    ? []
    : [
        ["build", ["pnpm", ["-r", "build"]]],
        ["node-fallback-smoke", ["node", ["scripts/node-fallback-smoke.mjs"]]],
        ["engine-contract-diff", ["node", ["scripts/engine-contract-diff.mjs"]]],
        ["export-epub-smoke", ["node", ["scripts/export-epub-smoke.mjs"]]],
      ]),
];

const results = [];
let failed = 0;
for (const [name, [cmd, cmdArgs]] of steps) {
  const startedAt = Date.now();
  process.stdout.write(`▶ ${name} ... `);
  const res = spawnSync(cmd, cmdArgs, { cwd: repoRoot, encoding: "utf-8", stdio: ["ignore", "pipe", "pipe"] });
  const seconds = ((Date.now() - startedAt) / 1000).toFixed(1);
  if (res.status === 0) {
    console.log(`✓ ${seconds}s`);
    results.push([name, true, seconds]);
  } else {
    failed += 1;
    console.log(`✗ ${seconds}s`);
    results.push([name, false, seconds]);
    if (res.stdout) console.log(res.stdout.slice(-1500));
    if (res.stderr) console.error(res.stderr.slice(-1500));
  }
}

console.log("\n──────── 门禁汇总 ────────");
for (const [name, ok, seconds] of results) {
  console.log(`${ok ? "✓" : "✗"} ${name} (${seconds}s)`);
}
if (failed > 0) {
  console.error(`[gate-ts] ✗ ${failed} 项失败`);
  process.exit(1);
}
console.log("[gate-ts] ✓ 全部门禁通过");
