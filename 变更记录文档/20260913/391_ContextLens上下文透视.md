# 391 号：R21 Context Lens 上下文装配透明面板——纯读投影全链（四轮 P0 第二件）

- 日期：2026-09-13
- 类型：契约批 + 接线批（纯函数投影 + golden + 读端点双端 + UI）
- 前序：[390 号 R20 接线](./390_系列正典接线.md)、规划 [388 号 v4](./388_四轮规划立项.md)

## 做了什么

1. **契约批（纯函数投影双端）**：`packages/core/src/utils/context-lens.ts`
   真源 + `engine-rs/src/utils/context_lens.rs` 1:1 镜像。
   `buildContextLens({contextPackage, trace})` 把每章治理落盘的两件工件
   （`chapter-NNNN.context.json` 预算后实际进入 prompt 的包 ×
   `chapter-NNNN.trace.json` 治理留痕）投影成单一只读视图：
   - entries 以 context.json 为准（实况），order=1 起装配序（G2 最终序）；
   - tier/tierPrecedence 按来源分类表现算（G2/390 号全表，含
     codex-series/→user-reference；未注册→ephemeral），protected 按
     `isProtectedContextSource` **独立判定、不信任留痕**（双端各有一条
     篡改 trace 分层不影响输出的对抗测试）；
   - tokens 复用 367 号估算口径 `estimateTextTokens(source+reason+excerpt)`
     （Rust 侧 `estimate_context_source_tokens` 由私有改 pub 复用），零新增计数器；
   - compiled = `source===trace.compression.compiledSource`；压缩段存在时
     `preCompressionSources` 透传 `sourceTokens`（压缩前原始来源与逐源 token）；
     compression 无则显式 null（Rust 不加 skip，序列化对齐）；
   - rank 特征透传（缺省 null）；totals 为 entries 聚合。
2. **共享 golden 向量**：`golden/context-lens-vectors.json`（Python 生成器
   独立实现投影契约，三方一致才算过）3 组用例：全层级装配（9 条含
   rank 透传/空 excerpt 剔除/未注册来源 ephemeral）、预算压缩后实况
   （2 受保护+1 编译条目+3 条压缩前留痕）、冷启动空装配。TS
   `golden-context-lens.test.ts`（2 断言：逐向量 deep equal + 篡改对抗）；
   Rust `tests/golden_context_lens_diff.rs` 同源差分（2 测试）。
   **双端差分首跑即绿**。
3. **读端点双件双端**：GET `/api/v1/books/:id/context-lens`（扫 runtime
   目录 `chapter-NNNN.trace.json` 回升序章节号列表；无留痕空数组）+
   GET `/api/v1/books/:id/context-lens/:chapter`（读双工件→schema 校验→
   投影返回；缺工件 404、非法章节号 400）。TS server.ts 动态 import core
   走真实 `buildContextLens`+两 schema 校验；Rust books_state_routes.rs
   serde 解析+`build_context_lens`，mod.rs 路由注册（codex 路由旁）。
   **纯读零写作链侵入**。
4. **UI**：`ContextLensPanel.tsx` 挂书籍详情（CodexPanel 之后）——章节
   选择器（默认最新章）、totals 概览行（条目/受保护/tokens + 预算状态
   徽标）、逐条目行（装配序/来源/层级标签/保护/编译徽标/~tokens）、
   压缩留痕块（被编译的原始来源+压缩前 token）、留痕备注；层级七层
   中英标签映射。

## 验收

- core：vitest **238 文件 / 2076 用例**全绿（+1 文件 +2 用例）。
- studio：tsc 干净；vitest **810 用例**全绿（+1 端点行为测试：空列表/
  回放投影/404/400 四分支）。
- engine-rs：`INKOS_DUEL=1` **35 个测试目标全 ok / 1759 用例** exit 0
  （+1 lens 差分目标 +2 用例）。
- bindings **184 全绿**（ContextLens/Entry/Compression/Totals 四类型
  新导出；首跑漏 `use ts_rs::TS` 条件导入被门禁当场抓住——门禁有效）。
- bench:gate：机器负载超阈（loadavg 137%/核，系统级 PerfPowerServices/
  mds 索引突发；非本批工作负载）两度拒绝执行——按 257 号纪律不强跑。
  本批 diff 未触碰任何 bench 覆盖路径（context_assembly 仅私有 fn 改
  pub，零行为变更），待静默时段补跑复核。

## 教训

- **bindings 门禁是活的**：新结构体带 `ts(export)` 时必须随附
  `#[cfg(feature = "export-bindings")] use ts_rs::TS;`——普通 cargo
  test 编译不过此 feature，只有 bindings 门禁会暴露。
- **studio server.test.ts 的 core mock 是显式白名单**（不展开
  `...actual`）：端点动态 import core 的新导出（buildContextLens/两个
  Schema）必须逐个加进白名单透传，否则端点内 `core.X` 为 undefined
  报 500。376/390 号的 deriveCodexCards/isValidSeriesId 未入白名单
  是因为其端点路径无 studio 行为测试覆盖——本批补测试时暴露此暗坑。
- 全量 cargo test 输出用 `| tail` 截断会丢掉大部分目标结果行——
  必须**完整落盘再 grep 汇总**（本批为此多跑一轮，编译已缓存仅重执行）。

## 影响面

- 修改 8 文件 + 新增 5 文件（context-lens.ts/.rs、vectors、双端测试、
  ContextLensPanel）；纯读零行为变更（未绑定的写作链路逐字节不变）。
- 未触碰并行会话在途文件（package.json、rustbin.rs、audit-rust.mjs）。

## 里程碑

- **R21 Context Lens 收官——四轮 P0 第二件落地**：作者可回放"模型
  看见了什么"（层级/序/保护/token/压缩前编译账），367 号留痕面首次
  有了消费端。
- 下一步=**392 号 R22 反AI规则种子包**（363 号推进模式库种子先例，
  规则库冷启动内置兜底）→ 393 号 R23 伏笔类型标注（≤7 类，存量零
  迁移）；R16 G12 重评（quality-first 跑通率>60% 触发）与 R19 真实
  反馈项待真实长跑数据，不排期。
