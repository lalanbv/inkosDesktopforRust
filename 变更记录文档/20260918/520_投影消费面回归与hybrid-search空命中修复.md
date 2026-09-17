# 520 号：投影消费面回归复验——Rust 管线索引改磁盘 FTS，hybrid-search 空命中缺陷修复

日期：2026-09-18　分支：develop　基线：67c15a56（519 号）

## 背景

519 号修复 write-next 投影时序后，双引擎 sqlite 消费面（promises/currentChapter）已一致，但**投影与检索消费端点从未系统性纳入契约差分器**。本号盘点 MemoryDB/检索消费端点，把缺口补进差分器并活体复验。

## 差分器扩展（38→40 端点）

- `GET /books/:id/quality-debts`（G3/337，quality_debts 表消费端）——入 GET 清单，双端即刻一致。
- `POST /books/:id/hybrid-search`（G1/349 混合召回，FTS5 消费端）——新增 POST 对照。契约面 = `mode` + top 命中 + 双端交集（排序无关）；BM25 打分/次序随双端分词实现波动（lens rank 同型），各自边缘命中输出信息行备案。

## 抓获并修复的真缺陷：Rust 磁盘 FTS 索引永远为空

差分器首跑即坐实：同 query「苏檀 碎镜」node 命中 8 条（fact:1 领头）、**rust 全空**。

根因：`retrieve_memory_selection`（写作管线检索）Rust 侧用 `LocalSearchIndex::new(":memory:")` 临时索引——每次检索临时构建即弃，**磁盘 `story/memory.db` 的 FTS 表永远无人写入**；而 hybrid-search/检索面板读的正是磁盘表。TS 侧 `retrieveMemorySelection` 用磁盘索引（顺带重建，content_hash 去重），双端语义在此分叉。

修复（`engine-rs/src/utils/memory_retrieval.rs`）：管线检索改用磁盘路径 `story/memory.db`，磁盘打开失败降级内存索引保检索不中断。注释写明对齐依据（520 号）。

**连带修复**（`engine-rs/src/server/ops_routes.rs`）：Rust hybrid-search 响应的 fts 元素含 `rank` 字段而 TS 契约形状为 `{id, score, source, title}`（rank 是 TS 端 ftsRanked 内部产物不入响应）——去除之；`fts_for_rrf` 的 RRF 融合改用 enumerate 重建名次（原从 JSON 回读 rank，去除后会静默变 0）。

**边缘抖动定性**：修复后双端共同命中 7 条完全一致、top 一致（fact:1）；仅边缘 1 条不同（node 多 summary:2、rust 多 hook:H01，k*2 截断边界）——BM25 分词/打分实现差异，非投影缺陷，差分器按交集契约 + 信息行备案。

## 验证

- 差分器 **40 端点 0 分歧**（hybrid-search 交集契约绿 + 边缘命中备案输出）。
- engine-rs 全量 cargo test **1803 绿**（memory_retrieval/ops_routes 改动后复跑）；`pnpm clippy:gate` 双 crate 0 告警。
- node-fallback-smoke 双引擎 13/13 绿；export-epub-smoke 双引擎绿；`gate:ts:fast` 全绿。
- `pnpm bench:gate` 9 基准零回退（-3%~-13.5%；负载守卫两轮拦截后静候回落）。
- TS 侧零改动。

## 关联

- 519（投影时序——本号为同主题消费面复验延伸）、518（差分器扩展方法）、406（投影段）、349/412（hybrid-search 实现与补注册）、337（quality-debts）、488（差分器端点扩展惯例）。
- 后续候选：BM25 分词器对齐专项（边缘命中抖动的根治面，独立工作量）。
