# 631 号：SQLite 并发面双端审计——Rust WAL 移植缺口修复 + TS busy_timeout 补齐

日期：2026-10-05。类型：fix(engine+core)。前置：630 号 pi 1.0 迁移缓行裁决后，软件分析驱动转向可靠性面。

## 一、审计缘起与初步假设（后被实测部分反转）

无 key 维持 pi 1.0 缓行（630 号），分析驱动开新增量：审计 memory 召回面（每轮 write_next 必经、未基准化）时发现并发写面问题——write_next 每轮双开 SQLite（MemoryDb 回填 + LocalSearchIndex 磁盘 FTS 重建 replace_scope 的 BEGIN IMMEDIATE），消费端 post_hybrid_search（检索面板）每次请求 LocalSearchIndex::new 的 migrate 即写事务、materials.rs 材料导入同构。初判「四打开点 busy_timeout 全缺」。

## 二、实证反转（诚实协议：静态推理让位产物实测）

红绿验证首轮出现矛盾（并发测试旧码应红却绿），分步计时探针实锤：

1. **rusqlite 0.32.1 在 `Connection::open` 内已默认 `sqlite3_busy_timeout(db, 5000)`**（inner_connection.rs:119）——Rust 侧四打开点从来就不是零等待。裸连接 `PRAGMA busy_timeout` 实测返回 5000；持锁窗口内 B 连接的 BEGIN IMMEDIATE 实测等待 ~5.4s 才 busy 失败（handler 行为特征）。「Rust 零 busy_timeout」前提错误。
2. **TS 侧 node:sqlite `DatabaseSync` 默认 busy_timeout=0**（本机实测）——真正的缺口在 TS：**Node 回退引擎与 CLI 跨进程同书并发写会瞬时 SQLITE_BUSY**（Node 单线程同步 SQLite 使进程内碰撞不可能，跨进程 [引擎+CLI 同时跑] 才是真实碰撞面）。
3. **Rust 侧真实缺陷=local_search 磁盘索引缺 `journal_mode=WAL`**——TS 构造函数已有而 Rust 缺失的移植缺口（520 号移植磁盘索引时未镜像 pragma 面）。后果：回滚日志模式读写互斥（检索面板读与 write_next 写互斥，WAL 下本可并行）+ 崩溃安全性与 TS 不一致。
4. memory_db.rs 的 WAL pragma 经探针实证工作正常（pragma_update 对 journal_mode 返回 Ok，模式落盘 wal）——初判「可能静默失败」被探针否定。

## 三、修复面（双端四文件）

1. **engine-rs/src/utils/local_search.rs**：磁盘索引补 `PRAGMA journal_mode = WAL`（:memory: 跳过）+ 显式 `busy_timeout(5000)`（契约声明，防 rusqlite 上游默认漂移；与 TS 同值）。
2. **engine-rs/src/state/memory_db.rs**：显式 `busy_timeout(5000)`（同上契约声明；WAL 已在）。
3. **packages/core/src/retrieval/local-search.ts**：`PRAGMA busy_timeout = 5000`（node:sqlite 默认 0 → 补齐，对齐 rusqlite 默认）。
4. **packages/core/src/state/memory-db.ts**：同上。

## 四、验证链（红绿可证伪 + 契约锁）

- **TS busy_timeout（本号主可证伪位）**：新测试 `sqlite-pragmas.test.ts` 注释修复行跑旧码 **2/2 红**（busy_timeout=0≠5000）→ 恢复 **2/2 绿**；同时锁 WAL=wal。
- **Rust WAL（第二可证伪位）**：`disk_index_runs_in_wal_mode` 注释 WAL 行跑旧码**红**（mode="delete"）→ 恢复**绿**（"wal"）；同测锁 busy_timeout=5000 契约值。
- **Rust 并发契约测试**：`concurrent_writer_waits_instead_of_failing_busy`——持锁窗口（BEGIN IMMEDIATE 300ms）内写者线程全链（new+migrate+replace_scope）不死、锁释放后完成。诚实备案：rusqlite 默认 5000 下旧码同样通过（不可作本号可证伪位），锁的是「等待不死」形态防上游默认漂移。
- **memory_db 配置断言**：`open_sets_busy_timeout` 磁盘/内存双路 busy_timeout=5000。
- **插曲备案**：①首轮并发测试旧码绿的反常=本号红绿验证的起点，分步计时探针（临时测试，已删）揭开 rusqlite 默认 5000；②测试 sed+mv 快速连续操作曾产生 cargo 增量编译陈旧产物伪象（WAL 断言恢复后仍红一次），强制重编译后消散——mtime 粒度伪象，红绿判读须以强制重编译为准；③MemoryDb::open/MemoryDB 不建 story/ 父目录（生产调用方保证），测试需先建目录（CannotOpen 非 pragma 问题）。

## 五、门禁

clippy 双 crate 0 告警；cargo:testgate engine 45 目标全绿 1902 passed（+3 新测试）+ src-tauri 584 绿；gate:ts 七步与 bench:gate 见收口补充（Spotlight 索引风暴 loadavg 136%/核两拦，按 625 号先例低谷重跑）。

## 六、教训

1. **「零配置=缺省行为」必须查库源码而非推理**：rusqlite 默认 5000ms busy_timeout 藏在 inner_connection.rs 一行——按「显式设置缺失=行为缺失」推理差点修了个不存在的缺陷（注释即已写错，实测探针纠正）。
2. **node:sqlite 与 rusqlite 默认值不同源**（0 vs 5000）——跨语言移植面「pragma 对齐」要把**库默认值**也算进契约；同构判断的基线是运行时实测而非源码字面。
3. **红绿验证的反常红是最高信息量信号**：预期红却绿→追出 rusqlite 默认值；测试基建的伪象（cargo 增量编译陈旧、CannotOpen≠目标断言）须逐一排除后才可下结论。
4. play 双端（play_graph.rs/play-db.ts）同缺 TS 侧 busy_timeout，交互式单用户+每 run 独立 db 风险低，**备案不扩**（同模式一行可补，留待证据驱动）。

（631号）
