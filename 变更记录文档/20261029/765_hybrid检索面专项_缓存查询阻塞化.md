# 765 号：hybrid 检索面专项——缓存查询阻塞化清偿（764 号另册项收口）

> 目标线：高性能 0GC。764 号档案第三节"缓存类另册"落地：定性
> `Mutex<MemoryDb>` 长寿缓存的检索调用形态，按形态清偿。

## 一、定性（三处调用面）

| 调用面 | 形态 | 定性 |
|---|---|---|
| `hybrid_memory_selector::run_selection`（经 composer:1466 / memory_retrieval:728 await 进入，每章节步一次） | ①`list_chunk_vectors()` **全表读**进 HashMap（向量随章节累积，1024 维×8KB/条，数百章即 MB 级反序列化）；②逐条 `upsert_chunk_vector()`（N 次短锁+写） | runtime 线程最大残留阻塞面 |
| `write_next` prepare 构造点（:1812） | `MemoryDb::open(book_dir)` 直跑 runtime 线程 | open 廉价但属同类，顺手清偿 |
| `post_hybrid_search` fts 段 | `LocalSearchIndex`（独立 rusqlite 连接）同步 `new`（含 migrate）+ FTS5 查询 | 同类 sqlite 直写 |

## 二、清偿形态（关键改造一处结构 + 三处搬移）

1. **`HybridMemorySelector.cache` 改 `Arc<Mutex<MemoryDb>>`**：原借用形态
   无法进 spawn_blocking（需 'static）；Arc 后 Connection 全生命周期限于
   阻塞线程。`new` 签名不变（内部包 Arc），生产唯一构造点 + 测试零改动。
2. **全表读** → spawn_blocking 返回 owned HashMap（JoinError/查询错误统一
   String 化，走既有 `?` 链）。
3. **upsert 批量化**：新嵌向量先物化 `Vec<StoredChunkVector>`，单次阻塞段
   按原顺序写完；任一写失败整体失败（与逐条 `?` 等价），347 号降级契约
   （→空精选回退 BM25）兜底不变。N 次锁+写 → 1 次锁+1 次阻塞跳。
4. **write_next 构造点**：open 移入 spawn_blocking，`None` 臂（缓存库不可
   开→维持 LLM 精选）语义逐字保留。
5. **fts 段**：index new+search 移入 spawn_blocking（search 无失败态，仅
   连接/migrate 可失败），错误语义与原 match 一致。

## 三、验证

- `cargo test --lib` **1504 passed**（含 hybrid selector 全部测试——
  `#[tokio::test]` current_thread 下 spawn_blocking 照常工作，实证阻塞池
  方案对测试运行时形态无侵入）；全目标 testgate：engine-rs **1938** +
  src-tauri **583** passed。
- clippy 双 crate **0 告警**；gate:ts 八步全绿（rust-bin 指纹重建；契约
  差分 0 分歧）。
- bench 面：hybrid 检索不在 21 基准组内（依赖外部 embedding 服务，无
  离线基准），基线无需重锚。

## 四、runtime 线程阻塞面收口状态

763 号（文件 IO 27 站点）+ 764 号（MemoryDb 10 站点）+ 本号（缓存/FTS 3
面）后，async 宿主内的同步 IO/sqlite 直访已知面清零；残留同类面仅
`LocalSearchIndex` 其余消费方（grep_walk 工具路径）与 hybrid 检索的
后续形态演进——均已在台账记录，无隐性阻塞面存量。
