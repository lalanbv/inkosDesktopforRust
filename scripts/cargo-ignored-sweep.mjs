#!/usr/bin/env node
// src-tauri 挂起用例（#[ignore]）本地实跑清偿（988 号）。
//
// M1a 教训：被 #[ignore] 的测试路径是零观测盲区（两枚潜伏缺陷全在被 ignore 的
// episode 测试上）。src-tauri 的挂起用例清一色「环境门控」——CI 无 keychain /
// pfctl / release 二进制跑不了，但本机 macOS 全部前置齐备。本脚本把「挂起面
// 定期实跑」从人肉拼命令变成一条命令，并带**清单地板卫兵**：实测集合必须与
// 登记清单精确相等——静默缩水到 0 用例（重命名/删除/过滤器写错）即非零退出，
// 防止清偿面烂成恒绿（847/848 纪律）。
//
//   pnpm cargo:ignored-sweep
//
// 覆盖面（988 号 lib+watcher 实跑清偿；989 号扩面桌面 e2e 三 scope）：
// - lib 单测 7 枚：pfctl bail（C5 回归）+ keychain 真实四枚（roundtrip/
//   delete-missing/stale-tolerant/concurrent）+ updater 双通道网络面（GitHub
//   release 探测 + node 二进制真实下载校验）。
// - tests/sync_watcher_io 3 枚：notify watcher 端到端（988 号已改探针自同步，
//   固定 sleep 注册竞态已清偿）。
// - tests/e2e_updater 1 枚：is_newer 版本比较（988 号勘误：旧 ignore 理由
//   「需网络」已过时，实为纯逻辑，0.00s 级）。
// - tests/supervisor_integration 1 枚：真实 inkos sidecar 拉起→SPA 可达→
//   kill_tree 端口释放（自动先跑 desktop-package-engine.sh 组装；需 :4567
//   空闲——桌面应用在跑会假红）。
// - tests/e2e_startup 2 枚（--release）：桌面应用启动冒烟×2——**会闪现应用
//   窗口各 ~5s**（超时击杀即预期）；release profile 陈旧时本 scope 触发增量
//   release 编译（首次数分钟）。
// - tests/e2e_secrets 9 枚（--test-threads=1）：真实 keychain 契约面（990 号
//   unique_service 修复后无 ACL 弹框；需桌面解锁会话——keychain 锁定/dark wake
//   时 securityd 调用会阻塞）。
//
// 豁免：无——src-tauri 全部 23 枚挂起用例已全量收编（988/989/990 三轮）。
//
// 纪律：**勿与其它 cargo test 并行跑**——同机双 cargo 实例会因 target 锁 +
// securityd/keychain 跨进程串行化造成分钟级停滞假象（988 号自身双跑实测）。
// 网络面两枚需外网可达；pfctl 枚需非 root macOS。全程约 60-80s（release
// 陈旧时另计编译）。
//
// **超时哨兵**（992 号，清偿 990 备案①已知边界）：每 scope 带按面定的预算
// （timeoutMs）。keychain 锁定（dark wake）/securityd 异常会让 e2e_secrets
// 阻塞无限期（898「无界等待」教训同族）——哨兵把无限挂死变成有限超时 + 红报
// + 孤儿清理指引。预算含编译余量：release scope 最宽（陈旧缓存时数分钟）。
// 已知边界：spawnSync 的 kill 只达 cargo 直接子进程，测试二进制可能成孤儿
// （903 族）——超时报文自带特征清理指引；下次 sweep 的 supervisor 端口预检
// 与 e2e_startup 独占启动也会暴露残留。

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");
const tauriDir = join(repoRoot, "src-tauri");

if (!existsSync(join(tauriDir, "Cargo.toml"))) {
  console.error("✗ src-tauri: 无 Cargo.toml（目录结构变了？先更新本脚本）");
  process.exit(3);
}

