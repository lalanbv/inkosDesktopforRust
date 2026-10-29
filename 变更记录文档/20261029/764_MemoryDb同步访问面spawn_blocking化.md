# 764 号：MemoryDb（rusqlite）同步访问面 spawn_blocking 化——runtime 线程最大阻塞面清偿

> 目标线：高性能 0GC。763 号档案「新发现待办」落地：`MemoryDb` 包
> rusqlite `Connection`（Send/!Sync），此前在多个 async handler 的同步
> 闭包/直写段内 open+查询——summaries 全量读（上限 10 万行）与批量
> replace 直接阻塞 runtime worker。

## 一、定性（11 个 async 宿主 `MemoryDb::open` 站点全量清点）

| 类 | 站点 | 处置 |
|---|---|---|
| ops 只读投影 | get_roster_candidates / get_promises / get_quality_trend / get_quality_debts / post_hybrid_search | **spawn_blocking 化**（5 处） |
| ops 小写 | handle_audit_failure（record_debt 单点） | spawn_blocking 化 |
| pipeline 内 | write_next validate ×2（review-metric 幂等写 / seed summaries+hooks 批量回写）、prepare_write_input 的 hybrid 缓存构造 | 前 2 处 spawn_blocking 化；**第 3 处归缓存类**（见三） |
| 工具层 | film_authoring tool_upsert_characters（事实写） | spawn_blocking 化 |
| 检索层 | memory_retrieval 空库回填（replace_summaries/facts） | spawn_blocking 化（结构重排，见二） |

## 二、清偿形态（10 处）

- **闭包整体搬运**（roster/promises/write_next×2/film/audit-failure）：
  owned 捕获（book_dir/数据 clone），`unwrap_or_else(|join| Err(Constraint))`
  把 JoinError 并入既有失败语义——audit/写管线的 warn 日志、工具结果路径
  行为逐字不变。
- **get_quality_trend 结构重排**：原 async 块中段 await `load_book_config`
  （与 db 结果无依赖）——先 await 配置，db 查询+全部纯投影整体进
  spawn_blocking。
- **memory_retrieval 结构重排**：回填数据源与下方选取投影**同源**
  （structured rows 或 markdown 解析，250 行原式）——summaries 计算上移
  物化一次，克隆给阻塞闭包做回填；选取路径用原值（读内存投影，与回填
  写互不影响）；JoinError=回填跳过（DB 可重建，下次检索重试）。
- **get_quality_debts / post_hybrid_search**：open+单查询直写段搬入
  spawn_blocking（status 过滤器转 owned String）。

## 三、缓存类另册（不属闭包类，不动）

- `hybrid_memory_selector` 的 `Mutex<MemoryDb>` 长寿缓存及其构造点
  （write_next:1812）：open 本身廉价，重面在后续检索调用形态（LLM/embed
  混合），标准化需先定检索调用面形状——留作 hybrid 检索面专项。
- `post_hybrid_search` 的 fts 段、`hybrid_memory_selector` 内部 db 访问
  （已在其自身同步方法内）随专项一并看。

## 四、Send 纪律补注

memory_retrieval 原「Send 纪律」注释（不得跨 await 持 Connection）语义
升级：现Connection 在 spawn_blocking 闭包内 open+close 全生命周期，
跨线程只传 owned 数据（PathBuf/StoredSummary/NewFact 均实现 Clone）。

## 五、验证

- `cargo test --lib` **1504 passed**；全目标 testgate：engine-rs 45 目标
  **1938 passed** + src-tauri 25 目标 583 passed（零新增测——既有路由/
  管线集成测试覆盖全部改动路径，含 roster/promises/quality/debts/
  hybrid-search/回填幂等）。
- clippy 双 crate **0 告警**；gate:ts 八步全绿（rust-bin 指纹重建；
  契约差分 0 分歧；node-fallback-smoke/export-epub-smoke 真实落盘路径）。
- bench 面：改动路径均不在 21 基准组内（handler 投影/管线回写），基线
  无需重锚。
