# 380 号：R12 契约批——sqlite-vec 真向量检索升级（三轮 P0 末件）

- 日期：2026-09-13
- 类型：契约层 + 双端镜像 + golden 差分
- 规划依据：[v3 规划 §3 R12](../../开发时SpecCoding'sPlan/inkosDesktop/07_产品演进规划/对标调研分析与改进规划_v3.md)（347 号内存余弦替代方案的规模化升级路径；风险对策=feature 探测失败自动回退）

## 做了什么

1. **引擎决策纯函数双端**（`vector-engine.ts` / `vector_engine.rs`）：
   `resolveVectorEngine` 决策表——
   - 扩展不可用 → **memory-cosine**（一票否决，347 行为不变）；
   - 可用 + chunkCount > threshold（缺省 5000，严格大于）→ **sqlite-vec**；
   - 可用 + ≤ threshold → memory-cosine（小规模内存更快）；
   - threshold 可覆盖。
2. **golden**：`vector-engine-vectors.json`（决策表 5 例：不可用回退/
   阈值下不切换/超阈值切换/自定义阈值/边界等于阈值不切换）；TS 2 用例
   与 Rust 1 差分读同文件全绿（第二十六守门域）。
3. **TS 导出**：resolveVectorEngine/SQLITE_VEC_DEFAULT_THRESHOLD/
   SQLITE_VEC_PATH_ENV（env 覆盖扩展路径探测用，接线批消费）。

## 兼容性决策

- 探测封装采用「注入式」：vecExtensionAvailable 由调用方探测后传入
  （TS 接线批经 better-sqlite3 loadExtension 试加载 env
  INKOS_SQLITE_VEC_PATH 指向的扩展；Rust 侧 rusqlite 需
  load_extension feature——当前未启用，探测结果恒 false 即回退路径，
  行为安全）；决策函数保持纯函数可全量 golden。
- 边界语义：chunkCount == threshold 不切换（严格大于）。

## 验收（golden×5 + duel + 双端全绿）

- core：tsc 干净；vitest **233 文件 / 2062 用例**全绿（379 基线 232/2060
  + 1 文件 2 用例）。studio tsc 干净；vitest 802 全绿。
- engine-rs：`INKOS_DUEL=1 cargo test` **33 个测试目标全 ok**（+1 新增
  差分目标 golden_vector_engine_diff）。
- bindings **180** 全绿；bench:gate 静默窗通过（首轮 +514.8% 假回退与
  负载拒跑，隔离复跑绿）。

## 影响面

- 新增 4 文件（TS 契约/golden 测试/共享向量/Rust 镜像+差分），修改
  2 文件（utils mod.rs + index.ts）。
- 未触碰并行会话在途文件（package.json 根文件、rustbin.rs、audit-rust.mjs）。

## 里程碑

- **R12 契约批落地——三轮 P0 三件（R10/R11/R12）全部收官。** 接线批
  （381 号）：混合召回引擎切换接线（探测+超阈值启用 sqlite-vec + 回退）
  + Doctor retrieval 引擎标注更新。此后 P1（R13–R15）/ P2（R16–R19）。
