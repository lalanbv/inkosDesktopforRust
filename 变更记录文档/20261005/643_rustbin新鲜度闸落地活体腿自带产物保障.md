# 643 号：gate 前置 rust-bin 二进制新鲜度闸——活体腿自带产物新鲜度保障（641 建议清偿）

日期：2026-10-05。类型：chore(scripts)+gate 结构。前置：641 号（a0d118af）实证 gate:ts 活体步骤消费磁盘既有 `target/debug` 二进制而 cargo test 不编译 bin 目标，「先重建再判读」七次人肉教训后建议另号可议——本号把它变成门禁结构。

## 一、设计：cargo 指纹为新鲜度 oracle，宁红勿假绿

新增共享模块 `scripts/ensure-rust-bin.mjs`（CLI + import 双形态）：默认路径下先 `cargo build`（增量，新鲜时亚秒空转 0.2s 实测），以 cargo 自身指纹判定新鲜（严格强于 641 试探的 mtime-vs-HEAD 探针——源码任意改动必被 fingerprint 捕获）。合约四态：

| 状态 | 条件 | 行为 |
| --- | --- | --- |
| fresh/rebuilt | cargo 可用且 build 通过 | 放行（重建时输出可见） |
| build-failed | cargo 可用但 build 失败 | **exit 1 拒绝**——陈旧二进制不得出具活体证据 |
| uncertified/missing | cargo 不在 PATH | 告警放行——保持「cargo 链接受阻不影响 TS 门禁可用性」既有合约（Xcode 先例），缺失时套件既有 skip/快速失败语义兜底 |
| override/skipped | `INKOS_SMOKE_RUST_BIN` 已设（517 形态外部产物）/ `GATE_TS_SKIP_RUST_BIN=1` | 不构建；前者新鲜度调用方自管，后者整闸豁免出口（gate:ts 与套件同认） |

## 二、接线面（scripts 5 文件，packages/Rust 零触碰）

1. `gate-ts.mjs`：非 fast 分支首步插入 `rust-bin`（七步→八步）；该步成功时 stdout 证据行强制可见（其余步骤成功保持静默）——降级/重建信息属活体证据面，不许被汇总吞掉。
2. `node-fallback-smoke.mjs`：`ensureRustBinFreshOrExit` + legs 计算上移到任何子进程 spawn 之前（build 失败早退不留孤儿子进程；mock 启动等待/断言原样保留）；node 单腿不消费 Rust 二进制不启用。
3. `export-epub-smoke.mjs`：同上（legs 本就在 mock spawn 前，仅插闸）。
4. `engine-contract-diff.mjs`：rustBinary 补 `INKOS_SMOKE_RUST_BIN` 解析（517 形态对齐，顺带解锁 release 二进制差分形态）+ 缺失二进制快速失败并给可行动指引（旧形态 spawn 失败后要干等 30s 启动超时才崩，实测新形态 0.068s）。

## 三、验证：七场景 + 全量八步门禁

- **A fresh**：cargo 空转 0.23s，「cargo 指纹核验新鲜」，exit 0。
- **B 陈旧（可证伪核心）**：`touch engine-rs/src/lib.rs` → 5.8s 增量重建 →「增量重建」→ 二进制 mtime 1791206561→1791213927。旧码无闸时此处静默放行（641 已实证 stale+smoke 照绿=红证据面）。
- **C 源码破碎**：lib.rs 追加垃圾行 → exit 1 + cargo 错误尾输出（拒绝出具证据）；`git checkout` 还原后复跑绿。
- **D override / E skip / F cargo 缺失**：三合约各归其位（自管/豁免/告警放行，均 exit 0）。
- **G diff 快速失败**：`INKOS_SMOKE_RUST_BIN=/nonexistent` → 0.068s exit 1 + 指引（旧形态 30s 挂起）。
- **全量**：`pnpm gate:ts` 八步全绿（build 16.9s/typecheck 16.0s/test 75.2s/audit 2.1s/rust-bin 0.2s/smoke 34.3s/diff 40.3s/epub 34.3s），rust-bin 证据行在门禁输出可见；smoke 双腿+差分 61 端点 0 分歧+EPUB 结构校验在新鲜二进制上复跑绿。
- Rust 门禁裁剪备案（535 先例）：本号 scripts-only 零触碰 engine-rs/src 与 src-tauri，HEAD 8ece52bd Rust 全绿 + 641/本号 B 场景双重证明当前二进制与 HEAD 同源。

## 四、备案

- duel/bench 等消费二进制的 Rust 门禁不在本闸射程（属 Rust 门禁编排，cargo run/test 天然自带编译）；未来若有消费既有产物的 Rust 门禁步骤可复用本模块。
- `uncertified`（cargo 缺失+二进制在）是唯一「可能陈旧仍放行」的残余态——环境性约束下的既有合约保留，告警可见属明示降级而非静默假绿。
- fast 模式不跑活体步骤故不设闸（无消费方，设闸纯浪费）。

## 五、教训

1. 门禁步骤按「证据可自证性」分级后，缺口要用**结构**清偿而非教训叠教训——七次人肉记忆挡不住第八次，闸一次挡完。
2. 新鲜度 oracle 选 cargo 指纹而非 mtime 对比：指纹天然覆盖「源码改了但 mtime 更旧」（checkout/回拨）等边角，且语义就是构建系统自己的判据，零自研零漂移。
3. 闸的失败要「快速+可行动」：diff 旧形态 30s 超时才崩且报「启动超时」（误导性归因），新形态 0.07s 给出修法指引——失败面本身也是契约面。
