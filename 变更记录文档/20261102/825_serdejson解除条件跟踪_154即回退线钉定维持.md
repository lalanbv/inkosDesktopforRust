# 825 号：serde_json 解除条件跟踪——1.0.154 即回退线，上游无修复版，钉定维持

日期：2026-11-02。前置：820 号（serde_json 1.0.151 钉定，解除条件=上游发布修复版后升级并复测 sse bench）。

## 一、跟踪结果（定案）

crates.io API 实测（HTTP 直查，index 协议无关）：serde_json **max_stable = 1.0.154**（2026-10-11 发布），其后**无新版本**。1.0.152/153/154 同批发布即 820 号实测的回退线（dispatch 117.97→207.90ns，+76.2%）。

**解除条件定案未满足**：不存在可升的修复版——1.0.154 就是回退线的最新成员。`engine-rs/Cargo.lock` 的 serde_json 1.0.151 钉定维持。下一跟踪点=serde_json 发布 >1.0.154 的新版本时复测。

## 二、网络插曲（不影响结论）

cargo 走 ustc 镜像 index 与官方 index 均超时（`[28] Timeout reached`，网络面整体降级），`cargo update -p serde_json --precise 1.0.154` 的重试无法完成——但结论不依赖重试：crates.io API（HTTP）已实锤 1.0.154 为最新且即回退线成员，即便升级成功也只是回到已实测的回退态。lock 未被触碰（失败更新不落盘）。

## 三、其余巡检项

- 上游 `fetch --prune` 复跑：`master..upstream/master` 仍 **0**。
- RustSec/npm audit：820 号终态（刷新后 RustSec 复审双 0 + audit-npm 10/10 白名单）后无依赖变更，维持。
- bench：820 终态 ✓ 后无性能相关改动，维持。

## 四、登记

- serde_json 跟踪点：crates.io 发布 >1.0.154 → `cargo update -p serde_json` + 隔离跑 `sse_broadcast/dispatch/8` bench + 全量 bench:gate 护航。
- 数值分协议扩展（上游面）维持唯一实质余项。

## 五、教训

1. **解除条件的跟踪要查「最新版是否就是回退版」而非「有无新版」**：1.0.154 即回退线的最新成员——API 一次直查就定案（无修复版可升），不需要等网络恢复做无意义的升级重试。
2. 镜像/官方 index 的 git 协议超时与 HTTP API 可达性是两回事——元数据结论走 HTTP API 即可，不被 git/index 网络问题阻塞。
