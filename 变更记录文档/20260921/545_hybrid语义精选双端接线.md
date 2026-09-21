# 545 号：hybrid 语义精选双端接线——写作链检索向量重排 opt-in 激活

日期：2026-09-21
类型：feat(engine)+fix(studio)——537 号立案专项②清偿（接线方向，双端同步补齐）
提交：本文件同批路径限定提交

## 裁决：接线而非退役（考古链）

537 号把 hybrid 语义检索定性为「双端同构已建成未接线」。本轮考古推翻"退役"选项：

1. **非废弃实验，是未竟功能**（530 号"两端都断=上游未竟意图"先例）：G1 三件套
   全部就位——原语双端 golden 锁（`semantic-retrieval-vectors.json`）、
   embedding 双协议客户端（TS `createEmbeddingClient` 完整实现；Rust
   `embed_batch` 已被 recall 端点活用）、指纹缓存表（`retrieval_chunks`
   双端同构）、selector 注入口双端都在（TS runner.ts memorySemanticSelector
   组装位 / Rust write_next.rs:1809 `memory_semantic_selector`）。
2. **recall 测试端点语义分支双端都是活的**（请求体带 embedding 配置即走向量
   +RRF 融合）——"恒空降级"的根源只在写作链写面未接，读面是活契约。
3. **降级契约完备**（347 号）：embedding 任何失败 → 空精选 → 检索链回退
   BM25 排序；配置缺省 → 行为零变更。opt-in 接线无默认行为风险。
4. 缺的只有两块：配置面（`llm.embedding` 在双端 schema 均不存在——TS
   embedding-client.ts 注释提到的 `llm.embedding` 是悬空设计）与最后接线。

## 改动

### TS（真源端）

- `packages/core/src/models/project.ts`：`LLMConfigSchema` 新增
  `embedding: EmbeddingConfigSchema.optional()`（G1/348 canonical schema
  挂接；embedding-client 运行时仅依赖 zod，models←retrieval 无运行时环）。
- `packages/core/src/pipeline/runner.ts`：`createGovernedArtifacts` 组装位
  条件注入——`createEmbeddingClient(defaultLLMConfig.embedding)` 合法时
  `createHybridMemorySelector({ client, cache })`（cache=MemoryDB(bookDir)，
  打不开仅失去缓存 try/catch）；缺省维持 `composer.selectMemoryCandidates`
  LLM 精选。原 11 行死 import 转活。
- `packages/core/src/__tests__/hybrid-memory-selector.test.ts`：+2 配置面
  验收（LLMConfig 接受 embedding 节并产出客户端 / 缺省非法→null 降级）。

### TS 潜伏缺陷修复（接线考古牵出）

- `packages/studio/src/api/server.ts` recall 端点：`new MemoryDB(dbPath)`
  误传 **db 文件路径**（构造参数是书目录，内部拼 `story/memory.db`）——
  实际打开 `.../story/memory.db/story/memory.db` 嵌套假库，语义分支恒读空。
  改传 `join(root, "books", id)`。Rust 侧 `MemoryDb::open(book_dir)` 一直
  正确。写面激活后此分叉必然显形，本轮一并清偿。

### Rust（对称接线）

- `engine-rs/src/utils/hybrid_memory_selector.rs`（新模块，镜像 TS
  hybrid-memory-selector.ts 语义）：`HybridMemorySelector` 实现
  `MemorySemanticSelector`——query 嵌入 → `list_chunk_vectors` 指纹缓存 →
  `select_stale_chunks` 增量嵌入 → `upsert_chunk_vector` 写回（写面首次
  激活）→ `top_k_by_similarity` 精选；任何失败 `Ok(vec![])`（降级契约）。
  `EmbedFn` 端口拆分（生产 `ProductionEmbedder`=embed_batch+装配处
  apiKeyEnv 解析；测试 mock）。cache 包 `Mutex<MemoryDb>`（rusqlite
  Connection !Sync，锁内均为同步调用无跨 await 持锁）。
- `engine-rs/src/pipeline/write_next.rs`：`WriteNextConfig.embedding`
  （`from_project` 读 `llm.embedding`，非法整节 None）+ 注入位条件化
  （配置存在且缓存库可开 → hybrid，否则 LLM 精选）。
- `engine-rs/src/state/memory_db.rs` / `models/quality_governance.rs`：
  537 号备案注释清偿更新（upsert 写面已消费；count 仍为对称 API 面备案；
  creates_debt/continues_pipeline 已由 540 号消费）。
- 测试 +4：hybrid 三场景（镜像 TS：增量嵌入分批+余弦 topN+二轮仅嵌 query /
  失败降级空 / 空候选短路）+ from_project 解析（合法/非法）。

## 行为面

- **缺省（无 llm.embedding）零行为变更**——全部既有测试/套件原样绿。
- 配置后：写作链记忆精选由 LLM 精选切换向量重排（opt-in）；retrieval_chunks
  写面激活，recall 端点语义分支吃到真数据（TS 侧经本轮假库修复）。
- HTTP 契约/差分器覆盖面零变更（内部行为，非端点面）。

## 门禁（全绿）

1. engine-rs cargo test 全量 **1815**（1811+4）exit=0（落盘统计）
2. clippy:gate 双 crate 0 告警
3. verify:engine-bindings **186** 导出全绿
4. duel 真跑（INKOS_DUEL=1）**10/10** @41.43s
5. gate:ts 七步：typecheck/audit:npm/build/双腿 smoke/contract-diff/EPUB 冒烟
   ✓；`pnpm -r test` 门禁内首跑 946.9s 失败=531 号已知高负载 worker 停滞
   （正常 ~50s 的 19 倍），空闲单跑复验 **3238** 全绿（core 2126+studio
   894+cli 218）
6. bench:gate 豁免备案：检索精选非 bench 热路径且缺省零行为变更

## 编号事故（同日四撞，警戒升级）

本号开编时为 541；写档前三查发现并行会话已连续落 541（8e6af1b7——该会话
发现其 540 与本会话 11760bc0 撞号后自改 541）/542/543/544 四笔规划文档，
本号全程改用 **545**（13 处代码注释同步改号）。教训：**并行会话密集产出期
编号窗口极短**——开号三查到收口之间数十分钟内并行会话可连落四号；三查
必须在写档前+收口前最后一刻执行，且归档文档应尽早在开号当日落盘占号。

## 教训

1. **"零引用"裁决前先核读面与配置面**：hybrid selector 若按 537 号表
   面定性走退役，将永久封死一个基建完备度 90% 的产品能力；读面活体
   （recall 端点）+ 悬空配置注释是"未竟意图"的两个关键信号。
2. **依赖注入式组件的"未接线" ≠ 死代码**——它是库的扩展点，接线的
   义务在应用侧配置面，不在组件本身。
3. rusqlite `Connection` !Sync：持库字段的 async trait 对象需 `Mutex`
   包装（锁内只做同步调用，避免跨 await 持锁破坏 Send）。
4. **MemoryDB 构造参数是书目录**——recall 端点传文件路径的嵌套假库
   bug 潜伏至今的根源：表恒空使错库与空表不可区分；写面激活前修
   数据源，是"接线序"的正确姿势。

## 编号警戒（更新）

545 已用（本会话）；541–544 并行会话占用。**下一号自 546 起**。
