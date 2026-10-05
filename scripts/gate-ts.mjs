#!/usr/bin/env node
// TS 侧统一门禁入口（509 号）：一条命令跑完全部非 Rust 门禁并汇总。
//   node scripts/gate-ts.mjs [--fast]     # --fast 跳过 build 与活体套件
// 步骤（任一失败即标红，最终退出码非零；587 号 build 前置为第一步）：
//   1. pnpm -r build              发布产物链（core/cli tsc + studio vite7+server）
//   2. pnpm -r typecheck          三包双 tsconfig 类型检查（消费新鲜 dist）
//   3. pnpm -r test               core+studio+cli 全量测试
//   4. node scripts/audit-npm.mjs npm 依赖审计（白名单语义）
//   5. node scripts/ensure-rust-bin.mjs       rust 活体腿二进制新鲜度闸
//     （643 号：活体步骤消费磁盘既有 debug 二进制而 cargo test 不编译 bin
//     目标——先 cargo build 增量核验/重建，拒绝以陈旧产物出具活体证据）
//   6. node scripts/node-fallback-smoke.mjs   双引擎一致性（mock+fixture+端点）
//   7. node scripts/engine-contract-diff.mjs  GET/写入/DELETE 契约差分
//   8. node scripts/export-epub-smoke.mjs     EPUB 导出结构冒烟
// Rust 门禁（clippy:gate / audit:rust / cargo test / duel / bench）不受本脚本
// 管理——Xcode 许可阻断期间亦不影响本脚本可用性。

import { spawnSync } from "node:child_process";

const args = process.argv.slice(2);
const fast = args.includes("--fast");
const repoRoot = process.cwd();

const steps = [
  // 587 号：build 前置——studio/cli 的 typecheck 消费 core **dist**（package
  // main 指向发布产物），陈旧 dist 会产生「源码已对但类型红」的瞬态假红
  // （572/581 两次实测坑）。产物链先行后，typecheck/test/活体套件全部消费
  // 新鲜 dist。--fast 仍跳过 build（快速信号折衷，dist 陈旧风险备案）。
  ["build", ["pnpm", ["-r", "build"]]],
  ["typecheck", ["pnpm", ["-r", "typecheck"]]],
  ["test", ["pnpm", ["-r", "test"]]],
  ["audit:npm", ["node", ["scripts/audit-npm.mjs"]]],
  ...(fast
    ? []
    : [
        // 643 号：活体腿二进制新鲜度闸（详见 ensure-rust-bin.mjs 头注释）。
        ["rust-bin", ["node", ["scripts/ensure-rust-bin.mjs"]]],
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
    // 643 号：rust-bin 的重建/降级信息属于活体证据面（增量重建 or cargo 不可用
    // 降级），成功也要可见；其余步骤成功保持静默。
    if (name === "rust-bin" && res.stdout?.trim()) console.log(res.stdout.trimEnd().replace(/^/gm, "    "));
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