// 登记清单：实测集合必须与之精确相等。src-tauri 挂起用例增删时**有意**更新此处
// （清单漂移正是本卫兵要抓的信号）。
const INVENTORY = [
  {
    scope: "--lib",
    // 网络两枚（GitHub API + node 下载）+ keychain 四枚 + 冷编译余量
    timeoutMs: 300_000,
    tests: [
      "isolation::macos::tests::pf_guard_lock_bails_without_root",
      "secrets::store::tests::keyring_real_roundtrip_upsert_read_delete",
      "secrets::store::tests::keyring_real_delete_missing_is_noop",
      "secrets::store::tests::keyring_real_index_stale_entry_tolerated",
      "secrets::store::tests::keyring_real_concurrent_upsert_no_lost_keys",
      "updater::engine::tests::real_engine_channel_check",
      "engine::node::tests::real_bootstrap_downloads_and_verifies",
    ],
  },
  {
    scope: "--test sync_watcher_io",
    // 三枚 IO 测 ~6s + 冷编译余量
    timeoutMs: 180_000,
    tests: [
      "watcher_anti_loop_programmatic_write_does_not_writeback",
      "watcher_spa_write_writebacks_to_keychain",
      "watcher_shutdown_flag_exits_thread_cleanly",
    ],
  },
  {
    scope: "--test e2e_updater",
    // 纯逻辑一枚 0.00s 级（990 勘误），预算=编译余量
    timeoutMs: 120_000,
    tests: ["test_is_newer_version_check"],
  },
  {
    scope: "--test supervisor_integration",
    // 真实 sidecar 60s 探测上限 + 端口重试 5s + 组装余量
    timeoutMs: 240_000,
    tests: ["real_inkos_sidecar_serves_spa"],
    // 真实 sidecar 前置：src-tauri/engine 组装（幂等；TS dist 缺失时脚本内
    // 自行触发 pnpm build）。缺组装时用例自带「engine 未组装」panic。
    pre: [join(repoRoot, "scripts", "desktop-package-engine.sh")],
  },
  {
    scope: "--release --test e2e_startup",
    // 最宽预算：release profile 陈旧时触发数分钟级增量编译（989 号实测 10s 级
    // 是热缓存态；冷态另计）
    timeoutMs: 1_200_000,
    tests: ["test_startup_smoke", "test_startup_logs_version"],
  },
  {
    scope: "--test e2e_secrets",
    // 本哨兵主目标：9 枚 ~19s，但 keychain 锁定（dark wake）时 securityd
    // 阻塞无上限（990 备案①）——预算到点 SIGKILL，挂死变红报
    timeoutMs: 240_000,
    // --test-threads=1：keychain 操作串行（原文件头惯例），防 securityd 并发抖动
    extraTestArgs: ["--test-threads=1"],
    tests: [
      "test_keychain_upsert",
      "test_keychain_read_all",
      "test_keychain_delete",
      "test_keychain_read_empty",
      "test_keychain_overwrite",
      "test_secrets_full_cycle",
      "test_multiple_keys",
      "test_empty_value",
      "test_unicode_value",
    ],
  },
];

function cargoAvailable() {
  try {
    execFileSync("cargo", ["--version"], { stdio: "pipe" });
    return true;
  } catch {
    return false;
  }
}

if (!cargoAvailable()) {
  console.error("✗ 未找到 cargo。请先安装 rustup（清偿面缺失不应静默绿灯）");
  process.exit(3);
}

let failed = false;

