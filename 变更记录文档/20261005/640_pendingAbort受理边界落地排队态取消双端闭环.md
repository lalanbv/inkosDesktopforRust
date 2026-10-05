# 640 号：pending-abort 受理边界落地——排队态取消双端闭环（636 走查备案清偿）

日期：2026-10-05
提交：本次（develop）
前置：636 号备案「backlog 冻结（POST 已发未受理）时点停止，解冻后回复诚实落地 transcript committed；收敛方向=受理时查 pending-abort 标记另号立项」

## ①缺陷语义与定性

引擎 abort 契约只覆盖「注册表在飞轮」：TS `abortAgentSession` 只触达 agentCache（排队轮尚未装配 Agent，无条目）；Rust 排队轮在受理点新建 abort_flag，此前置位被冲掉。双端对「已入队未受理」请求的停止都是 no-op——用户在 backlog 冻结期点停止，解冻后陈旧指令照常执行并 committed 落盘，用户看到一条自己明确取消过的回复（636 号 T4 走查实录）。

## ②设计（契约同形原则）

**单调序数裁决**：全局单调计数器（TS 模块级计数器 / Rust `AtomicU64`）——请求入队前捕获序号；abort 路由在会话忙（TS 队列 map 非空 / Rust registry 到达即注册）时置标记（记当前序数）；受理点（TS `runAgentSessionUnlocked` 任何重装配之前 / Rust 拿到 queue guard 刷新 handle 之后）取标记裁决：**入队序 < 标记序 → 受理即中止**。
- 为什么不用毫秒时间戳：同一毫秒内「先入队后置标记」（应取消）与「先置标记后入队」（换向流新消息，不可取消）无法靠时钟区分；序数给出严格全序，两向都不误判。
- 消费自愈：受理点取标记即清除——FIFO 下第一个受理者是唯一可能早于标记的轮；未命中（标记后新入队）说明标记已无目标，同样清除。空闲误置的标记对后续新轮天然未命中，不误杀。

**受理即中止与在飞中止契约同形**：transcript 落 `request_started + user message + request_failed{"aborted"}` 三事件（在飞轮的 user 事件也已随事件流落盘）；响应 500 aborted（TS `errorMessage:"aborted"` → formatAgentFailure 同链；Rust `{code:AGENT_ERROR,message:"aborted",response:"aborted"}`）；Rust 另广播 `agent:error`（与在飞同形）。**客户端 624 `chatAbortedAt` 契约零改动**——排队轮被取消后客户端按既有中止语义丢弃痕迹、不记重试。

**置位条件**：abort 路由无条件「busy 即置」（在飞命中也置——保护已排队的下一轮；用户 stop 意图 = 取消 abort 时刻前已入队的全部未受理工作）。scope 无关（标记只管聊天队列；生产任务 TS 409 互斥无排队形态）。overflow 压缩重试递归不传入队序（新受理不裁）。

## ③实现与红绿可证伪

- **TS**（`agent-session.ts` + `server.ts`）：`pendingSessionAborts` map + `nextSessionQueueSeq` + 导出 `markPendingSessionAbort`/`takePendingSessionAbort`；`runAgentSession` 入队前捕获序（调用链同步段无插队窗口）；受理检查在技能/模型/Agent 装配之前短路（`acceptAbortedChatTurn` 零模型调用）；abort 路由 busy 即置位。单测 `session-pending-abort.test.ts` 3 断言（hold mock 占队列构造真实排队：排队轮受理即中止 transcript started+user+failed 无 committed；标记消费后新轮正常；空闲置位自愈）——stash 旧码红 3/3 → 绿 3/3。
- **Rust**（`agent_route.rs` + `session_routes.rs`）：`SESSION_QUEUE_SEQ` AtomicU64 + `pending_session_aborts` OnceLock map（键 `root\0sessionId` 与 TS 形态对齐）+ `mark/take`；`post_agent` 入队序在 `queue.lock_owned().await` 前捕获、受理检查在 handle 刷新后立即短路；abort 路由 registry contains_key 即置位。语义单测 4 断言（无标记/命中消费/未命中自清除/键隔离）+ **阻塞 LLM 全链 e2e**（axum mock LLM Notify hold：A 在飞 hold → B 排队（断言未到 LLM）→ abort → A 在飞中止 500 + B 受理即中止 500 + transcript started×2/user×2/failed×2/committed×0 → C 正常 committed）——stash 旧码 e2e 红在 B 断言（B 正常 200）→ 恢复绿。
- **门禁白名单**：server.test.ts core mock 补 `markPendingSessionAbort` 登记（TESTING.md「mock 白名单须显式登记新导出」惯例，445 号教训复验）。

