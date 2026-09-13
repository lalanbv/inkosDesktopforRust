#!/usr/bin/env node
// 路由注册 vs 处理函数 机械对照（412/413 号模式沉淀）。
//
//   node scripts/check-routes.mjs
//
// 排查「ops/books 等路由模块已实现 axum handler（-> impl IntoResponse）
// 但 server/mod.rs 未挂路由」的孤儿端点（411 writing-stats-rows、412
// hybrid-search 两例同款）。输出差集供人工核实——差集里混有内部工具
// 函数（如 load_radar_history 被 handler 调用），需逐个确认调用方。
//
// 退出码：存在孤儿 → 1（CI/验收用），否则 0。

import { readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const modPath = join(repoRoot, "engine-rs", "src", "server", "mod.rs");
if (!existsSync(modPath)) {
  console.error("[check-routes] 找不到 engine-rs/src/server/mod.rs（请在仓库根运行）");
  process.exit(2);
}

const mod = readFileSync(modPath, "utf-8");
const registered = new Set(
  [...mod.matchAll(/(?:get|post|put|delete|patch)\(\s*([a-z_]+(?:::[a-z_0-9]+)+)/g)]
    .map((m) => m[1].split("::").pop()),
);

// 已人工核实的白名单（413 号）：非漏挂（内部工具函数误匹配 / 备用 API）。
const ALLOWLIST = new Set([
  "effective_router",   // books_routes 内部缓存辅助（正则误匹配）
  "get_writing_stats",  // 清理候选：tuple 序列化不符前端期望；411 rows 版为正确链路
  "save_radar_scan",    // 备用 API（组合 scan 入口的内部包装）
  "peek_create_status", // 备用 API（建书状态轮询，前端走 SSE/session）
]);

const modules = [
  "ops_routes", "books_routes", "books_state_routes",
  "book_create_routes", "write_next_route", "service_routes", "agent_production",
];
const orphans = [];
let total = 0;
for (const m of modules) {
  const path = join(repoRoot, "engine-rs", "src", "server", `${m}.rs`);
  if (!existsSync(path)) continue;
  const src = readFileSync(path, "utf-8");
  // handler 形态：pub async fn 名( ... ) -> impl IntoResponse（跨行非贪婪）。
  for (const match of src.matchAll(/pub\s+async\s+fn\s+([a-z_0-9]+)\s*\(.*?\)\s*->\s*impl\s+IntoResponse/gs)) {
    total += 1;
    const name = match[1];
    if (!registered.has(name) && !ALLOWLIST.has(name)) orphans.push({ module: m, name });
  }
}

console.log(`[check-routes] 注册 handler = ${registered.size}，handler 形态函数 = ${total}，孤儿 = ${orphans.length}`);
for (const { module, name } of orphans) {
  console.log(`  ⚠️ [${module}] ${name} — 实现存在但 mod.rs 未注册（或为内部工具函数，请人工核实调用方）`);
}
process.exit(orphans.length > 0 ? 1 : 0);
