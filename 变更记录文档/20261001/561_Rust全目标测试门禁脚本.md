# 561 号：Rust 全目标测试门禁——cargo-test-gate.mjs（--lib 盲区根治）

日期：2026-10-01
类型：chore(scripts)，560 号教训的工程化清偿

## 选题来路

560 号暴露 556 号潜伏回归：`cargo test --lib` 只覆盖 lib 内嵌单测，tests/
目录的独立 test 目标（engine-rs 42 个 + src-tauri 23 个——e2e/golden/duel
全族）永远在每轮门禁覆盖外。这是 569 号「cargo check 不覆盖 test 目标」
同类教训第二例（516 号 build_launch 第5参、560 号 research85），两例都是
靠 clippy --all-targets 编译拦截才现形——编译拦截只保编译不保行为。

## 实施

- 新建 `scripts/cargo-test-gate.mjs`（仿 clippy-gate 433 号形态）：
  - 双 crate `cargo test --all-targets`，spawnSync 落盘日志再解析（532/552
    号 cargo 管道统计伪象两次实证——禁 |grep|awk）；
  - 汇总每个目标 test result：FAILED 任一或 status 非 0 即 exit 1，并给出
    目标计数与累计 passed 与日志路径；
  - **engine-rs 侧固定注入 `INKOS_DUEL=1`**——strangler_duel 未设 env 时
    早退也计 pass（170/172 号假绿事故），--all-targets 会把 duel 一并跑掉，
    env 在脚本内注入不依赖调用方记得（duel-requires-inkos-duel-env 记忆
    的工程化落位）；
  - 不含 doc-tests（538 号 576vs578 口径差备案）；
  - package.json 挂 `cargo:testgate`。
- 首跑真验证：engine-rs 42 目标 1873 passed + src-tauri 23 目标 576 passed
  全绿（src-tauri 的 19 个 tests/ 目标首度纳入；engine-rs golden_*/e2e 全族
  首次在门禁口径一次全跑）。

## 门禁纪律定位

`cargo:testgate` 与 `clippy:gate` 平级，属专项门禁（跑频次低于 gate:ts——
冷编译首跑数分钟），建议在大改族/发布前/每专项收口时跑；禁止为提速注
skip/过滤洗门禁（脚本头注明纪律）。

## 教训

- 同类教训三连（516 build_launch / 560 research85）说明「--lib 绿」的
  证据面只到内嵌单测——凡 tests/ 目录有独立目标的 crate，完成度主张
  必须以全目标口径为锚。
- env 依赖型测试（duel）在全目标口径下会被无 env 假绿吞掉——门禁脚本
  必须在脚本内固化环境注入，不能指望调用方记得 export。