## ④插曲两例（测试构造学）

1. **e2e 首跑 A 轮 200 假绿**：abort 后立即 release mock LLM，`tokio::select!` 任一分支 ready 即完成——chat 分支几 ms 内 ready 抢先于 abort_waiter 的 50ms 轮询窗口，A 正常完成。修复：abort 与 release 之间留 120ms（>2 个轮询周期）→ 中止分支确定性命中。**时序敏感断言必须给异步轮询留足窗口**（627 号 duel flake 同族：竞态确定性靠构造而非重试）。
2. **测试 config 顶层字面量捕获 undefined**：`const config = { projectRoot, ... }` 在模块顶层求值时 mkdtemp 尚未执行——对象字面量捕获当下值而非引用，3 用例连锁超时/TypeError。修复：beforeEach 重建 config。

## ⑤门禁矩阵（本号 HEAD 全量实跑）

clippy 双 0 + gate:ts 七步全绿（build 16.6/typecheck 16.0/test 74.8/audit:npm 2.1/smoke 34.1/差分 40.1 61 端点 0 分歧/epub 34.1）+ cargo:testgate engine 45 目标 **1926 passed(+5)** + tauri 584 + audit:rust 双 0 + duel 真跑 10/10（41.4s）+ bench:gate 首跑负载闸 115%/核拦截（testgate 余热）候谷 2.44/18=13.5% 通过零回退。

## ⑥备案

- write-next 受理窗口（TS `activeWriteNextTasks` 注册前）不在本号范围：write-next 管线不消费 AbortController 信号（2640 注释既有备案），其取消语义是独立专项。
- 双端 abort 契约既有差异（scope 载体 query vs body、广播 scope 字段、write-next 可停性）为 640 号前既有事实，本号未改动、未扩大；受理中止各自与各自在飞中止同形。
- 标记键形：TS `root\0sid`、Rust `root.display()\0sid`——各自进程内一致即可，不跨端持久。

## ⑦教训

1. **取消语义的受理边界 = 队列的隐含契约**：串行队列把「执行中」的语义边界从 HTTP 到达点移到了出队点，凡是按「注册表在飞」建模的取消/查询，都必须回答「排队中的算不算」——636 走查发现、本号闭环，中间隔了两个专项，说明走查备案的收敛方向要有编号才不会漂。
2. **契约同形是客户端零改型的前提**：受理即中止完全复刻在飞中止的 transcript 三事件 + 响应形状 + 广播，624 客户端契约（chatAbortedAt 识别）原样生效——新状态路径的设计目标是「让既有消费方无感」，而非新增一套状态。
3. **时序敏感的取消测试要建模轮询周期**：select/轮询类中止机制的测试，事件间隔必须大于轮询周期，否则竞态分支出场概率变成掷硬币（首跑 200 假绿正是硬币反面）。
4. **白名单 mock 的引擎侧新导出 = 上游破坏面**：server.test.ts 的 core mock 是白名单制，core 每加导出都要登记——这条已是第三次踩（445 备案在 TESTING.md），考虑后续把 mock 改为 importOriginal spread + 显式 override 制，一次根治。
