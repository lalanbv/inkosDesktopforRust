# 820 号：cargo 锁文件全量兼容刷新——117+107 项落地，serde_json 1.0.152+ 性能回退钉定

日期：2026-11-02。前置：819 号登记（cargo 117 项兼容刷新，testgate+bench:gate 双护航择机轮）。

## 一、执行

双侧锁文件全量兼容刷新（`cargo update`，仅 semver 兼容范围）：engine-rs **112 项**、src-tauri **107 项**（src-tauri 与 engine-rs 同一 testgate/bench 周期覆盖，合并执行零额外验证成本）。刷新后残余 `--dry-run` 仅索引刷新（behind-latest 均为 major，有意保留）。

## 二、护航与一次真实回退的二分定案

- clippy 双 0（117 项新版本编译零告警）；testgate **1980+584** 全绿；**RustSec 复审（新版本上）双 0**——刷新的安全收益落袋（传递依赖补丁链全部前移）。
- bench:gate 首跑 **红灯**：`sse_broadcast/dispatch/8` 117.97→207.90ns（**+76.2%**），其余全过。
- **二分定案**：异步族在案的变化=tokio 1.53.1→1.54.1、serde_json 1.0.151→1.0.154。钉回 serde_json 1.0.151 → **128.48ns 回归解除**（207.90 为 serde_json 1.0.152+ 引入）；再钉回 tokio 1.53.1 → 133.73ns（无益，噪声带 126-136）→ 恢复 tokio 1.54.1，**保留 serde_json 1.0.151 钉定**。
- 终态全量 bench:gate：**✓ 门禁通过**，`sse_broadcast/dispatch/8` 123.48ns（+4.7% 阈值内）。

## 三、serde_json 钉定说明

`engine-rs/Cargo.lock` 中 serde_json 保持 1.0.151（1.0.152+ 的 `serde_json::to_string` 微序列化路径存在 ns 级性能回退，SSE 派发每事件一次、位于热路径）。解除条件：上游 serde_json 修复后 `cargo update -p serde_json` 并复跑该 bench。src-tauri 锁未钉（其消费面无该热路径，跟随刷新）。

## 四、验证（终态）

clippy 双 0；cargo:testgate 47 目标 **1980** + src-tauri 25 目标 584 + **RustSec 复审双 0**；bench:gate ✓ 全过；gate:ts 八步全绿（含 engine-contract-diff + legacy 11/11 + export-epub-smoke）。TS 零触碰。diff 71/1 维持。

## 五、登记

- serde_json 钉定（1.0.151）：上游修复后解除并复测。
- 数值分协议扩展（上游面）维持为唯一实质余项。
- 刷新节奏定线：锁文件兼容刷新属周期巡检动作（随 RustSec 复审），非紧急不单发；本轮已建立「刷新→护航四门→二分钉回」的完整作业法。

## 六、教训

1. **护航序列的价值在二分**：117 项一次刷、四门护航、红一项 → 归因只需在在案变化里找（异步族仅 tokio/serde_json 两件）——「一次刷新 + 快速钉回」远优于逐项升级（117 轮）。
2. **`CycleAuditor` 教训的兄弟版**：ns 级 bench 的回退要怀疑「端口/协议错配」之外的**传递依赖微变**——serde_json 1.0.152 的序列化路径变化即足以 +76%。
3. 负载守卫 + 编译高峰衰减曲线（20→2.3 需 ~4 分钟）：大刷新后先跑编译类门禁（clippy/testgate 顺带消耗编译高峰），bench 排最后自然入窗。
