# 985 号 · 进程内表族扫有界化轮（TS 镜像）：chatRequests 终态逐出 + bookCreateStatus cap + 惯用形收敛

> 984 同族双栈镜像轮（goal 0GC/内存有界直系）：双端「无界进程内表」族扫
> ——Rust 侧定性零改动（LOCKS 族键空间=磁盘资源有效有界；两注册表任务终
> 态双移除；context_meter/pending aborts 已 cap）；TS 侧两处真无界清偿 +
> 通用有界插入惯用形收敛（bounded-map 独立模块+四例锁形）。

## 一、族扫定性（双端）

- Rust 零改动定性：①LOCKS 族 4 站键空间均为磁盘资源——write_next
  book_locks（book_id）、task_store/backup LOCKS（任务/备份文件 PathBuf）、
  interactive_film project_locks（project_root::project_id）——有效有界；
  ②agent_production 两注册表（reserved_production_sessions/
  active_confirmed_tasks）任务终态双移除（agent_production.rs 2762-2765，
  result 返回前无 await 窗口）→ 有界；③context_meter cap64（713 号）+
  pending aborts cap64（984 号）既有；SESSION_QUEUE_SEQ 原子计数非表。
- TS 有界面定性（既有，零改动）：subscribers（SSE 关闭删）、
  agentSessionQueues（队列 settle 自排，agent-session.ts 452）、agentCache
  （TTL 5min/60s 扫描+空表停表）、activeConfirmedTasks/
  reservedProductionSessions（任务终态删）、deletedSessionIds（6433 消费
  自排水墓碑）、modelListCache（键=service 配置数，有界）。
- TS 真无界两处：①`chatRequests`——终态条目永久滞留（middleware finally
  只更新快照不删除，每会话一条=长驻进程无界，984 Rust 同族镜像）；
  ②`bookCreateStatus`——error 终态无删除路径（仅成功 delete，失败创建的
  distinct bookId 条目永久滞留）。

## 二、修复（TS 有界化）

1. **bounded-map.ts**（新）：通用 `insertWithOldestEviction`（delete+超 cap
   按插入序逐最旧+set；覆写已有键不增长且重置插入序=被覆写键变最新在尾）
   ——与引擎侧 context_meter_registry（713）/pending aborts（984）逐出语义
   同族，注释互指。**bounded-map.test.ts** 四例锁形：cap 内不逐出/超 cap 逐
   最旧/覆写保鲜（重置序后旧另键被逐）/cap=1 只留最新。
2. **bookCreateStatus**：`setBookCreateStatus` wrapper（cap 64）——14 调用
   点全量统一（初勘 grep head 截断漏数，全量替换实证 14 站）。error 条目逐
   出安全性=读面磁盘兜底既有设计（create-status 端点 404 歧义消解走
   isBookFoundationComplete，注释明证「server restart can also drop it」）。
3. **contextMeterRegistry**：内联逐出收敛到 helper（语义与 713 原形逐字
   等价——delete+超 cap 逐最旧+set；同文件双轨统一=防漏网信号）。
4. **chatRequests**：middleware finally 终态守卫删除（save+broadcast 后
   `chatRequests.get(sessionId)===request` 才 delete）——live=在飞 only
   语义；守卫防 retry 校验失败恢复形（6587 `chatRequests.set(sessionId,
   previous)` 后 finally 不误删 previous）。全部读面逐点核验兜底：
   loadChatRequest 先 live 后磁盘双检+I/O 后复检（2499-2505）、retry
   previous 走 `previous?.snapshot ?? store.load`（6578）、abort 路由
   running 守卫（6502-6504）、branch busy 在飞判（6526）、6693 区运行中
   活体更新（早于 finally 无窗口）。

## 三、必红与门禁

- bounded-map.test.ts 必红自证形：暂定错断言 `size===3` → 恰红「expected
  2 to be 3」（逐出真实发生锤定，非恒绿）→ 改正复绿 4/4。
- chatRequests 终态删除=语义等价面（全部读面兜底——外部无观测差异，契约
  同形客户端无感，640 教训正向应用）：必红由 helper 单测承担 + 门禁活体
  （studio 839 全绿，server.test.ts retry/flow 真实走 finally 删除路径）。
- 门禁：gate:ts 十步全绿（build 16.0s/typecheck 14.8s/test 101.4s 含
  bounded-map 4 新例+studio 839/audit:npm 3.3s/rust-bin 15.7s 活体二进制
  同源/node-fallback-smoke 35.9s/engine-contract-diff 38.7s/
  export-epub-smoke 35.5s/work-alternation 5.2s/check-routes 0.1s）；
  engine-rs/src-tauri 树零改动引用 984（clippy 双 0/testgate 2112/audit
  双 0）。U+FFFD 净。

## 四、备案

- chatRequests 终态删除的语义等价性以读面逐点核验替代外部观测（无外部可
  观测差异=客户端无感）。
- bookCreateStatus cap 64：逐出后读面走磁盘兜底（404 歧义消解既有设计）。
- modelListCache 的 `at` 时间戳未做 TTL 逐出——键空间=service 配置数有界，
  定性保留（如未来键空间扩展再议）。
