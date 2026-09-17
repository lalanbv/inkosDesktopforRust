# 516 号：Xcode 许可解除——cargo 欠账全量清算（含 489/500 遗留断裂两处修复）

日期：2026-09-17
范围：`src-tauri/tests/rust_backend_launch.rs`（断裂修复）+ `engine-rs/src/server/book_create_routes.rs`（clippy 告警修复）；主体为零产品语义变更的欠账补跑。

## 背景

自 481 号起 cargo 链接被 Xcode 许可阻断（exit 69），全部 cargo test / duel / bench:gate 排队约 30+ 循环。本轮实测 `cargo build` 解除（42.7s 成功），按待清算队列一次性清算。

## 欠账清偿清单

| 项 | 结果 |
|---|---|
| engine-rs 全量 cargo test | **1803 passed / 0 failed**（含 500 号 2 个灵感卡单测：`writes_inspiration_card_with_defaults` / `never_overwrites_existing_inspiration_card` 实跑确认） |
| src-tauri 全量 cargo test | 单测 **468 passed**（含 489 号 3 个：`resolve_builtin_asset_dirs_requires_both_dirs` / `build_launch_injects_builtin_dirs_env` / `none_when_no_candidate_exists`）+ 集成 `rust_backend_launch` 2 passed（修复后） |
| clippy:gate（--all-targets 双 crate） | 修复 1 处后 **双 crate 0 告警** |
| duel（INKOS_DUEL=1 真跑） | **10/10 passed，41.44s**（未设 env 时同套件 0.00s 早退假 pass——再次印证 duel env 记忆） |
| bench:gate | **9 基准零回退**，多数略优于 09-06 基线（parse_200 -6.9%、title_dedup -7%~-11.4%、sse dispatch/1 -12.8%）；首轮被负载守卫拦截（macOS XProtectRemegrator 扫描致 loadavg 17.8/18=99%），静候回落后通过 |

## 阻断期积累的断裂两处（本轮修复）

### 1. src-tauri 集成测试编译断裂（489 号遗留）

489 号给 `rustbin::build_launch` 增第 5 参 `builtin_dirs: Option<&(PathBuf, PathBuf)>`，但集成测试 `tests/rust_backend_launch.rs` 两处调用点未同步——`cargo test --test rust_backend_launch` 编译失败（E0061 缺参）。当时仅跑 `cargo check`（不覆盖 test 目标），许可阻断又使 test 无法运行，断裂潜伏至今。

修复：两调用点补 `None`（该二测试不涉及内置资源注入；注入态已由 rustbin.rs 单测双态覆盖）。

**教训强化**：`cargo check` ≠ test 目标可编译——正是「clippy 门禁须 --all-targets」记忆（433 号）同一盲区的第二实例；改函数签名时须 `cargo check --tests` 或 `cargo test --no-run` 兜底。

### 2. engine-rs clippy 告警（500 号遗留）

`book_create_routes.rs:517`：`&runtime.state.project_root()` 中 `project_root()` 已返回 `&Path`（state/manager.rs:53），外加 `&` 成 `&&Path`（clippy `needless_borrow`，-D warnings 拦截）。修复：去多余 `&`。

## 验证汇总

- engine-rs：1803 绿；src-tauri：468 单测 + 全部集成目标绿（ignored 项为 notify timing 依赖，按注释单独跑策略不变）。
- `pnpm clippy:gate` 双 crate 0 告警。
- `INKOS_DUEL=1 cargo test --test strangler_duel` 10/10（41.44s 真跑）。
- `pnpm bench:gate` exit=0（阈值 +30%，基线 2026-09-06）。
- TS 侧零改动；515 号三活体套件与 gate:ts:fast 当日已绿，不重复。

## 关联

- 481（POST 审计欠账）、489（rustbin 注入+资源打包）、500（灵感卡 Rust 侧写入）——三号欠账就此清偿注销。
- 433 号 clippy --all-targets 记忆、duel env 记忆、bench:gate 负载守卫记忆——本轮三度实证。
- 用户侧事项：Fork 推送积压提交（433–515 共 83+ 笔）；三库种子 canonical 已由 505 号证实双端一致、可关闭。
