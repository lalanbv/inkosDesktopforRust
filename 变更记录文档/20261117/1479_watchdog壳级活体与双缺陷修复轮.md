# 1479 号：watchdog 壳级活体与双缺陷修复轮——Rust 直启形态死契约面闭环

## 目标与范围

/goal 自主持续开发续轮。巡检未到期（2026-11-17）。1459 parent-watchdog
的反向链（SIGKILL 壳→引擎自退）只有 bin 级集成测——**app 级全形态从未
真跑**（1473/1474 清理走 SIGTERM=cleanup 路径不触发 watchdog）。本轮
活体验证抓出**双缺陷**并修复闭环。

## 双缺陷（活体独有发现）

①**注入漏（死契约面）**：`rustbin.rs build_launch`（Rust 直启=产品
默认形态）env 只有 PORT/PROJECT_ROOT——**无 INKOS_PARENT_PID**。1459
三腿的「壳注入腿」只改了 supervisor.rs（Node 回退腿 spec），引擎侧
watchdog 就位（bin 级绿）但默认形态不喂 pid=env 有消费端无注入端的
死契约（1431 形态）。修复：rustbin.rs 补注入+env 契约测试断言锁
（防回归）。

②**烤入引擎二进制陈旧**：`src-tauri/target/release/engine-rust/
inkos-engine-server`=**8 月 24 日**（v0.2.0 发版产物）——1459 的引擎
侧修复从未进烤入面（release 从未重编，testgate 只编 debug）。修复：
engine-rs release 重编+重烤入。**用户重烤待办的重要提醒**：正式打包
三脚本重烤**必须含 engine-rs release 重编**（否则打包形态仍带 8 月
旧引擎，watchdog/此后全部引擎改动全部缺席）。

## 活体终证（修复后）

release app 直跑→SIGKILL app（04:55）→引擎 **watchdog 自退**→
**进程表零残留**（WATCHDOG-CONFIRMED）——1459 三腿在产品默认形态真
闭环：壳 SIGKILL 后引擎 ≤4s 自退（2s 轮询），不再依赖 sweep 事后清。

## 验收

- rustbin 测试族全绿（含新 env 契约断言）；clippy 双 0；testgate
  src-tauri 25 目标 598 全绿
- 活体：修复前引擎残留（PPID=1）→修复后零残留（强对照）

零新缺陷遗留，U+FFFD 净。

出口=下一巡检周期（2026-11-18 到期，明日触发）/earendil-mz 单包合并；
用户侧待办=**Fork 推送（含 955-1479）+正式打包三脚本重烤（含 engine-rs
release 重编——本轮教训）**。下一号自 1480 起。
