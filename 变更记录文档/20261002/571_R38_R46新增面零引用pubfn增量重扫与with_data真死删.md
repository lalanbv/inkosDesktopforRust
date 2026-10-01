# 571 号：R38–R46 新增面零引用 pub fn 增量重扫（537 法）——59+52 全域清洁，唯一真死 with_data 删除

- 日期：2026-10-02
- 类型：chore(src-tauri)（死代码清偿，一行构造器删除）
- 路线：570 号盘点备案项核销（「零引用重扫增量小待下轮」→本轮兑现）
- 状态：✅ 门禁全绿收口

## 扫描面与方法（537 号方法）

对象=R38–R46 新增面全量：interaction/{registry,spill,author_skill_tool,session_transcript}+utils/token_meter+skills/{mod,external_loader,production_bindings}+llm/agent_trajectory（59 个唯一 pub fn）+ 插件域 src-tauri/plugin/*（52 个）。方法=pub fn 名全量提取→全仓（src+tests 双 crate）逐名引用计数（排除定义行/文档注释）→零引用候选人工定性（grep 驼峰变体教训：计数管道须排除定义行而非含 fn 串——首版管道误报 builtin_skills_root，人工核实为 env 解析缝活引用[373 行调用+489 号 INKOS_BUILTIN_SKILLS_DIR 部署依赖]）。

## 结果

- **R38–R46 新增面 59 个 pub fn：零真死代码**——全部有活引用（含测试消费）。新增面卫生状况显著优于 537 号时期（当时 854 个扫出 4 候选）。
- **插件域 52 个：唯一真死=`RpcError::with_data`**（protocol.rs）——全仓零引用（src+tests），错误构造恒走 `new`；插件域 src-tauri 专属无双端同构约束（删除最高约束不适用）。**真死删**，data 字段保留（解析插件响应侧反序列化仍需；JSON-RPC data 成员的构造需求到达时以测试同行引入，删除点留档注）。
- 537 号三处保留备案（continues_pipeline/creates_debt/upsert/count）未复发新增零引用。

## 明确不做

- `builtin_skills_root`：活引用（env 解析缝），保留。
- TS 侧新增面：本专项范围为 Rust pub fn（537 法口径）；TS 出口面由 tsc+index 出口测试覆盖。

## 门禁

clippy 双 0 + cargo:testgate（含 570 审计哨兵）engine 45 目标 1893 + src-tauri 24 目标 583 全绿。TS 侧零改动，gate:ts 不触发（Rust-only 口径备案）。

## 下一号自 572 起
