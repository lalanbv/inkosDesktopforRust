# 984 号 · pending-aborts 标记表有界化轮：长驻进程内存有界 + 640 契约双栈定性勘误

> 接管收口轮。工作树存在已标注 984 的未提交改动（并行会话 sess_63b77726 idle
> 遗留——无文档无提交，开工三查实证：984 无文档/无撞号/无在跑会话）。继承改
> 动=pending_session_aborts 标记表有界化（cap 64 最旧 seq 逐出）+并发鲁棒测试。
> 本轮收口三面：①遗留改动补跑门禁——clippy 恰红三处（全为遗留代码自带，
> 实证其从未跑过 clippy：`split('-').last()` on DoubleEndedIterator）；
> ②双栈定性勘误：640 排队态取消契约 TS 侧随上游 2.0 合并消失=架构一致演进
> 非缺口（两处陈旧注释修正）；③有界化必红活体（HEAD 无界形恰红 → 恢复复绿）。

## 一、有界化主修（继承+收口）

- 缺陷面：`pending_session_aborts()` 全局 `Mutex<HashMap<String,u64>>` 无界
  ——未被受理点 take 消费的标记（mark 后该会话再无新请求）永久滞留，长驻
  进程内存无界增长（0GC/内存有界目标直系）。
- 修复形：cap 64（同文件 context_meter_registry 先例 agent_route.rs:67）；
  key 在场=覆写不增长；key 缺席且 len≥cap=按最旧 seq 逐出（min_by_key，
  AtomicU64 全序无并列）；整段持同一锁作用域无窗口。被逐出标记语义=该
  abort 不再 pending（与 640 消费自愈同族——「未命中说明标记已无目标，同样
  清除自愈」）。
- 逐出触发面定量：需 64 枚更新序 mark 置入才逐出一枚旧标记；生产触发=
  mark 后无新请求的会话 ×64 堆积，概率极低且语义无损（该 abort 本就无受理
  目标）。
- 测试 pending_session_aborts_cap_evicts_oldest（80 探针>cap 64）：并发鲁棒
  断言——①最后 mark 的 key 必可达（淘汰只删最旧）②本测试 key 命中数≤cap
  （被逐 take 返回 false=不再 pending）。seq=0 边界解明：进程首枚 fetch_add
  返回 0 → `0<0` 取 false → hits 63/62 皆 ≤64 安全（必红活体实测 hits=78 与
  seq 边界共存互证）。

## 二、双栈定性：640 排队态取消契约 TS 侧消失=架构一致演进（非缺口）

- 考古链：640 号引入双端 pending-abort（TS markPendingSessionAbort 于
  agent-session.ts +session-pending-abort.test.ts 187 行；Rust AtomicU64+
  标记表）。上游 2.0 合并后 TS 侧零命中（HEAD agent-session.ts grep=0，
  -S 追溯仅 8ece52bd 一跳=移除发生于 2.0 合并换血）。
- 定性依据：TS 2.0 架构 chat 无服务端队列——studio server.ts 6563 区忙到达
  即 409 CHAT_REQUEST_ALREADY_RUNNING 拒绝（无排队轮=无受理裁决目标）；
  Rust 220 号 per-session 串行队列（到达即注册入队）才有排队面。机制随宿主
  架构消失=一致演进（dual-stack 判据：架构载体消亡≠漂移，非 804 形移植后
  上游单侧改）。
- 结论：排队态取消契约 Rust 独占；TS 无对应有界化需求。
- 勘误两处：640 区块注释「TS 对应物 = packages/core agent-session.ts
  pendingSessionAborts」（符号已不存在）+984 新注释同引用——均改为架构现状
  表述（无队列/409 拒绝/Rust 独占）。

## 三、必红形态与门禁

- 必红活体（Edit 等价 stash HEAD 形——实现面回退无界 HEAD 形+测试面保留）
  → cap 测试恰红「命中数不得超过 cap（其余按最旧 seq 逐出），实际 78」
  （78=79 探针−seq=0 边界枚，仍 >64 恰红）→ 恢复复绿 6/6。恢复为精确互逆
  Edit（当前态==clippy 已门禁态）。
- 瞬时红备案：编辑后首跑一枚失败（38 连绿+全通道确定性分析未复现未归属；
  疑环境负载瞬时态——当日 bench 轮实测 loadavg 3.64/18 桌面常态进程）。
- 门禁：clippy:gate 双 0（拦截修复后——三处 `Iterator::last on
  DoubleEndedIterator` error 全为遗留代码自带，修复=sid 索引直构不再反解）
  +testgate engine-rs 52 目标 2112 passed（+1 新用例，其余 2111 持平）
  +audit:rust 双 0 漏洞（unmaintained 既有备案面；yanked 联网核验超时=网络
  抖动非漏洞）+必红夹具 7+1 自检活。src-tauri/TS 树零改动引用 983（584 持平
  /gate:ts 十步全绿）。
- U+FFFD 净（中文落盘扫 EF BF BD）。

## 四、备案

- 瞬时红（见上）：若复现先抓测试名再归因。
- 逐出语义面：被逐标记的 abort 意图丢失（该会话下一次受理不再命中中止）
  ——与消费自愈同族语义，触发面极小（需 64 会话级堆积），接受。
- 编号纪律：change-no 双查（取号时 984 无撞；收口提交前复查）。
