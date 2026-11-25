#!/usr/bin/env node
// Rust 全目标测试门禁（561 号）。
//
// 对 engine-rs 与 src-tauri 两把 crate 各跑一次 `cargo test --all-targets`
// 并从落盘日志解析每个目标的 test result 汇总——任一 FAILED 即非零退出。
//
// 为什么需要它（560 号教训第二例）：`cargo test --lib` 只覆盖 lib 内嵌
// #[cfg(test)] 单测，tests/ 目录的独立 test 目标（e2e/golden/duel 全族）
// 永远在覆盖外——556 号的 research85 回归在 --lib 门禁全绿下潜伏了两轮，
// 直到 clippy --all-targets 触碰 test 编译才暴露（569 号「cargo check 不
// 覆盖 test 目标」同类教训）。本门禁把「跑过所有目标」从人肉纪律变成
// 一条命令。
//
//   node scripts/cargo-test-gate.mjs             # 两把 crate 全跑
//   node scripts/cargo-test-gate.mjs engine-rs   # 只跑指定 crate 目录
//   node scripts/cargo-test-gate.mjs --self-check-only   # 只跑必红自检（duel 台账解析器 + 日志清理器）
//
// 前置与环境：
// - 第一步固定跑 audit:rust（RustSec 供应链审计，570 号接入）——需 cargo-audit
//   （`cargo install cargo-audit`），缺失即 exit 3 不静默。
// - engine-rs 侧必须带 INKOS_DUEL=1——strangler_duel 未设 env 时早退也计
//   pass（170/172 号假绿事故；--all-targets 会把它一并跑掉，故 env 在
//   本脚本内固定注入，不依赖调用方记得）。
// - duel 真跑地板（1666 号，见下方 DUEL_EXECUTED_FLOOR 段）：env 层早退之外，
//   守卫层早退同样计 pass——另以台账做正向确认。
// - 不含 doc-tests（口径与 `cargo test --lib` 计数惯例一致；538 号
//   576vs578 口径差备案）。
// - 已知代价：test 目标编译缓存冷时首轮耗时数分钟——门禁本义，与
//   clippy:gate 同纪律，禁止为提速注 skip/过滤洗门禁。

