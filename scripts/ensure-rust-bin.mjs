#!/usr/bin/env node
// Rust 活体腿二进制新鲜度闸（643 号，641 号教训的结构性清偿）：
//   node scripts/ensure-rust-bin.mjs
//
// 641 号实证：gate:ts 的活体步骤（smoke rust 腿 / 契约差分器 / EPUB 冒烟）消费
// 磁盘既有的 target/debug 二进制，而 testgate 的 cargo test 不编译 bin 目标——
// Rust 改动后若未显式重建，活体面测旧代码照样全绿（「先重建再判读」七次人肉
// 教训）。本闸 = 活体腿自带产物新鲜度保障：默认路径下先 `cargo build`（增量，
// 新鲜时亚秒空转），以 cargo 自身指纹为新鲜度 oracle（严格强于 mtime 探针）；
// build 失败即拒绝继续（宁红勿以陈旧产物出具活体证据）。
//
// 合约：
//   - INKOS_SMOKE_RUST_BIN 已设（517 形态：release 二进制等外部产物）→ 跳过
//     构建，仅核在，新鲜度由调用方自管。
//   - GATE_TS_SKIP_RUST_BIN=1 → 整闸跳过（cargo 链接受阻等环境豁免出口；
//     gate:ts 与三个活体套件同认）。
//   - cargo 不在 PATH → 告警放行（二进制缺失时各套件既有 skip/快速失败语义
//     兜底），保持「cargo 链接受阻不影响 TS 门禁可用性」既有合约。
//
// 三个活体套件（node-fallback-smoke / engine-contract-diff / export-epub-smoke）
// import 本模块，独立运行同样受保；本脚本亦可作为 gate:ts 步骤直接运行。

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const defaultRustBinary = (repoRoot) =>
  join(repoRoot, "engine-rs", "target", "debug", "inkos-engine-server");

/**
 * 保证活体腿消费的 Rust 二进制与 engine-rs 当前源码同源。
 * @returns {{status: "fresh"|"rebuilt"|"override"|"skipped"|"uncertified"|"missing"|"build-failed", rustBinary: string, detail?: string}}
 */
export function ensureRustBinFresh(repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..")) {
  const rustBinary = process.env.INKOS_SMOKE_RUST_BIN ?? defaultRustBinary(repoRoot);
  if (process.env.GATE_TS_SKIP_RUST_BIN === "1") {
    console.warn("[rust-bin] ⏭ 新鲜度闸跳过（GATE_TS_SKIP_RUST_BIN=1）");
    return { status: "skipped", rustBinary };
  }
  if (process.env.INKOS_SMOKE_RUST_BIN) {
    console.log(`[rust-bin] INKOS_SMOKE_RUST_BIN 已设——外部产物新鲜度由调用方自管（${rustBinary}）`);
    return { status: "override", rustBinary };
  }
  const probe = spawnSync("cargo", ["--version"], { encoding: "utf-8" });
  if (probe.error) {
    if (!existsSync(rustBinary)) {
      console.warn(`[rust-bin] cargo 不可用且二进制缺失（${rustBinary}）——活体套件将按缺失语义跳过/报错`);
      return { status: "missing", rustBinary };
    }
    console.warn(`[rust-bin] ⚠ cargo 不可用（${probe.error.code ?? probe.error}）——二进制新鲜度无法自证（${rustBinary}）`);
    return { status: "uncertified", rustBinary };
  }
  const build = spawnSync("cargo", ["build"], { cwd: join(repoRoot, "engine-rs"), encoding: "utf-8" });
  if (build.status !== 0) {
    const tail = `${build.stdout ?? ""}${build.stderr ?? ""}`.slice(-2000);
    console.error(`[rust-bin] ✗ cargo build 失败——拒绝以陈旧二进制出具活体证据\n${tail}`);
    return { status: "build-failed", rustBinary, detail: tail };
  }
  if (!existsSync(rustBinary)) {
    console.error(`[rust-bin] ✗ cargo build 成功但未产出 ${rustBinary}`);
    return { status: "build-failed", rustBinary, detail: "build 成功但二进制缺失" };
  }
  // cargo 实际动手时 stderr 会带 Compiling 行；纯 Finished = 指纹核验新鲜。
  const rebuilt = /\bCompiling\b/.test(build.stderr ?? "");
  console.log(`[rust-bin] ✓ 活体二进制与 engine-rs 源码同源（${rebuilt ? "增量重建" : "cargo 指纹核验新鲜"}：${rustBinary}）`);
  return { status: rebuilt ? "rebuilt" : "fresh", rustBinary };
}

/** 套件/门禁调用面：build 失败即终止进程（失败早退须发生在任何子进程 spawn 之前）。 */
export function ensureRustBinFreshOrExit(repoRoot) {
  const res = ensureRustBinFresh(repoRoot);
  if (res.status === "build-failed") process.exit(1);
  return res;
}

function main() {
  ensureRustBinFreshOrExit();
}

// 直接运行（gate:ts 步骤）vs 被套件 import——两者共用同一实现。
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main();
