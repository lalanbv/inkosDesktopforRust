# 1663 号：prompt-packs TS 端点族回座 + duel 8 红黑盒破解（688 守卫判据迁移）+ run-log 死面复座 + Node 腿会话探针结案 + RUSTSEC-2026-0594 清偿

> 日期：2026-11-23 起轮，11-24 收口 ｜ 类型：混合轮（R54 全线收官 + 两项 1662 备案定性勘误 + 供应链拦截 + 门禁流程勘误）
> 定位：1662 号 8 红黑盒**机理破解**——非 tsx sidecar 行为差，而是 **688 号豁免守卫语义**；同轮把 1662 备案的两项「口径差/适配缺口」实跑取证，**两项定性均不成立**，各自是可修真缺口，均已回座。

## ① 黑盒破解（取证 30 分钟收口）

- **取证 A**：HEAD 干净版手动复刻 duel 环境（duel-shaped root + sidecar）→ `/api/v1/books` 返回**空**——「TS 腿读 books 布局」假设本身不成立（TS=上游 2.0 works 语义）。
- **真相**：`duel_require_fork_contract!` 宏探 TS 腿 `/api/v1/prompt-packs`——HEAD TS **无该端点→404→判定「上游 2.0」→12 个 duel 测试全部响亮豁免（return 计 pass）**。duel「12/12 绿」=688 号 2.0 迁移欠账豁免的**常态**（非对跑真绿）。1662 给 TS 加 prompt-packs 路由→端点 200→**守卫意外判定「fork 契约」→12 个早已失效的对跑真执行**→TS(works 语义) vs Rust(books fixture)不匹配→8 红。
- 1662 成分矩阵全部吻合：「+import=绿」（不动路由不影响守卫）、「+任意路由=红」（`/prompt-packs` 200 复活守卫）、「位置无关」「空 handler 亦红」（stub 也返回 200）。

## ② duel 守卫判据迁移（根因修复）

- `ts_leg_contract_source` 探测判据 `/api/v1/prompt-packs` → **`/api/v1/health`**（62/170 号 Rust 超集端点=真 fork 独有面；与 1662 三脚本判据迁移一致）。
- TS 腿无 health → 守卫继续豁免（响亮记账，非静默）→ duel 12/12 绿且**不因业务端点同构化而误复活**。

## ③ 五端点族回座（server.ts 终态）

- GET/PUT/DELETE `/api/v1/prompt-packs` + GET `/:promptId/versions` + POST `.../versions/restore`（builtin 投影含 packId、错误码 PROMPT_PACK_PROMPT_NOT_FOUND 对齐 Rust、大小写归一、regressionHint）。
- **回环 origin 放行分支回偿**（W-A4a/173 号）：`loopbackHosts.has(originHostname)` 放行 vite dev 形态跨口 Origin；smoke node 腿 origin 断言通过。
- **PromptPacksPanel 降级解除**：Node 回退腿同构端点可达（unavailable 分支保留=纯网络失败兜底）；studio 全链测 10/10。
- 副作用（已在位语义，非改动）：smoke 700 号契约源探针按 `/prompt-packs` 200 判定→node 腿自动回到**严格计数**（软失败记账不再触发）。

## ④ run-log 死面破解与写入点复座（1662「口径差」定性勘误）

- **勘误**：1662 把 node 腿 run-log 断言显式跳过并备案为「深播口径差=1663 专项」；本轮初稿（未提交）又写成「2.0 迁移欠账族语义差」——**两次定性都未实测**。验证器点出弱验证项后实跑取证，结论完全不同。
- **真相**：403 号的唯一写入点在 `packages/core/src/llm/model-chain.ts`（两处 `globalRunLog.append`）。688 号上游合并（a962f480）退役 model-chain.ts，**写入点随之消失**；701 号回收 fork-only GET 面时只救回了端点。此后 Node 回退腿 `GET /api/v1/run-log` 恒返回 `{total:0,kept:0,entries:[]}`——Dashboard「运行遥测」永不出现。全仓 `globalRunLog` 只剩一读零写。
- **为何一个合并周期无人发现（回声室第五形态）**：端点在位掩盖了写入面失踪；fixture 对 TS 腿在 `runWriteChain("node")` 后 `process.exit(0)`（455 行），**§3.6 run-log 断言从未可达**；smoke 的 `check("[node] fixture 一致性套件（…/run-log）", true)` 标签却声称覆盖 run-log。
- **修复（写入点收敛到 2.0 两条 LLM 传输边界）**：
  - `utils/run-log.ts` 加单源记录器 `recordRunLogCall`（只记九个元数据键=v5 隐私红线；`tookOver` 仅成功且 `attemptIndex>0||round>1`，与 Rust 同口径）。
  - 非 agent 路径 `llm/provider.ts` `chatCompletion`：**每个重试轮一条**（round=attempt；新增 `agentLabel` 选项，缺省=轨迹角色 main/subagent/workflow，无作用域=`chat`；预先中止的轮次未上线不计）。调用点标签：`verify`/`service-probe`/`series-backfill`。
  - agent 路径 `agent/pi-stream.ts` `observeModelStream`：**每次传输尝试一条**；`attempt` 穿透 observe 层重试递归（流僵死/空响应/缺必需工具各成一条），`settled` 三路防重。失败类按**性质**划分（与 Rust `ChainFailure` 同口径）：可重试三类=transient，中止/其余=fatal。
