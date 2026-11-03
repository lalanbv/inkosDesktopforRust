# 881 号：勘察轮——work 域 Rust 腿迁移设计（695 最大未迁域）

日期：2026-11-04。性质：**勘察轮**（零代码变更；产出迁移设计，全文存 `docs/plans/work-domain-rust-migration-design.md`——本地规划文档，1a2fd09b 先例不入库；本档收敛建议与分解）。

## 一、价值链分层（迁移顺序的依据）

work 域数据产生源（短篇/剧本生产 runner）同样只在 TS 腿——纯 Rust 桌面用户 /works 恒空，**只迁读取面没有端到端用户价值**。正确顺序：

```
L0 存储与契约（文件形状/sqlite schema/校验语义）
L1 读写面（works/artifacts/profiles/episodes CRUD + adopt）──UI 三面直接依赖
L2 生产管线（short-fiction-runner 等）──work 数据最大产生源
L3 agent 工具面（harness 工具进 agent_loop）
```

L0/L1 先行：双引擎交替读写立即可用（契约差分活体化、Node/夹具播种数据 Rust 可读、WorkInspector 活体）；端到端 Rust 创作价值在 L2。

## 二、TS 真源盘点

**端点 9 条**（server.ts L2807-2930）：`/works` 列表（updatedAt 降序+id tiebreak，Dashboard 前端过滤 longform-novel）、`/works/:id`（work+episodes limit50+chatRequest）、`/works/:id/artifacts`、revision GET（**sha256 校验** 409）、`…/adopt` POST（**executeExplicitCapabilityTool 能力框架环**：episode start→executeAction→finish+receipts + createAdoptWorkRevisionTool 的 authority 投影/冲突/checksum/DERIVED 校验）、`/episodes` 列表（workId/profileId/status/limit=100）、`/episodes/:id`（episode+events）、`/profiles` GET/POST/PUT（内置 9 + `.inkos/profiles/*.json`，409/400 语义）。

**前端三面**：WorkInspector（详情/artifacts/版本抽屉/adopt/episodes）、Dashboard works 区、CreativeMethodsEditor（profiles 列表+保存+`/skills/:id/documents` GET/PUT——该端点 Rust skill_routes **尚未实现**，随 L1 补）。

**core 依赖闭包 ≈1640 行 TS**：contracts 213（zod strict 全契约）/ work-store 103 / artifact-revisions 166（stage/promote/checksum）/ episode-store 342（node:sqlite；WAL+FK+busy_timeout=5000；enqueue per-session 串行）/ profile-registry+builtin 140 / 能力框架环（explicit-action 66+receipts 75+capability-registry 123+runtime 234）/ adopt 工具 179。上游 2.0 原生域、fork 无改动——**迁移基准=上游当前形态**。

## 三、Rust 现状：基建近乎齐备，增量依赖为零

- `utils/atomic_file_set.rs`（734 行，同源 TS 移植，760 号 fsync 加固）✓
- rusqlite 0.32 bundled（memory-db 先例；WAL/FK/busy_timeout 参数可对齐）✓
- sha2 0.10 ✓；路由分域文件惯例 ✓；876 金标准行测试模式 ✓

缺口六项（详见设计档案 §3.2）：zod strict↔serde 精确字段集互逆（876 教训：写出字段集必须与 TS 可读集精确互逆）、episode-store 行为对齐（enqueue/排序/restart 语义）、adopt 能力框架环的最小等价、assertDisplayText/validatedArtifactWrites/syncWorkSourceArtifacts 投影、skills documents 端点、/works 排序 tiebreak。

## 四、契约与测试策略（876/874 纪律延伸）

①金标准行双端锁（work.json 双版本 artifact 不变量 / episode+events / profile——Rust 单测与 TS vitest 逐字共用，程序化核验）；②契约差分 works 9 条从「声明确认」升级「活体对比」（双腿同源夹具——walkthrough-fixture §2.9 seedWorkEntity 已是 core-API 播种双腿通用）；③**双引擎交替读写互证**（Node 写→Rust 读→Rust adopt→Node 读，876 盲区补法）；④变异三类（checksum 不符/currentRevisionId 漂移/未知字段）。

## 五、分解与轮次规划

| 层 | 内容 | 规模 | 验收 |
|---|---|---|---|
| M1a | contracts serde + work-store + artifact-revisions + episode-store(rusqlite) + 金标准锁 | ≈900-1200 行+测试 | 金标准行双端绿；双引擎读写互证脚本 |
| M1b | GET 端点全域 + adopt（最小 episode/receipt 环）+ skills documents 端点 + 契约差分升级 | ≈600-800 | WorkInspector/Dashboard works 区 Rust 引擎活体；差分 works 域活体对比 |
| M2 | profiles POST/PUT + 创作配置保存面 | ≈300 | CreativeMethodsEditor Rust 引擎读写活体 |
| M3 | short-fiction-runner 移植（work 创建+episode 产生主力；需前置依赖勘察轮） | 最大 | Rust 引擎短篇创作端到端产出 |
| M4 | harness 工具进 agent_loop | 大 | 创作对话内使用 harness 工具 |

**下一实施轮 = M1a**。

## 六、风险与约束

①双引擎交替读写（876）：serde 写出字段集精确互逆是硬门禁；②assertDisplayText 要求 upsert 节点全字段（878 实证）；③episode-store 双实现对齐（enqueue 串行/事务/排序）；④/works 排序 tiebreak；⑤M3 的 LLM 管线依赖需前置勘察；⑥桌面端到端价值时序在 M3（M1 价值=双引擎用户与差分活体化）。
