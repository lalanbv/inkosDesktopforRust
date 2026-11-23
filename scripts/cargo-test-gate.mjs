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
//   node scripts/cargo-test-gate.mjs --self-check-only   # 只跑 duel 台账解析器必红自检
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
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
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
// 测试总数怎么取：本脚本把 cargo 的 stdout 与 stderr **分开采集后拼接**
// （stdout 在前、stderr 在后）。`Running <binary>` 走 stderr、`running N tests`
// 走 stdout——两者在拼接输出里不相邻，不能按「节」取。靠序号对应：cargo
// 顺序执行各二进制，stderr 第 k 条 `Running` ↔ stdout 第 k 条 `running N tests`
// ↔ 第 k 条 `test result:`，并用 `passed + failed + ignored === N` 交叉验证
// 对应没错位（1666 首版按节取数在真实日志上取到 0——合成夹具没复现拼接
// 形态，真实日志才暴露；夹具此后按真实拼接形态构造）。
const DUEL_EXECUTED_FLOOR = 2;
const DUEL_EXEMPT_CEILING = 11;
const DUEL_LEDGER_ENV = "INKOS_DUEL_LEDGER";

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
function duelFixture({ tests = 13, started = [], exempt = [], withDuelBinary = true, withDuelStdout = true, extraRunninglessBinary = false, bogusRunningLine = false }) {
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
  const all = names("duel", 13);
  const first = (count) => all.slice(0, count);
  const cases = [
    { label: "当前树形态（13 测 / 11 豁免 → 真跑 2）", wantOk: true,
      fixture: duelFixture({ started: all, exempt: first(11) }),
      verify: (p) => p.total === 13 && p.executed === 2 && p.exempted.length === 11 && p.exempted[10] === "duel_11" },
    { label: "复活后零豁免（13 测 / 0 豁免 → 真跑 13）", wantOk: true,
      fixture: duelFixture({ started: all, exempt: [] }),
      verify: (p) => p.executed === 13 && p.exempted.length === 0 },
    { label: "真跑跌破地板（13 测 / 12 豁免 → 真跑 1）", wantOk: false, needle: "地板",
      fixture: duelFixture({ started: all, exempt: first(12) }) },
    { label: "豁免突破上限（20 测 / 12 豁免 → 真跑 8 但豁免 12 > 11）", wantOk: false, needle: "上限",
      fixture: duelFixture({ tests: 20, started: names("duel", 20), exempt: names("duel", 12) }) },
    { label: "台账为空（通道失效≠零豁免）", wantOk: false, needle: "台账通道失效",
      fixture: { ...duelFixture({ started: all }), ledgerText: "" } },
    { label: "台账不完整（13 测只握了 12 次 start）", wantOk: false, needle: "台账通道失效",
      fixture: duelFixture({ started: first(12), exempt: first(6) }) },
    { label: "台账行格式漂移", wantOk: false, needle: "格式漂移",
      fixture: { ...duelFixture({ started: all }), ledgerText: `${first(13).map((n) => `start ${n}`).join("\n")}\nexempt-duel_1\n` } },
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

{
  console.log("== 必红自检（duel 真跑地板解析器） ==");
  if (!duelSelfCheck()) {
    console.error("✗ duel 解析器自检失败——检测器失效，门禁结论不可信，拒绝放行");
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

process.exit(failed ? 1 : 0);