- **测试抓到的实现缺陷**：首版把终态 `record(...)` 写在 `if (done||error)` 守卫**外**——`start` 后首个 `text_start` 即触发 `record(false,"fatal")` 并落定，**每次成功的 agent 调用都会记成一条致命失败**（失败用例只有 start+error 两事件，恰好不暴露）。`run-log-writer.test` 4 红逼出，收进守卫内后 15/15。
- **口径备案**：粒度=agent 循环可见的一次传输尝试；传输**内部** 429/5xx 退避不单列（Rust 逐次 `chat_once` 记；TS 流式路径的 pi-ai 内部重试在本层不可见，体现在耗时与终态上）。用户中止记 fatal（沿 403 原语义，Rust 同为两值枚举）。
- **锁定**：core `run-log-writer.test.ts` 15（记录器单源 1 + chatCompletion 7 + guardedPi* 7；真实回环 http 上游脚本化应答）；studio `run-log-route.test.ts` 1（投影形状/limit 截尾/no-store/写入点→端点同进程贯通）；smoke **node 腿 run-log 真实断言**（`total>0 && entries>0`，不再是跳过）。

## ⑤ Node 腿会话探针结案（1662「works 布局适配缺口」定性勘误）

- **勘误**：1662 备案「2.0 works 布局下 chat 会话 agent 驱动链适配缺口=1663 专项」。本轮初稿沿用「显性跳过 + 注释写 1663 专项」——未取证。实跑（取证开关解除跳过）后逐项取证，**与布局无关**，是两处可修缺口：
  1. **mock 夹具缺口**：2.0 会话 `streamFn` 恒 `toolChoice:"required"`（每轮必须以工具调用收尾，通用收尾工具=`finish_turn`），真实模型照系统提示调用；`walkthrough-mock` 无 `finish_turn` 分支，会话请求落 persona 文本支→`MODEL_REQUIRED_TOOL_MISSING`→`POST /agent` 非 200（transcript 实证 `request_failed`）。
  2. **契约面缺口**：TS `GET /sessions/:id` 的 `session` 与 `GET /sessions` 摘要**缺 `head`/`branchCount`**——636 号原契约（「derive 透出 head/branchCount」），692 号重施只回座了 branch 端点与 `/branches` 读面，这两个投影字段随 2.0 会话形状遗失（Rust `BookSession`/`BookSessionSummary` 一直在位）。studio 分支切换器消费的是 `/branches`，故非可见 UI 缺陷。
- **修复**：mock 加 `finish_turn` 分支（仅当工具表含 `finish_turn` 时命中，与 `submit_*` 结构化支互斥；Rust 腿 chat 不带 tools 不受影响）；core `session-branch.ts` 抽单源 `projectSessionTree`/`sessionTreeProjection`（branch 结果/分支点读面/derive·list 三处共用），server.ts derive 与 list 加法透出（不动上游 2.0 会话类型）。
- **smoke 收口**：撤销 node 腿跳过，**双腿恒跑全量会话探针**（node 腿 9/9）；顺带去掉跳过态下一枚用伪造 200 对象打出的 `✓ branch 探针建会话`（不可能失败的绿=回声室形态）。
- **锁定**：studio `session-branch-routes.test.ts` +1（分支前后 derive 与 list 的 head/branchCount 同步移动 + 2.0 形状字段不受影响）。
- **残留备案（无害，非待办）**：mock `finish_turn` 应答的 message 仅取字符串形态 user 文本（数组形态时为空串）；TS list 摘要投影=每会话一次额外 transcript 读（list 本就逐条全量 derive，成本同阶）。

