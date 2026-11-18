#!/usr/bin/env node
// 路由注册 vs 处理函数 机械对照（412/413 号模式沉淀）。
//
//   node scripts/check-routes.mjs
//
// 排查「ops/books 等路由模块已实现 axum handler（-> impl IntoResponse）
// 但 server/mod.rs 未挂路由」的孤儿端点（411 writing-stats-rows、412
// hybrid-search 两例同款）。
//
// 退出码：自检失败 → 3（本次「孤儿=0」结论作废，847 号语义）；存在孤儿 → 1
// （CI/验收用）；否则 0。
//
// 1577 号机制化自检（847 号精神：解析型门禁须配必红自检走同一解析路径）：
// 每次主跑前对三夹具断言精确集合——必红例（未注册 handler 必须检出）/
// 吞尾例（1576 号事故形态回归哨：普通 pub async fn 不得误报、其后的真
// handler 不得被吞）/ 绿例（注册齐全必须 0 孤儿）。任何一例偏离 → exit 3。

import { readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

// ── 解析函数（主流程与自检共用同一路径——847 号「自检走同一解析路径」）──

// handler 形态：pub async fn 名( ... ) -> impl IntoResponse（跨行非贪婪）。
// 1576 号收紧：签名段禁 `;{}`——裸 `.*?` 会从「签名不含 impl IntoResponse
// 的普通 pub async fn」的括号跨行吞到下一个真 handler 的尾部（返回 bool
// 的 doctor_models_probe 被误匹配为 handler 形态，413 号 effective_router
// 白名单项同款根因）；真 handler 签名（参数类型列表）不含这三个字符。
const HANDLER_RE = /pub\s+async\s+fn\s+([a-z_0-9]+)\s*\([^;{}]*?\)\s*->\s*impl\s+IntoResponse/gs;
const REGISTER_RE = /(?:get|post|put|delete|patch)\(\s*([a-z_]+(?:::[a-z_0-9]+)+)/g;

/** mod.rs 文本 → 已注册 handler 名集合（取 `module::name` 尾段）。 */
export function parseRegistered(modText) {
  return new Set(
    [...modText.matchAll(REGISTER_RE)].map((m) => m[1].split("::").pop()),
  );
}

/** 模块源码文本 → handler 形态函数名列表。 */
export function scanHandlerNames(srcText) {
  return [...srcText.matchAll(HANDLER_RE)].map((m) => m[1]);
}

/** 注册面 vs handler 面 → 孤儿名列表（实现存在但未注册）。 */
export function diffOrphans(modText, moduleSources, allowlist = new Set()) {
  const registered = parseRegistered(modText);
  const orphans = [];
  let total = 0;
  for (const src of moduleSources) {
    for (const name of scanHandlerNames(src)) {
      total += 1;
      if (!registered.has(name) && !allowlist.has(name)) orphans.push(name);
    }
  }
  return { registered, total, orphans };
}

// ── 机制化自检（三夹具精确集合断言；每次主跑前执行）─────────────────

function runSelfcheck() {
  const HANDLER = (name) =>
    `pub async fn ${name}(State(rt): State<BooksRuntime>) -> impl IntoResponse {\n    todo!()\n}\n`;
  const PLAIN = (name) =>
    `pub async fn ${name}(base: &str, api_key: &str) -> bool {\n    true\n}\n`;
  const failures = [];

  // 红例：未注册真 handler 必须检出（防恒绿）。
  {
    const src = HANDLER("unregistered_orphan_handler");
    const { total, orphans } = diffOrphans("", [src]);
    if (total !== 1 || orphans.length !== 1 || orphans[0] !== "unregistered_orphan_handler") {
      failures.push(`必红例失效：total=${total} orphans=${JSON.stringify(orphans)}`);
    }
  }
  // 吞尾例（1576 号事故回归哨）：普通 fn 在前 + 真 handler 在后——旧正则
  // 会误报普通 fn 并吞掉真 handler 命中位。断言宽度=精确集合（854 号）。
  {
    const src = PLAIN("plain_helper_not_a_handler") + HANDLER("get_real_handler_below");
    const { total, orphans } = diffOrphans("", [src]);
    const exact =
      total === 1 && orphans.length === 1 && orphans[0] === "get_real_handler_below";
    if (!exact) {
      failures.push(
        `吞尾例失效（正则可能回潮裸 .*?）：total=${total} orphans=${JSON.stringify(orphans)}`,
      );
    }
  }
  // 绿例：注册齐全必须 0 孤儿（防「注册解析死掉→全孤儿」反向失明——退出码
  // 1 会响，但主跑前先确认解析面活着）。
  {
    const src = HANDLER("get_well_registered");
    const mod = "axum::routing::get(ops_routes::get_well_registered)";
    const { total, orphans } = diffOrphans(mod, [src]);
    if (total !== 1 || orphans.length !== 0) {
      failures.push(`绿例失效：total=${total} orphans=${JSON.stringify(orphans)}`);
    }
  }
  // 注册解析死链哨：modText 无任何注册行时绿例夹具必须报孤儿（解析正则
  // 若失效，绿例会假绿——同夹具反向断言）。
  {
    const src = HANDLER("get_well_registered");
    const { orphans } = diffOrphans("", [src]);
    if (orphans.length !== 1 || orphans[0] !== "get_well_registered") {
      failures.push(`注册解析哨失效：orphans=${JSON.stringify(orphans)}`);
    }
  }

  if (failures.length > 0) {
    console.error("[check-routes] 自检失败——本次孤儿=0 结论作废（847 号语义）：");
    for (const f of failures) console.error(`  ✗ ${f}`);
    process.exit(3);
  }
  console.log("[check-routes] 自检 4 夹具通过（必红/吞尾/绿/注册解析哨）");
}

// ── 主流程 ─────────────────────────────────────────────────────────

const modPath = join(repoRoot, "engine-rs", "src", "server", "mod.rs");
if (!existsSync(modPath)) {
  console.error("[check-routes] 找不到 engine-rs/src/server/mod.rs（请在仓库根运行）");
  process.exit(2);
}

runSelfcheck();

const mod = readFileSync(modPath, "utf-8");
const modules = [
  "ops_routes", "books_routes", "books_state_routes",
  "book_create_routes", "write_next_route", "service_routes", "agent_production",
  // 954 号：backup_routes 补登记（443 号移植时漏列 modules 清单——孤儿检测
  // 对备份域 handler 从未生效；当前 4 handler 全部注册在位，差集为空）。
  "backup_routes",
];
const sources = [];
for (const m of modules) {
  const path = join(repoRoot, "engine-rs", "src", "server", `${m}.rs`);
  if (existsSync(path)) sources.push(readFileSync(path, "utf-8"));
}

const { registered, total, orphans } = diffOrphans(mod, sources);

console.log(`[check-routes] 注册 handler = ${registered.size}，handler 形态函数 = ${total}，孤儿 = ${orphans.length}`);
for (const name of orphans) {
  console.log(`  ⚠️ ${name} — 实现存在但 mod.rs 未注册（或为内部工具函数，请人工核实调用方）`);
}
process.exit(orphans.length > 0 ? 1 : 0);
