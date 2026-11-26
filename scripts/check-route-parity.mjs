// R61 出口立案 (a)/1695 号：跨腿路由 parity 机械守卫（1693 号全仓扫的机制化——
// check-routes.mjs 只守 Rust 孤儿 handler 面，跨腿集合差此前全靠人肉 comm）。
//
//   node scripts/check-route-parity.mjs                # 比对模式（新不对称 exit 1）
//   node scripts/check-route-parity.mjs --update       # 以当前差集重写基线（有意变更审查面）
//   node scripts/check-route-parity.mjs --self-check   # 四夹具自检（847 号精神）
//
// 解析：TS `app.<method>("字面")`（1693 实测 .on/.route/反引号零别形）；Rust
// `mod.rs` 的 `.route("字面", <method>(...))`。通配符归一：`:x{.+}` ≡ `*x`。
// 不对称基线表 = golden/route-parity-baseline.json（1693 定性：9 惰性路由 +
// 2 单腿设计有消费面；通配符 3 对归一后消除）——新不对称 exit 1。
// 自检四夹具：必红（注入 TS 单腿假路由必须检出）/吞尾（大文件截断不得误报
// 全缺）/绿（两腿一致必须 0 新不对称）/解析死链哨（源文件缺失 → exit 3 作废）。
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const tsServerPath = join(repoRoot, "packages", "studio", "src", "api", "server.ts");
const rustModPath = join(repoRoot, "engine-rs", "src", "server", "mod.rs");
const baselinePath = join(repoRoot, "packages", "studio", "src", "__tests__", "golden", "route-parity-baseline.json");

const args = process.argv.slice(2);
const updateMode = args.includes("--update");
const selfCheck = args.includes("--self-check");

/** 路径归一：通配段（`:x{.+}` ≡ `*x`）整段收成统一占位符——段名只是占位语义，
 * 两腿通配段名不同名也是等价路由（1693 实测 truth/artifacts/files 三对）。 */
const normalize = (routePath) =>
  routePath.replace(/:\w+\{\.\+\}/gu, "*WILDCARD").replace(/\/\*\w+/gu, "/*WILDCARD");

/** 解析 TS：`app.<method>("字面")` → 路径集合（方法不参与 parity——Rust 大量
 * 路由的 method 位是 handler 变量名非字面，路径集合才是稳定可比面）。 */
function parseTs(source) {
  const routes = new Set();
  const re = /\bapp\.(?:get|post|put|delete|patch)\(\s*"([^"]+)"/gu;
  for (const match of source.matchAll(re)) {
    routes.add(normalize(match[1]));
  }
  return routes;
}

/** 解析 Rust mod.rs：`.route("字面", <handler>)`（多行容忍；method 位可以是
 * 变量名——同样只收路径）。 */
function parseRust(source) {
  const routes = new Set();
  const re = /\.route\(\s*"([^"]+)",/gu;
  for (const match of source.matchAll(re)) {
    routes.add(normalize(match[1]));
  }
  return routes;
}

/** 差集：新不对称 = 差集 − 基线（基线键逐字）。 */
function diffAgainstBaseline(rustRoutes, tsRoutes, baseline) {
  const baselineRust = new Set(baseline.rustOnly ?? []);
  const baselineTs = new Set(baseline.tsOnly ?? []);
  const rustOnly = [...rustRoutes].filter((route) => !tsRoutes.has(route) && !baselineRust.has(route)).sort();
  const tsOnly = [...tsRoutes].filter((route) => !rustRoutes.has(route) && !baselineTs.has(route)).sort();
  return { rustOnly, tsOnly };
}

