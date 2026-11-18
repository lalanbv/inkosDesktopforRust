# 1581 src-tauri 死代理活体补齐轮（2026-11-19）

## 定性

按 /goal 逐面扫：回环代理族八站中 src-tauri 三站（1572 最初修复面）是唯一无死代理行为级活体的（当时只有真环境红转绿实证：用户系统代理开着时 3.00s 恒超时→0.61s 绿），engine-rs 七站全有（1573/1575/1577/1580）。本轮补齐终局最后一块。

## 补测（1 活体，无生产代码变更）

**`src-tauri/tests/health_probe_loopback_no_proxy_live.rs`**（独立进程防 env 污染）：死代理 env（127.0.0.1:1）+ 回环手写 200 mock（同 supervisor_integration 手写形态）→ `health_probe` 必须直连成功。**撤修转红四拍**：删 `.no_proxy()`（1572 修正前形态）→ 探活经死代理连接即败 → 3s deadline 全败返回 false → 3.19s 红 → 复原 → 0.21s 绿。既有 supervisor_integration mock 双测复跑绿（0.61s）。

注：probe_once 为 health_probe 薄封装（同款 client 链）、observer/sse 同谓词函数族——由本活体一并锁定语义；SSE 流级独立活体边际价值低，备案不扩。

## bench:gate 复试（仍拒，维持备案）

负载卫兵再拒：loadavg 4.34/18 核=24% > 20%（较 1579 轮 32% 回落但仍超阈，Edge+会话 UI 稳态）。维持 1448 号定性：静默窗口只能用户侧发起，热路径复扫留待静默时段。

## 验证

- cargo:testgate 双 crate **2729 passed**（engine-rs 58 目标 2129 + src-tauri **26 目标 600**，+1 活体）；clippy 0；fmt 0；U+FFFD 净；撤修转红四拍闭环。

## 后续

- 下号自 1582 起；用户侧待办不变（Fork 推送 955-1581 + 正式打包三脚本重烤 + 静默时段 bench:gate）。
