# 433 号：clippy --all-targets 告警全量清零 + clippy:gate 门禁落地

日期：2026-09-14　性质：Rust lint 治理（重构/优化批）

## 背景

v6 对标复查（外部：ANWA 0.4.24 后无新发布、WNW 依旧零动静，无可采信号）转内部体检：
`cargo clippy --all-targets` 累积 40+ 条告警（lib 38 + 测试目标若干）——400 号确立的
"--all-targets 门禁" 标准实际处于告警未清状态，且无脚本门禁防回潮。

## 内容

### 1. 机械批（cargo clippy --fix --all-targets）

18 文件：needless_borrow×13（ops_routes×16 处中的大头、agent_loop×5）、
同型 cast×5（u64→u64）、manual_map、flatten 迭代、replace 链合并、
items_after_test_module（composer.rs 388 行纯搬移——+/- 行对称比对确认零语义变化）、
冗余闭包、未用 mut/import 等。

### 2. 手工批（9 类）

- `entity_roster.rs` / `info_gap_ledger.rs`：flush! 宏内 `has_current = false` 死赋值
  （中途展开后立即被 `= true` 覆盖；尾部展开后函数即返回）——移除；
- `task_routing.rs`：`FieldLeaf` 死枚举（早期实现遗留）——删除；
- `ops_routes.rs:1096`：`&[query.clone()]` → `std::slice::from_ref(&query)`（免一次堆分配）；
- `asset_library.rs`：`sort_assets(&mut Vec)` → `&mut [LibraryAsset]`（调用点零改动）；
- `style_feature_engine.rs`：sort_by → sort_by_key+Reverse（排序语义不变，仅比较器形态）；
- `tension_curve.rs`：doc 懒续行补空行；
- golden 测试死代码：`CandidatesCase`（roster）、`expected_of`（director）——删除。

### 3. 门禁固化

新增 `scripts/clippy-gate.mjs`（对 engine-rs 与 src-tauri 各跑
`cargo clippy --all-targets -- -D warnings`，任一告警非零退出；
缺失 clippy 以码 3 拒绝静默绿灯）+ package.json 挂 `clippy:gate`。
禁止注 allow 洗门禁；clippy 升级引入新 lint 转红属门禁本义（与 bench:gate 同纪律）。

## 验证

- `cargo clippy --all-targets`（engine-rs / src-tauri 双 crate）：**0 告警 0 错误**；
- `pnpm clippy:gate`：双 crate ✓ 绿灯；
- `cargo test`（engine-rs）：**1787 用例全绿**（1370 lib + 206 e2e + golden 套件），0 失败。

## 关联

package.json 的挂载行与 434 号（pnpm 死覆写迁移）同文件相邻 hunk，
随 434 号提交一并入库（见该号记录）。