## ⑥ 当日供应链拦截与门禁流程勘误

- **RUSTSEC-2026-0594**（`cookie` < 0.18.3：「fork 进程中私有 cookie nonce 可重复」，CVSS 6.5 medium）：advisory-db 当日新增，`cargo:testgate` 首步审计如实拦截（本轮此前那次绿跑在通告发布之前——门禁不是恒绿摆设）。依赖链 = tauri 2.12.7 / tauri-runtime / wry 0.57.3 → `cookie 0.18.2`。处置 = `src-tauri/Cargo.lock` 定点 `cargo update -p cookie --precise 0.18.3`（同 minor 补丁，仅版本 + 校验和两行；未动用户全局 cargo 配置）。engine-rs 不依赖该 crate。
- **clippy 门禁勘误（流程缺陷，跨 1656–1662）**：`pnpm clippy:gate`（`-D warnings` 双 crate，433 号起的正规门禁）在 engine-rs 红——1656 的 `golden_text_diff_diff.rs` 有未用导入 `TextDiffKind`，1660 的 `golden_usage_budget_diff.rs` 有死字段 `daily`/`monthly`。**1656–1662 各轮记录的「clippy 双 0」是拿不带 `-D warnings` 的命令得出的，对这两处不成立**（告警被计成「0 errors」）。处置：删未用导入；预算差分的断言宽度补齐到 `BudgetStatus` 契约**全六键**（原只断 level/ratio/confirmRequired 三键——`daily`/`monthly` 反序列化后从不比较、`serviceKey` 未核）——按「加强断言」而非「压警告」修。
- **tmpdir 红档现场**：smoke 只在绿档清根（1438 号），红档保留现场**没有保留期**——实测 36 根 21MB（11-22/11-23 调试期累积），本轮手工清理。保留期机制挂观察项（须区分 `--keep` 人工保留与红档，未做）。
- **取证期调试残留**：8911 端口调试 sidecar 跑了 20 小时未清（1662 号文档自己写过「spawn 调试后必须 pkill 清场」）——本轮收口第一步清场。

## 门禁记录

终态树串行重跑（cargo:testgate → gate:ts），提交态 = 门禁态：

- `cargo:testgate` 全绿（`CARGO_GATE_EXIT=0`）：audit 双 0（engine-rs 3 条 / src-tauri 9 条 unmaintained/yanked 警告；yanked 联网核验超时 = 网络抖动非漏洞）+ 必红自检 9+1 活着 + engine-rs **72 目标 2200 passed** + src-tauri **26 目标 600 passed**（engine-rs 腿强制 `INKOS_DUEL=1`；全量日志交叉核对 0 失败 0 panic）。
- `gate:ts` 十腿全绿（`GATE_EXIT=0`）：build 17.2s / typecheck 17.4s / test 111.5s / audit:npm / rust-bin / node-fallback-smoke 35.5s（node 腿会话探针 9/9 + run-log 真实断言，零跳过）/ engine-contract-diff 36.2s / export-epub-smoke 35.0s / work-alternation / check-routes。
- `pnpm clippy:gate`（`-D warnings`，双 crate，`--all-targets`）0 告警；`cargo fmt --all -- --check` 双 crate 过。
- U+FFFD 净（全部触碰文件 + 记忆文件）。
- 测试增量：core +1 文件 15 测；studio +1 文件 1 测、session-branch +1 用例；Rust 测试数不变（预算差分断言由三键加强到六键）。
- 过程留痕（非终态）：本轮第一次 cargo:testgate 绿跑早于 RUSTSEC-2026-0594 发布且调试 sidecar 仍在场——不计入结论；第一次 `clippy:gate` 红（⑥）已修。

## 出口

- **R54 全线收官**（引擎面 1658+UI 1659+数据单源 1662+Node 腿 1663）。
- 1662 两项备案（run-log 口径差 / 会话链适配缺口）**全部结案**，均非「迁移欠账」。
- **教训入记忆**：`merge-loss-endpoint-without-writer`——「口径差/迁移欠账」定性必须附实跑证据；扫 688/692 漏保真须核「端点 → 写入点 → UI 消费 → 测试」四环。
- 观察项新增：smoke 红档现场保留期（须区分 `--keep`）；688/692 漏保真族其余 fork 功能面（R 号立项逐面四环核查）。
- 下号自 1664 起；出口=观察项窗口+新对标周期（v11 规划轮）。
