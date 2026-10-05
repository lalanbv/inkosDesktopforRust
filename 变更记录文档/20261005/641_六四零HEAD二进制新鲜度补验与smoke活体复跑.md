# 641 号：640 HEAD 二进制新鲜度补验 + smoke 活体腿复跑（零代码改动）

日期：2026-10-05。类型：docs+chore(smoke 复跑)。前置：636 号（4e20e9b9）全量复验收口后，并行会话相继落库 637–640（HEAD 移至 8ece52bd，其中 640 号清偿了 636 走查备案的 pending-abort 受理边界）。本号=对**当前 HEAD** 的完成审计补强。

## 一、发现：640 门禁的活体腿二进制新鲜度无法自证

640 号提交声称全量门禁全绿（testgate engine 1926(+5)/tauri 584、clippy 双 0、duel 10/10、差分 61 端点 0 分歧、gate:ts 七步、bench 候谷零回退）。逐面核对证据可自证性：

- **源码级面全部自证**：cargo test/testgate 从源码现场编译执行，二进制陈旧不影响其结论——1926 passed 含 pending-abort 的 +5 新测试，640 的 Rust 逻辑已被证明。
- **唯一缝隙=gate:ts 的 smoke/差分器活体腿**：node-fallback-smoke 的 rust 腿与差分器起的是磁盘上既有的 `target/debug/inkos-engine-server`，不自带重建。若 640 改 Rust 后未重建 bin 就跑七步，smoke 照样绿（smoke 面无 abort 断言，改的面它探不到）——绿但测的是旧代码。

## 二、实证与补验

1. `cargo build --bin inkos-engine-server`：**重编 19.5s**（坐实 build 前二进制不含 HEAD 全部 Rust 改动），重建后 mtime 21:22 > HEAD commit 19:55，二进制与 HEAD 同源。
2. 重建后 `node-fallback-smoke` 双腿复跑：**全绿**——rust 腿 15 断言（fixture 一致性/director/task-routing/no-store×2/genre 穿越探针/**637 新增 branch 探针族 8 条**（parentSeq 恒带/branch 回提交点/branch_moved 落盘/derive head 透出）/run-log/context-meter×2）+ node 腿全过。当前 HEAD 的活体证据补实。

## 三、定性

- 640 各门禁结论**不受影响**（源码级面自证 + smoke 面与其改动无交集 + 本次新鲜二进制 smoke 复跑绿）。
- 但「先重建再判读」（623/628/632，本号前已六次实证）至今靠人记教训——本号实证它在门禁编排上存在**结构性残留**：任何消费 `target/` 既有二进制的门禁步骤（smoke 双腿、差分器、duel 起引擎）都可能静默测旧代码，且无任何信号。

## 四、建议（另号可议，未实施）

gate:ts 的 smoke 步骤前置二进制新鲜度闸（`cargo build --bin inkos-engine-server` 先行，或 mtime 对比 engine-rs 源码树 vs 二进制、陈旧即拒跑）——把七次复现的人肉教训变成门禁结构。改动面=scripts/gate-ts.mjs 或 node-fallback-smoke.mjs 一处，成本极低。

## 五、教训

1. **门禁步骤按「证据可自证性」分级**：源码级编译执行（cargo test/vitest）天然新鲜；消费既有产物的活体腿（smoke/差分/duel 起引擎）必须自带产物新鲜度保障，否则绿不等于测了新代码。
2. 二进制 mtime vs HEAD commit time 是最低成本的新鲜度探针（本号实测定性一条命令 19.5s 出结论）。
3. 并行会话密集落库期，跨会话引用门禁结论时按上述分级复核——提交信息里的「全绿」不区分这两类证据。
