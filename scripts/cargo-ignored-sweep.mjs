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
//
// 已知豁免（不在本脚本，理由备案于变更记录 988 号）：
// - tests/e2e_secrets（9 枚）：真实 keychain e2e，注释明示「dark wake 时首次
//   访问弹授权框阻塞」——无人值守跑会挂会话。
//
// 纪律：**勿与其它 cargo test 并行跑**——同机双 cargo 实例会因 target 锁 +
// securityd/keychain 跨进程串行化造成分钟级停滞假象（988 号自身双跑实测）。
// 网络面两枚需外网可达；pfctl 枚需非 root macOS。全程约 40-60s（release
// 陈旧时另计编译）。

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
    tests: [
      "watcher_anti_loop_programmatic_write_does_not_writeback",
      "watcher_spa_write_writebacks_to_keychain",
      "watcher_shutdown_flag_exits_thread_cleanly",
    ],
  },
  {
    scope: "--test e2e_updater",
    tests: ["test_is_newer_version_check"],
  },
  {
    scope: "--test supervisor_integration",
    tests: ["real_inkos_sidecar_serves_spa"],
    // 真实 sidecar 前置：src-tauri/engine 组装（幂等；TS dist 缺失时脚本内
    // 自行触发 pnpm build）。缺组装时用例自带「engine 未组装」panic。
    pre: [join(repoRoot, "scripts", "desktop-package-engine.sh")],
  },
  {
    scope: "--release --test e2e_startup",
    tests: ["test_startup_smoke", "test_startup_logs_version"],
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
  const args = inv.scope.split(" ").concat(["--", "--ignored"]);
  const run = spawnSync("cargo", ["test", ...args], {
    cwd: tauriDir,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  const output = `${run.stdout ?? ""}\n${run.stderr ?? ""}`;
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
