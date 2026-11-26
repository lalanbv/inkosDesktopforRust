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
 * 变量名——同样只收路径）。1696 号补强：容忍 .route( 与路径字面量之间的整行
 * // 注释——sessions branch/branches 的「R36 会话树化（636 号）」注释行此前
 * `\s*` 不吃注释漏抓两条→被误判「TS 单腿双打」收进基线成静默假绿（1695 号
 * 实测：Rust=189 vs 真值 191）。1576 号教训：基线不许收解析器误报产物。 */
function parseRust(source) {
  const routes = new Set();
  const re = /\.route\(\s*(?:\/\/[^\n]*\n\s*)*"([^"]+)",/gu;
  for (const match of source.matchAll(re)) {
    routes.add(normalize(match[1]));
  }
  return routes;
}

/** 差集：新不对称 = 差集 − 基线（基线键逐字）。1696 号加 stale 哨：基线条目
 * 不再「恰好单腿存在」（两腿已配对或两侧均消失）=失效——基线只许收缩，修复
 * 不对称必须清条目（1695 号 sessions 两条假阳性条目即靠本哨强制出清）。 */
function diffAgainstBaseline(rustRoutes, tsRoutes, baseline) {
  const baselineRust = new Set(baseline.rustOnly ?? []);
  const baselineTs = new Set(baseline.tsOnly ?? []);
  const rustOnly = [...rustRoutes].filter((route) => !tsRoutes.has(route) && !baselineRust.has(route)).sort();
  const tsOnly = [...tsRoutes].filter((route) => !rustRoutes.has(route) && !baselineTs.has(route)).sort();
  const stale = [...baselineRust, ...baselineTs].filter(
    (route) =>
      !(
        (rustRoutes.has(route) && !tsRoutes.has(route)) ||
        (!rustRoutes.has(route) && tsRoutes.has(route))
      ),
  );
  return { rustOnly, tsOnly, stale };
}

// ── 自检夹具（847 号精神：主跑前自动执行走同一解析/比对路径——1696 号从
// 手动 --self-check 改入主跑前置；--self-check 保留为独立调用形态）──
function runSelfcheck() {
  const fixturesTsClean = 'app.get("/api/v1/ok", async (c) => {});\napp.get("/api/v1/books/:id/chapters/:chapters{.+}", async (c) => {});';
  const fixturesRust = '.route("/api/v1/ok", get(ok_handler))\n.route("/api/v1/books/:id/chapters/*chapters", get(wild_handler))';
  const fixturesTsGhost = 'app.get("/api/v1/ts-ghost", async (c) => {});';
  const fail = (msg) => {
    console.error(`[route-parity] 自检失败——本次结论作废（847 号语义）：${msg}`);
    process.exit(3);
  };
  // 绿：两腿一致 + 通配符归一后无差。
  const clean = diffAgainstBaseline(parseRust(fixturesRust), parseTs(fixturesTsClean), { rustOnly: [], tsOnly: [] });
  if (clean.rustOnly.length !== 0 || clean.tsOnly.length !== 0) fail(`绿例失败：一致输入报不对称 ${JSON.stringify(clean)}`);
  // 必红：TS 单腿假路由（不在 Rust、不在基线）必须检出。
  const red = diffAgainstBaseline(parseRust(fixturesRust), parseTs(fixturesTsClean + fixturesTsGhost), { rustOnly: [], tsOnly: [] });
  if (red.tsOnly.length !== 1 || !red.tsOnly[0].includes("ts-ghost")) fail(`必红例失败：TS 单腿假路由未检出 ${JSON.stringify(red)}`);
  // 解析活体哨：夹具解析产出空集 = 解析路径死链（结论作废）。
  if (parseTs(fixturesTsClean).size === 0 || parseRust(fixturesRust).size === 0) fail("解析死链：源解析产出空集");
  // 注释容忍例（1696 号，回归哨）：带整行注释的 .route( 必须抓到——漏抓形态
  // 即 1695 号 sessions 两条假阳性进基线的事故。
  const commented = parseRust('.route(\n            // R36 会话树化（636 号）：分支 head 指针移动（append-only）\n            "/api/v1/with-comment",\n            post(handler)\n        )');
  if (commented.size !== 1 || !commented.has("/api/v1/with-comment")) fail(`注释容忍例失败：${JSON.stringify([...commented])}`);
  // 吞尾例（头注释自 1695 起声称的夹具，1696 号补实现）：非 route 普通文本
  // 不得误产出路径；其后真 route 不得被吞。
  const tail = parseRust('fn helper(x: &str) -> bool {\n    x.contains(".route(");\n}\n\n.route("/api/v1/real-after-helper", get(real_handler)),');
  if (tail.size !== 1 || !tail.has("/api/v1/real-after-helper")) fail(`吞尾例失败：${JSON.stringify([...tail])}`);
  // stale 例（1696 号）：基线条目两腿都有（已配对）必须报失效——基线只许收缩。
  const staled = diffAgainstBaseline(parseRust('.route("/api/v1/paired", get(x))'), parseTs('app.get("/api/v1/paired", (c) => {});'), { rustOnly: ["/api/v1/paired"], tsOnly: [] });
  if (staled.stale.length !== 1 || staled.rustOnly.length !== 0) fail(`stale 例失败：${JSON.stringify(staled)}`);
  console.log("[route-parity] 自检七夹具全过（绿/必红/解析活体/注释容忍/吞尾/stale/通配归一）。");
}