for (const inv of INVENTORY) {
  if (inv.pre) {
    const pre = spawnSync("bash", inv.pre, { cwd: repoRoot, stdio: "inherit" });
    if (pre.status !== 0) {
      console.error(
        `✗ [${inv.scope}] 前置脚本失败（exit ${pre.status ?? 1}）：bash ${inv.pre.join(" ")}`,
      );
      failed = true;
      continue;
    }
  }
  console.log(`\n== cargo test ${inv.scope} -- --ignored ==`);
  // 统计落盘再解析——cargo 管道统计伪象两次实证（532/552 号），禁 |grep|awk。
  const logFile = join(mkdtempSync(join(tmpdir(), "cargo-ignored-sweep-")), "output.log");
  const args = inv.scope.split(" ").concat(["--", "--ignored"], inv.extraTestArgs ?? []);
  const run = spawnSync("cargo", ["test", ...args], {
    cwd: tauriDir,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    // 992 号超时哨兵：预算到点 SIGKILL cargo（无限挂死→有限超时+红报）
    timeout: inv.timeoutMs,
    killSignal: "SIGKILL",
  });
  const output = `${run.stdout ?? ""}\n${run.stderr ?? ""}`;

  // 超时分支先行：SIGKILL 非零退出 + 无完整 test result 可解析——keychain
  // 锁定（dark wake）/securityd 异常/编译器僵死都是此形状。测试二进制可能
  // 成孤儿（spawnSync kill 只达 cargo 直接子进程，903 族），红报自带清理指引。
  if (run.signal === "SIGKILL" || run.error?.code === "ETIMEDOUT") {
    console.error(
      [
        `✗ [${inv.scope}] 超时哨兵触发（预算 ${inv.timeoutMs}ms 被 SIGKILL）：`,
        "  高概率根因 = keychain 锁定（dark wake）/securityd 异常（e2e_secrets、lib keyring 面）/编译僵死。",
        "  解锁屏幕确认 keychain 可达后重跑；孤儿清理特征：",
        "    pkill -f inkos-desktop; pkill -f inkos-engine-server; pkill -f 'sync_watcher_io|e2e_secrets|inkos_desktop-'",
        output.slice(-800),
      ].join("\n"),
    );
    failed = true;
    continue;
  }
  try {
    writeFileSync(logFile, output);
  } catch {
    // 落盘失败不阻断——解析走内存副本
  }

  const okTests = [...output.matchAll(/^test (\S+) \.\.\. ok$/gm)].map((m) => m[1]);
  const failedTests = [...output.matchAll(/^test (\S+) \.\.\. FAILED$/gm)].map((m) => m[1]);
  const expected = new Set(inv.tests);
  const actual = new Set(okTests);
  const missing = inv.tests.filter((t) => !actual.has(t));
  const extra = okTests.filter((t) => !expected.has(t));

  if (run.status !== 0 || failedTests.length > 0 || missing.length > 0 || extra.length > 0) {
    const lines = output.split("\n").filter((l) => /panicked|FAILED|^error|warning: unused/.test(l));
    console.error(lines.slice(0, 40).join("\n") || output.slice(-1500));
    if (failedTests.length > 0) {
      console.error(`✗ [${inv.scope}] 挂起用例实测红：${failedTests.join(", ")}（日志 ${logFile}）`);
    }
    if (missing.length > 0) {
      console.error(
        `✗ [${inv.scope}] 清单地板卫兵：${missing.length} 枚登记用例未观测到绿——被删/改名/过滤器失效都会走到这（日志 ${logFile}）`,
      );
    }
    if (extra.length > 0) {
      console.error(
        `✗ [${inv.scope}] 清单地板卫兵：观测到 ${extra.length} 枚未登记用例——新增挂起用例须有意更新 INVENTORY（日志 ${logFile}）`,
      );
    }
    if (run.status !== 0 && failedTests.length === 0 && missing.length === 0 && extra.length === 0) {
      console.error(`✗ [${inv.scope}] cargo 非零退出（exit ${run.status}；日志 ${logFile}）`);
    }
    failed = true;
    continue;
  }

  console.log(`✓ [${inv.scope}] ${okTests.length}/${inv.tests.length} 枚挂起用例全绿，清单精确匹配（日志 ${logFile}）`);
}

process.exit(failed ? 1 : 0);
