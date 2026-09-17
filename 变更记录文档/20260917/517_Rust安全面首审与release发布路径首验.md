# 517 号：Rust 安全面首次审计 + release 发布路径首验（cargo 解除后新开面补验）

日期：2026-09-17　分支：develop　基线：226b9004（516 号）

## 背景

516 号清算的是许可阻断期**排队**的欠账（debug 测试面）；本号盘点 cargo 解除后**新打开**的两张真实缺口面：① Rust 依赖安全面从未审计（TS 侧 npm audit 506 号已清零，cargo audit 一直被链接阻断挡在门外）；② release 发布路径从未在本基线验证（504 号发布路径复验仅覆盖 TS 产物链，`cargo build --release` 当时被许可阻断跳过）。

## 改动

### 1. cargo audit 双 crate 首审与清零

| crate | 首审结果 | 处置 | 复审 |
|---|---|---|---|
| engine-rs | 1 vulnerability（RUSTSEC-2026-0285 rustls 0.23.43，TLS 1.3 跨加密层边界误接受，medium 5.3）+ 2 warnings | `cargo update -p rustls` → 0.23.45（连带 rustls-webpki 0.103.15）；`cargo update -p chacha20` 0.10.1→0.10.2 出 yanked | **0 vulnerability**；余 1 warning |
| src-tauri | 同一 rustls 漏洞 + 8 warnings（全为 tauri/wry/gtk 生态传递） | `cargo update -p rustls --precise 0.23.45`（裸 update 因镜像索引判"最新兼容"未动，需 precise 强制） | **0 vulnerability**；余 8 warnings |

剩余 warnings 定性（全部备案，不可本地修）：
- engine-rs：`ttf-parser` unmaintained（pdf-extract→lopdf 0.42 传递，0.25.1 已是分支最新，等上游迁移 read-fonts）。
- src-tauri：`fxhash`/`unic-*` unmaintained、`glib` 0.18 unsound 等——tauri 2.x 生态传递依赖，等上游 tauri 升级。

### 2. release 发布路径首验

- `cargo build --release` 双 crate 全绿：engine-rs 45.9s + src-tauri 36.2s。
- 两活体套件升级：`INKOS_SMOKE_RUST_BIN` env 可指二进制路径（默认 debug 路径不变，零行为漂移）。
- **release 二进制活体冒烟双绿**：`INKOS_SMOKE_RUST_BIN=…/target/release/inkos-engine-server` 跑 export-epub-smoke（双引擎 8 断言）与 node-fallback-smoke（rust 腿健康探针/fixture/director/task-routing/no-store/run-log 全过）——发布形态（optimized + rustls 0.23.45）全链可用。

## 验证

- engine-rs 全量 cargo test：**1803 绿**（升级后复跑）；src-tauri 单测：**468 绿**。
- `cargo audit` 双 crate：0 vulnerability（warnings 见上，备案）。
- release 冒烟双套件全绿（见上）。
- TS 侧零改动。

## 后续

- ttf-parser / tauri 生态 warnings：随上游版本滚动跟踪（可入 npm 接受风险同型的滚动复核节奏）。
- `cargo audit` 可纳入发版前清单（本地脚本发版流程，无 CI）。
- resync 架构分歧产品决策（483 备案）仍留守，差分器豁免面待专项。

## 关联

- 506（npm audit 清零——本号为 Rust 侧同型清零）、504（TS 发布路径——本号补 Rust 侧）、483/484（resync 备案）、490（写入面差分——套件 env 化沿用其基建）。