if (selfCheck) {
  runSelfcheck();
  process.exit(0);
}

// ── 主跑 ──
if (!existsSync(tsServerPath) || !existsSync(rustModPath)) {
  console.error("[route-parity] 源文件缺失——解析死链（作废语义）");
  process.exit(3);
}
const tsSource = readFileSync(tsServerPath, "utf8");
const rustSource = readFileSync(rustModPath, "utf8");
runSelfcheck();
// 1696 号：TS 别形注册哨——解析覆盖面假设（1693 实测 .on/.route/反引号零
// 别形）不许静默失效，出现即 exit 3 作废。
const TS_ALIAS_RE = /\bapp\.(?:route|on)\(/;
if (TS_ALIAS_RE.test(tsSource)) {
  console.error("[route-parity] TS 腿出现解析器覆盖外的注册形态（app.route(/app.on()——解析失效，结论作废（exit 3）");
  process.exit(3);
}
const tsRoutes = parseTs(tsSource);
const rustRoutes = parseRust(rustSource);
if (tsRoutes.size === 0 || rustRoutes.size === 0) {
  console.error("[route-parity] 解析产出空集（TS=" + tsRoutes.size + " Rust=" + rustRoutes.size + "）——源形态变化，结论作废");
  process.exit(3);
}

const baseline = existsSync(baselinePath)
  ? JSON.parse(readFileSync(baselinePath, "utf8"))
  : { _comment: ["跨腿路由 parity 基线：1693 号全仓扫定性（9 惰性路由+2 单腿设计有消费面）；新不对称必须显式加表并定性。"], rustOnly: [], tsOnly: [] };

const { rustOnly, tsOnly, stale } = diffAgainstBaseline(rustRoutes, tsRoutes, baseline);

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
let bad = false;
for (const route of rustOnly) {
  bad = true;
  console.log(`  NEW RUST-ONLY ${route}`);
}
for (const route of tsOnly) {
  bad = true;
  console.log(`  NEW TS-ONLY ${route}`);
}
for (const route of stale) {
  bad = true;
  console.log(`  STALE BASELINE ${route} — 条目已失效（两腿已配对或两侧均消失），请从基线删除`);
}
if (bad) {
  console.error("[route-parity] 新不对称/stale 基线——两腿路由须成对（或定性后加基线表；失效条目必须清除）");
  process.exit(1);
}
console.log("[route-parity] 无新不对称（基线外的两腿路由全成对，基线无失效条目）。");
process.exit(0);
