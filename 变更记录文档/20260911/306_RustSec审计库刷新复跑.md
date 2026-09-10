# 306 号：RustSec 审计库刷新复跑——双锁 0 漏洞维持（警告 10 条均无直接修复路径）

- 日期：2026-09-11
- 分支：develop
- 关联：263 号（wasmtime 升级审计——本批为 advisory 库更新后的复跑）、286 号（Node 侧审计对称面）
- 编号衔接：查 20260911 目录最大号 305，顺延 306
- 推送核验：origin/develop = 498253fa（300 号）未动；本地领先 301–306 共 6 提交待推

## 一、方法

RustSec advisory-db 每日更新——即使 Cargo.lock 未变（自 263 号零改动，277 号核验），**新发布的通告也会浮现**。本批以最新库对双锁复跑 `cargo audit`。

## 二、结果

| 锁 | vulnerabilities | 警告 |
|---|---|---|
| engine-rs | **0** | 2：`ttf-parser` unmaintained（RUSTSEC-2026-0192）、`chacha20` yanked |
| src-tauri | **0** | 8：`fxhash`/`proc-macro-error`/`unic-*` 五件 unmaintained、`glib` unsound（Iterator unsoundness，RUSTSEC-2024-0429） |

全部为 unmaintained / yanked / unsound 类**警告**（无 CVE 级漏洞）；均为 tauri 栈/字体栈的传递依赖，无直接修复路径，随上游更新。与 263 号「0 漏洞」结论一致且经最新库再确认。

## 三、验证性质与遗留

纯审计批，零代码改动。遗留：301–306 共 6 提交待推送；并行会话三件第三十四轮在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