import { execFileSync, spawnSync } from "node:child_process";
import {
  existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, utimesSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");

const targets = process.argv.slice(2).filter((a) => !a.startsWith("-"));
const crates = targets.length ? targets : ["engine-rs", "src-tauri"];
const selfCheckOnly = process.argv.includes("--self-check-only");

// ── 1666 号：duel 真跑地板（解析型门禁正向确认）──
//
// INKOS_DUEL=1 只堵了 env 层早退（170/172 号）。1663 号把 duel_require_fork_
// contract! 的契约源判据迁到 /api/v1/health（Rust 超集独有面——TS 腿没有这条
// 路由），守卫对 TS 腿恒判「上游 2.0」→ 受守卫的对跑恒 `return` 早退，而
// libtest 把早退计成 ok：按 `test result:` 汇总的「N passed」里混着从未执行
// 的对跑。1666 实测：libtest 报 13 passed，真跑只有 2 个。强制打开守卫另测：
// 13 个对跑 6 真绿 / 7 真红。
//
// 守卫的 eprintln「响亮记账」在门禁日志里同样看不见（cargo test 默认捕获
// 测试的 stdout 与 stderr），所以走台账文件：本脚本给 engine-rs 注入
// `INKOS_DUEL_LEDGER=<fresh file>`，strangler_duel.rs 里
//   - `duel_enabled()`（每个对跑的入口）追加 `start <test>`
//   - 守卫早退追加 `exempt <test>`
// 本节据此正向确认：
//   start 数 = 该二进制测试总数（通道存活的握手——台账空/缺 ≠ 0 豁免）
//   真跑 = start − exempt ≥ DUEL_EXECUTED_FLOOR，且 exempt ≤ DUEL_EXEMPT_CEILING
// 二进制缺失、总数定位不到、对应错位一律红。复活对跑时把地板上调、上限
// 下调——两个数只许朝「少豁免」方向动。
//
// 水位沿革：首版地板 2 / 上限 11（当时真跑 2 / 豁免 11）。对跑夹具复活
// （双根布局播种、mock 回 finish_turn、声明差异表、守卫重定）后 13 个对跑
// 真跑 11 / 豁免 2——地板 11 / 上限 2。余下两个豁免是 cross_write_read 与
// full_grey：它们要的是「Rust 写 TS 读得到」，两腿各写各的树（books/ ↔
// works/），不是夹具能补的；存储模型定线后复活并把地板再上调。
// 1672 号：LLM 未配置对跑（原独立二进制 llm_config_duel 的两个测试——此前
// 不进台账，握手看不见它）并入对跑二进制；另加互动叙事数据根现状钉。16 个
// 对跑真跑 14 / 豁免 2——地板 11 → 14，上限不变。
// 1673 号：互动叙事存储模型第一批（Node 腿创建的世界产品引擎读写同一棵树），
// 新增真跑对跑一条。17 个对跑真跑 15 / 豁免 2——地板 14 → 15，上限不变。
// 1675 号：互动叙事存储模型第二批（产品引擎创建的世界 Node 腿读得到、按需迁移 +
// 文件图谱导入，此后两腿写同一棵树）新增真跑对跑一条；原现状钉改写为「两棵树
// 都在」的对账规则钉（真跑不变）。18 个对跑真跑 16 / 豁免 2——地板 15 → 16，
// 上限不变。
//
// 测试总数怎么取：本脚本把 cargo 的 stdout 与 stderr **分开采集后拼接**
// （stdout 在前、stderr 在后）。`Running <binary>` 走 stderr、`running N tests`
// 走 stdout——两者在拼接输出里不相邻，不能按「节」取。靠序号对应：cargo
// 顺序执行各二进制，stderr 第 k 条 `Running` ↔ stdout 第 k 条 `running N tests`
// ↔ 第 k 条 `test result:`，并用 `passed + failed + ignored === N` 交叉验证
// 对应没错位（1666 首版按节取数在真实日志上取到 0——合成夹具没复现拼接
// 形态，真实日志才暴露；夹具此后按真实拼接形态构造）。
const DUEL_EXECUTED_FLOOR = 16;
const DUEL_EXEMPT_CEILING = 2;
const DUEL_LEDGER_ENV = "INKOS_DUEL_LEDGER";
// 当前树形态的对跑总数 = 真跑地板 + 豁免上限。自检里「恰在两个水位上」的夹具
// 由此推导——上调地板或下调上限时自检自动跟走，不用再手抄一遍这些数。
const DUEL_CURRENT_TOTAL = DUEL_EXECUTED_FLOOR + DUEL_EXEMPT_CEILING;

const RUNNING_LINE_RE = /^running (\d+) tests?$/;
const RESULT_LINE_RE = /^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored;/;

/** 唯一解析路径：真门禁与必红自检共用——自检证明的就是这段代码还能看见红。 */
function parseDuelExecution(output, ledgerText) {
  const trimmed = output.split("\n").map((l) => l.trim());
  const binaryIndex = trimmed
    .map((l) => /^\s*Running (\S+)/.exec(l))
    .filter(Boolean)
    .findIndex((m) => /(?:^|\/)tests\/strangler_duel\.rs$/.test(m[1]));
  const runningLines = trimmed.map((l) => RUNNING_LINE_RE.exec(l)).filter(Boolean);
  const resultLines = trimmed.map((l) => RESULT_LINE_RE.exec(l)).filter(Boolean);
  const running = binaryIndex >= 0 ? runningLines[binaryIndex] : undefined;
  const result = binaryIndex >= 0 ? resultLines[binaryIndex] : undefined;
  const total = running ? Number(running[1]) : 0;
  const ledgerLines = String(ledgerText ?? "").split("\n").map((l) => l.trim()).filter(Boolean);
  const events = ledgerLines
    .map((l) => /^(start|exempt)\s+(\S+)$/.exec(l))
    .filter(Boolean)
    .map((m) => ({ kind: m[1], name: m[2] }));
  const started = events.filter((e) => e.kind === "start").map((e) => e.name);
  const exempted = events.filter((e) => e.kind === "exempt").map((e) => e.name);
  return {
    found: binaryIndex >= 0,
    totalLocated: Boolean(running),
    // 序号对应没错位的交叉验证：result 行三类计数之和 = running 行的总数。
    countsConsistent: Boolean(running && result)
      && Number(result[2]) + Number(result[3]) + Number(result[4]) === total,
    total,
    started,
    exempted,
    malformed: ledgerLines.length - events.length,
    executed: started.length - exempted.length,
  };
}

function duelVerdict(parsed) {
  if (!parsed.found) {
    return { ok: false, reason: "日志无 strangler_duel 二进制（`Running …strangler_duel.rs` 行缺失——被过滤/改名/未编译？）" };
  }
  if (!parsed.totalLocated) {
    return { ok: false, reason: "按序号对应取不到 strangler_duel 的 `running N tests` 行——无法确认总数" };
  }
  if (!parsed.countsConsistent) {
    return { ok: false, reason: "strangler_duel 的 running 行与 test result 行计数对不上（passed+failed+ignored ≠ N）——二进制序号对应错位" };
  }
  if (parsed.malformed > 0) {
    return { ok: false, reason: `duel 台账有 ${parsed.malformed} 行无法解析——台账格式漂移` };
  }
  if (parsed.started.length !== parsed.total) {
    return { ok: false, reason: `duel 台账 start ${parsed.started.length} ≠ 测试总数 ${parsed.total}——台账通道失效或不完整（空台账不等于零豁免）` };
  }
  const stray = parsed.exempted.filter((name) => !parsed.started.includes(name));
  if (stray.length > 0) {
    return { ok: false, reason: `duel 豁免行没有对应 start 行：${stray.join(", ")}——台账错位` };
  }
  if (parsed.executed < DUEL_EXECUTED_FLOOR) {
    return { ok: false, reason: `duel 真跑 ${parsed.executed} < 地板 ${DUEL_EXECUTED_FLOOR}（豁免只许减少；复活对跑后把地板上调）` };
  }
  if (parsed.exempted.length > DUEL_EXEMPT_CEILING) {
    return { ok: false, reason: `duel 豁免 ${parsed.exempted.length} > 上限 ${DUEL_EXEMPT_CEILING}（新增受守卫对跑须让它真跑，或显式上调并记账）` };
  }
  return { ok: true, reason: "" };
}

/**
 * 合成夹具，按**真实拼接形态**构造：stdout（各二进制的 running/test result，
 * 在前）+ "\n" + stderr（Compiling/Finished/Running 行，在后）。
 * duel 是第 3 个被执行的二进制（前有 lib 单测与 golden，后有 work 域）。
 */
function duelFixture({ tests = DUEL_CURRENT_TOTAL, started = [], exempt = [], withDuelBinary = true, withDuelStdout = true, extraRunninglessBinary = false, bogusRunningLine = false }) {
  // 全绿形态：passed = tests。
  const stdout = [
    "running 2200 tests",
    "test unit_a ... ok",
    "test result: ok. 2200 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 30.00s",
    "",
    "running 1 test",
    "test golden_run_log ... ok",
    // 某个测试自己的输出里恰有一行像 libtest 的 running 行（失败时捕获输出会回显）。
    ...(bogusRunningLine ? ["running 5 tests"] : []),
    "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
    "",
    ...(withDuelBinary && withDuelStdout ? [`running ${tests} tests`, "test strangler_prompt_packs_duel ... ok"] : []),
    ...(withDuelBinary && withDuelStdout
      ? [`test result: ok. ${tests} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.99s`, ""]
      : []),
    "running 3 tests",
    "test work_a ... ok",
    "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s",
  ].join("\n");
  const stderr = [
    "   Compiling inkos-engine v0.1.0 (/repo/engine-rs)",
    "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.84s",
    "     Running unittests src/lib.rs (target/debug/build/inkos-engine/h/out/inkos_engine-h)",
    "     Running tests/golden_run_log_diff.rs (target/debug/build/inkos-engine/h/out/golden-h)",
    ...(extraRunninglessBinary ? ["     Running tests/phantom.rs (target/debug/build/inkos-engine/h/out/phantom-h)"] : []),
    ...(withDuelBinary ? ["     Running tests/strangler_duel.rs (target/debug/build/inkos-engine/h/out/strangler_duel-h)"] : []),
    "     Running tests/work_domain_m1b.rs (target/debug/build/inkos-engine/h/out/work-h)",
  ].join("\n");
  const ledger = [...started.map((name) => `start ${name}`), ...exempt.map((name) => `exempt ${name}`)].join("\n");
  return { output: `${stdout}\n${stderr}`, ledgerText: ledger ? `${ledger}\n` : "" };
}

const names = (prefix, count) => Array.from({ length: count }, (_, i) => `${prefix}_${i + 1}`);

/** 必红自检（847 号纪律）：喂已知红的输入走同一解析路径，必须判红。 */
function duelSelfCheck() {
  const all = names("duel", DUEL_CURRENT_TOTAL);
  const first = (count) => all.slice(0, count);
  // 「豁免突破上限」要多到真跑达标、只输在豁免数上。
  const overTotal = DUEL_CURRENT_TOTAL + 5;
  const overExempt = DUEL_EXEMPT_CEILING + 1;
  const cases = [
    { label: `当前树形态（${DUEL_CURRENT_TOTAL} 测 / ${DUEL_EXEMPT_CEILING} 豁免 → 真跑 ${DUEL_EXECUTED_FLOOR}，恰在地板与上限上）`, wantOk: true,
      fixture: duelFixture({ started: all, exempt: first(DUEL_EXEMPT_CEILING) }),
      verify: (p) => p.total === DUEL_CURRENT_TOTAL && p.executed === DUEL_EXECUTED_FLOOR && p.exempted.length === DUEL_EXEMPT_CEILING && p.exempted[DUEL_EXEMPT_CEILING - 1] === `duel_${DUEL_EXEMPT_CEILING}` },
    { label: `复活后零豁免（${DUEL_CURRENT_TOTAL} 测 / 0 豁免 → 真跑 ${DUEL_CURRENT_TOTAL}）`, wantOk: true,
      fixture: duelFixture({ started: all, exempt: [] }),
      verify: (p) => p.executed === DUEL_CURRENT_TOTAL && p.exempted.length === 0 },
    { label: `真跑跌破地板（${DUEL_CURRENT_TOTAL} 测 / ${overExempt} 豁免 → 真跑 ${DUEL_EXECUTED_FLOOR - 1}，差 1）`, wantOk: false, needle: "地板",
      fixture: duelFixture({ started: all, exempt: first(overExempt) }) },
    { label: `豁免突破上限（${overTotal} 测 / ${overExempt} 豁免 → 真跑 ${overTotal - overExempt} 达标但豁免 ${overExempt} > ${DUEL_EXEMPT_CEILING}）`, wantOk: false, needle: "上限",
      fixture: duelFixture({ tests: overTotal, started: names("duel", overTotal), exempt: names("duel", overExempt) }) },
    { label: "台账为空（通道失效≠零豁免）", wantOk: false, needle: "台账通道失效",
      fixture: { ...duelFixture({ started: all }), ledgerText: "" } },
    { label: `台账不完整（${DUEL_CURRENT_TOTAL} 测只握了 ${DUEL_CURRENT_TOTAL - 1} 次 start）`, wantOk: false, needle: "台账通道失效",
      fixture: duelFixture({ started: first(DUEL_CURRENT_TOTAL - 1), exempt: first(6) }) },
    { label: "台账行格式漂移", wantOk: false, needle: "格式漂移",
      fixture: { ...duelFixture({ started: all }), ledgerText: `${all.map((n) => `start ${n}`).join("\n")}\nexempt-duel_1\n` } },
    { label: "豁免行无对应 start（台账错位）", wantOk: false, needle: "没有对应 start",
      fixture: duelFixture({ started: all, exempt: ["ghost_duel"] }) },
    { label: "缺 duel 二进制", wantOk: false, needle: "二进制",
      fixture: duelFixture({ started: all, exempt: [], withDuelBinary: false }) },
    { label: "duel 二进制没有 stdout（序号对应滑到下一个 → 握手拦住）", wantOk: false, needle: "台账通道失效",
      fixture: duelFixture({ started: all, exempt: [], withDuelStdout: false }) },
    { label: "多出一个没有 stdout 的二进制（序号后移 → 握手拦住）", wantOk: false, needle: "台账通道失效",
      fixture: duelFixture({ started: all, exempt: [], extraRunninglessBinary: true }) },
    { label: "stdout 混入伪 running 行（running 与 result 对不上 → 交叉验证拦住）", wantOk: false, needle: "计数对不上",
      fixture: duelFixture({ started: all, exempt: [], bogusRunningLine: true }) },
  ];
  let bad = 0;
  for (const testCase of cases) {
    const parsed = parseDuelExecution(testCase.fixture.output, testCase.fixture.ledgerText);
    const verdict = duelVerdict(parsed);
    const shapeOk = testCase.verify ? testCase.verify(parsed) : true;
    const verdictOk = verdict.ok === testCase.wantOk && (testCase.wantOk || verdict.reason.includes(testCase.needle));
    if (verdictOk && shapeOk) {
      console.log(`✓ 自检：${testCase.label}——${verdict.ok ? "放行" : "判红"}`);
    } else {
      bad += 1;
      console.error(`✗ 自检失败：${testCase.label}——期望 ${testCase.wantOk ? "放行" : "判红"}，实得 ${JSON.stringify(verdict)}；parsed=${JSON.stringify(parsed)}`);
    }
  }
  return bad === 0;
}

// ── 1669 号：门禁日志目录保留期 ──
//
// 每跑一次在系统 tmp 留一个 `cargo-test-gate-<随机>` 目录（output.log +
// duel-ledger.log），脚本从不清理——累计 200 个。只保留最近 LOG_DIR_KEEP 份
// （按 mtime 新→旧），其余删除。清理是附带品不是判据：删除失败只告警，不影响
// 门禁结论。LOG_DIR_MIN_AGE_MS 内改动过的目录一律不动——另一会话可能有门禁
// 正在跑，其日志目录正当在用（在用保护优先于保留份数）。
const LOG_DIR_PREFIX = "cargo-test-gate-";
const LOG_DIR_KEEP = 10;
const LOG_DIR_MIN_AGE_MS = 5 * 60_000;

/** 唯一清理路径：真清理与自检共用——自检证明的就是这段代码真会删、且只删该删的。 */
function pruneGateLogs(tmpRoot, { keep, now, minAgeMs }) {
  let entries;
  try {
    entries = readdirSync(tmpRoot, { withFileTypes: true });
  } catch {
    return { removed: 0, kept: 0, errors: ["tmpdir 不可读"] };
  }
  const dirs = [];
  const errors = [];
  for (const entry of entries) {
    if (!entry.isDirectory() || !entry.name.startsWith(LOG_DIR_PREFIX)) continue;
    try {
      dirs.push({ path: join(tmpRoot, entry.name), mtimeMs: statSync(join(tmpRoot, entry.name)).mtimeMs });
    } catch (error) {
      errors.push(`${entry.name}: ${error.message}`);
    }
  }
  dirs.sort((a, b) => b.mtimeMs - a.mtimeMs);
  let removed = 0;
  let kept = 0;
  dirs.forEach((dir, index) => {
    if (index < keep || now - dir.mtimeMs < minAgeMs) {
      kept += 1;
      return;
    }
    try {
      rmSync(dir.path, { recursive: true, force: true });
      removed += 1;
    } catch (error) {
      errors.push(`${dir.path}: ${error.message}`);
    }
  });
  return { removed, kept, errors };
}

/** 清理器必红自检：旧目录真删、按新旧保留、无关目录与在用目录不动。 */
function pruneSelfCheck() {
  /** 造 n 个日志目录；ageMs 为距 now 的时长（0 = 刚建，在用；旧目录逐个再旧一分钟）。 */
  const makeDirs = (root, prefix, count, ageMs) => Array.from({ length: count }, (_, index) => {
    const path = join(root, `${LOG_DIR_PREFIX}${prefix}${index}`);
    mkdirSync(path);
    const at = new Date(Date.now() - ageMs - (ageMs > 0 ? index * 60_000 : 0));
    utimesSync(path, at, at);
    return basename(path);
  });
  const scenario = (label, run) => {
    const root = mkdtempSync(join(tmpdir(), "cargo-test-gate-selfcheck-"));
    try {
      const { wantRemoved, wantRemaining } = run(root);
      const result = pruneGateLogs(root, { keep: 10, now: Date.now(), minAgeMs: LOG_DIR_MIN_AGE_MS });
      const remaining = readdirSync(root).sort();
      const ok = result.removed === wantRemoved && JSON.stringify(remaining) === JSON.stringify(wantRemaining.sort());
      return { label, ok, detail: `removed=${result.removed}/${wantRemoved} remaining=${remaining.length}/${wantRemaining.length}${result.errors.length ? ` errors=${result.errors.join(";")}` : ""}` };
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  };

  const stale = 3_600_000;
  const results = [
    scenario("基本保留", (root) => {
      // 15 个一小时前的目录（逐个更旧）+ 1 个在用 + 2 个无关目录。
      const old = makeDirs(root, "old", 15, stale);
      const fresh = makeDirs(root, "fresh", 1, 0);
      const unrelated = ["cargo-test-gatex", "other-prefix"];
      for (const name of unrelated) mkdirSync(join(root, name));
      // 保留份数含在用的那 1 个：fresh + 最新的 9 个旧目录留下，最旧的 6 个被删。
      return { wantRemoved: 6, wantRemaining: [...old.slice(0, 9), ...fresh, ...unrelated] };
    }),
    scenario("在用保护优先于保留份数", (root) => {
      // 12 个在用目录（两套门禁并行）+ 15 个旧目录：在用的全留，旧的全删。
      const busy = makeDirs(root, "busy", 12, 0);
      makeDirs(root, "old", 15, stale);
      return { wantRemoved: 15, wantRemaining: busy };
    }),
  ];
  return results;
}

{
  console.log("== 必红自检（duel 真跑地板解析器） ==");
  if (!duelSelfCheck()) {
    console.error("✗ duel 解析器自检失败——检测器失效，门禁结论不可信，拒绝放行");
    process.exit(1);
  }
  console.log("== 必红自检（门禁日志清理器） ==");
  let pruneBad = 0;
  for (const result of pruneSelfCheck()) {
    if (result.ok) {
      console.log(`✓ 自检：${result.label}——${result.detail}`);
    } else {
      pruneBad += 1;
      console.error(`✗ 自检失败：${result.label}——${result.detail}`);
    }
  }
  if (pruneBad > 0) {
    console.error("✗ 日志清理器自检失败——清理器行为与声明不符，拒绝放行");
    process.exit(1);
  }
}

if (selfCheckOnly) {
  console.log("✓ 自检通过（--self-check-only，未跑测试）");
  process.exit(0);
}

function cargoAvailable() {
  try {
    execFileSync("cargo", ["--version"], { stdio: "pipe" });
    return true;
  } catch {
    return false;
  }
}

if (!cargoAvailable()) {
  console.error("✗ 未找到 cargo。请先安装 rustup（门禁缺失不应静默绿灯）");
  process.exit(3);
}

// ── 前置哨兵：Rust 供应链漏洞审计（570 号）──
// audit:rust（RustSec，engine-rs+src-tauri 双 Cargo.lock）先于全目标测试：
// Cargo.lock 变更在测试跑完之前即被拦截。此前 audit:rust 是孤立脚本——
// 与 audit:npm（gate:ts 内强制）不对称，517 号 rustls 清零靠的是当时手动
// 跑过；接入后「改依赖即审计」成为机械纪律。cargo-audit 缺失按 audit-rust
// 自身纪律非零退出（exit 3），不静默绿灯。
{
  const audit = spawnSync(process.execPath, [join(scriptDir, "audit-rust.mjs")], {
    cwd: repoRoot,
    stdio: "inherit",
  });
  if (audit.status !== 0) {
    console.error(
      `✗ Rust 供应链审计未通过（exit ${audit.status ?? 1}）——先清漏洞再进测试门禁`,
    );
    process.exit(audit.status ?? 1);
  }
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
  // 统计落盘再解析——cargo 管道统计伪象两次实证（532/552 号），禁 |grep|awk。
  const logDir = mkdtempSync(join(tmpdir(), "cargo-test-gate-"));
  const logFile = join(logDir, "output.log");
  // 1666 号：duel 台账——每轮新建空文件（旧台账残留会把上轮豁免记进本轮）。
  const duelLedgerFile = join(logDir, "duel-ledger.log");
  writeFileSync(duelLedgerFile, "");
  const env = { ...process.env };
  if (crate === "engine-rs") {
    env.INKOS_DUEL = "1";
    env[DUEL_LEDGER_ENV] = duelLedgerFile;
  }
  const run = spawnSync("cargo", ["test", "--all-targets"], {
    cwd: dir,
    encoding: "utf8",
    env,
    maxBuffer: 64 * 1024 * 1024,
  });
  const output = `${run.stdout ?? ""}\n${run.stderr ?? ""}`;
  try {
    writeFileSync(logFile, output);
  } catch {
    // 落盘失败不阻断——解析走内存副本
  }

  const resultLines = output.split("\n").filter((l) => /^test result: /.test(l));
  const oks = resultLines.filter((l) => l.includes("ok.")).length;
  const failedLines = resultLines.filter((l) => l.includes("FAILED"));
  const passedTotal = resultLines.reduce(
    (sum, l) => sum + (Number(/^test result: ok\.\s+(\d+) passed/.exec(l)?.[1] ?? 0)),
    0,
  );

  if (run.status !== 0 || failedLines.length > 0 || oks === 0) {
    const lines = output.split("\n");
    const picked = lines.filter(
      (l) => /^test result: (ok\.\s+\d+ passed)?/.test(l) === false && /panicked|FAILED|^test .* \.\.\. FAILED|^error/.test(l),
    );
    console.error(picked.slice(0, 60).join("\n") || lines.slice(-40).join("\n"));
    console.error(`✗ ${crate}: 测试门禁拦截（${failedLines.length}/${resultLines.length} 目标 FAILED；详见 ${logFile}）`);
    failed = true;
    continue;
  }

  // 1666 号：engine-rs 的 duel 真跑地板——`N passed` 把守卫早退也计成通过，
  // 这里另行确认「真跑了多少 / 豁免了哪些」。
  if (crate === "engine-rs") {
    const ledgerText = existsSync(duelLedgerFile) ? readFileSync(duelLedgerFile, "utf8") : "";
    const parsed = parseDuelExecution(output, ledgerText);
    const verdict = duelVerdict(parsed);
    const roster = parsed.exempted.length ? `：${parsed.exempted.join(", ")}` : "";
    if (!verdict.ok) {
      console.error(`✗ ${crate}: ${verdict.reason}；台账 ${duelLedgerFile}；豁免清单${roster || "：(无)"}`);
      failed = true;
      continue;
    }
    console.log(`  duel 真跑 ${parsed.executed} / 豁免 ${parsed.exempted.length}（地板 ${DUEL_EXECUTED_FLOOR} / 上限 ${DUEL_EXEMPT_CEILING}）${roster}`);
  }

  console.log(`✓ ${crate}: ${oks} 个目标全绿，累计 ${passedTotal} passed（日志 ${logFile}）`);
}

// 1669 号：日志目录保留期——只留最近 LOG_DIR_KEEP 份（本轮的目录在用保护内）。
{
  const pruned = pruneGateLogs(tmpdir(), { keep: LOG_DIR_KEEP, now: Date.now(), minAgeMs: LOG_DIR_MIN_AGE_MS });
  if (pruned.removed > 0 || pruned.errors.length > 0) {
    const failures = pruned.errors.length ? `；失败 ${pruned.errors.length}：${pruned.errors.slice(0, 3).join("；")}` : "";
    console.log(`\n日志目录清理：删 ${pruned.removed} / 留 ${pruned.kept}（保留最近 ${LOG_DIR_KEEP} 份）${failures}`);
  }
}

process.exit(failed ? 1 : 0);