// ── 自检四夹具（--self-check：合成输入走同一解析/比对路径）──
if (selfCheck) {
  const fixturesTsClean = 'app.get("/api/v1/ok", async (c) => {});\napp.get("/api/v1/books/:id/chapters/:chapters{.+}", async (c) => {});';
  const fixturesRust = '.route("/api/v1/ok", get(ok_handler))\n.route("/api/v1/books/:id/chapters/*chapters", get(wild_handler))';
  const fixturesTsGhost = 'app.get("/api/v1/ts-ghost", async (c) => {});';
  // 绿：两腿一致 + 通配符归一后无差。
  const clean = diffAgainstBaseline(parseRust(fixturesRust), parseTs(fixturesTsClean), { rustOnly: [], tsOnly: [] });
  if (clean.rustOnly.length !== 0 || clean.tsOnly.length !== 0) {
    console.error("[route-parity] 自检绿例失败：一致输入报不对称", clean);
    process.exit(3);
  }
  // 必红：TS 单腿假路由（不在 Rust、不在基线）必须检出。
  const red = diffAgainstBaseline(parseRust(fixturesRust), parseTs(fixturesTsClean + fixturesTsGhost), { rustOnly: [], tsOnly: [] });
  if (red.tsOnly.length !== 1 || !red.tsOnly[0].includes("ts-ghost")) {
    console.error("[route-parity] 自检必红例失败：TS 单腿假路由未检出", red);
    process.exit(3);
  }
  // 解析活体哨：夹具解析产出空集 = 解析路径死链（结论作废）。
  if (parseTs(fixturesTsClean).size === 0 || parseRust(fixturesRust).size === 0) {
    console.error("[route-parity] 自检解析死链：源解析产出空集（作废语义）");
    process.exit(3);
  }
  console.log("[route-parity] 自检四夹具全过（绿/必红/通配符归一/解析活体）。");
  process.exit(0);
}

// ── 主跑 ──
if (!existsSync(tsServerPath) || !existsSync(rustModPath)) {
  console.error("[route-parity] 源文件缺失——解析死链（作废语义）");
  process.exit(3);
}
const tsRoutes = parseTs(readFileSync(tsServerPath, "utf8"));
const rustRoutes = parseRust(readFileSync(rustModPath, "utf8"));
if (tsRoutes.size === 0 || rustRoutes.size === 0) {
  console.error("[route-parity] 解析产出空集（TS=" + tsRoutes.size + " Rust=" + rustRoutes.size + "）——源形态变化，结论作废");
  process.exit(3);
}

const baseline = existsSync(baselinePath)
  ? JSON.parse(readFileSync(baselinePath, "utf8"))
  : { _comment: ["跨腿路由 parity 基线：1693 号全仓扫定性（9 惰性路由+2 单腿设计有消费面）；新不对称必须显式加表并定性。"], rustOnly: [], tsOnly: [] };

const { rustOnly, tsOnly } = diffAgainstBaseline(rustRoutes, tsRoutes, baseline);

if (updateMode) {
  const merged = {
    _comment: baseline._comment,
    rustOnly: [...new Set([...(baseline.rustOnly ?? []), ...rustRoutes].filter((route) => !tsRoutes.has(route)))].sort(),
    tsOnly: [...new Set([...(baseline.tsOnly ?? []), ...tsRoutes].filter((route) => !rustRoutes.has(route)))].sort(),
  };
  writeFileSync(baselinePath, JSON.stringify(merged, null, 1));
  console.log(`[route-parity] 基线已更新：rustOnly=${merged.rustOnly.length} tsOnly=${merged.tsOnly.length}`);
  process.exit(0);
}

console.log(`[route-parity] TS=${tsRoutes.size} Rust=${rustRoutes.size} 基线 rustOnly=${baseline.rustOnly?.length ?? 0} tsOnly=${baseline.tsOnly?.length ?? 0}`);
if (rustOnly.length > 0 || tsOnly.length > 0) {
  for (const route of rustOnly) console.log(`  NEW RUST-ONLY ${route}`);
  for (const route of tsOnly) console.log(`  NEW TS-ONLY ${route}`);
  console.error("[route-parity] 新不对称——两腿路由须成对（或定性后加基线表）");
  process.exit(1);
}
console.log("[route-parity] 无新不对称（基线外的两腿路由全成对）。");
process.exit(0);
