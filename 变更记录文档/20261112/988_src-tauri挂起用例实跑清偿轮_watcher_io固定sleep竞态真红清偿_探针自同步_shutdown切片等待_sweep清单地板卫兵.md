# 988 号：src-tauri 挂起用例实跑清偿轮——watcher_io 固定 sleep 竞态真红清偿 + 探针自同步重写 + shutdown 切片等待 + sweep 清单地板卫兵

日期：2026-11-12。工程基调（goal）：最好最安全可靠——M1a 教训直系（被 ignore 的测试路径=零观测盲区，两枚潜伏缺陷曾全部藏身其中）。开工三查：987 并行在途（并行会话持有 review-cycle-matrix resync 占位分修复，walkthrough 进程活跃）→ 撞号让号，本轮取 988，全程避开 `scripts/review-cycle-matrix.mjs` 与 engine-rs 域。

## ① 普查：src-tauri 全树挂起用例 25 枚盘点

- lib 单测 7 枚：`pf_guard_lock_bails_without_root`（C5 回归，pfctl 非 root bail）+ keychain 真实四枚（roundtrip / delete-missing-noop / stale-tolerated / concurrent-20-线程无丢键）+ updater 网络双通道两枚（`real_engine_channel_check` GitHub release 探测 + `real_bootstrap_downloads_and_verifies` node 二进制真实下载校验——166 号双通道活体）。
- tests/ 集成 18 枚：`sync_watcher_io` 3（notify watcher 端到端）+ `e2e_secrets` 9（真实 keychain e2e，注释明示 dark wake 弹授权框阻塞）+ `e2e_startup` 2 + `e2e_updater` 1 + `supervisor_integration` 3（需 release 二进制）。
- engine-rs 零挂起（884 号清偿口径维持）。首查漏了 tests/ 目录（只扫 src/）——盲区普查自身也曾是盲区，复扫补齐。

## ② lib 7 枚实跑全绿（本机 macOS 前置全备：keychain 可达 + pfctl 内置 + 非 root + 有网）

keychain 真实四枚 + pfctl + updater 双通道全部拿到活体证据。假红勘误一轮：首次行内 `cargo test | tail -25` 把 lib 段结果截掉且管道吞了 cargo 退出码（tail exit 0 ≠ cargo exit 0），差点误读全绿——「统计落盘再解析」纪律（532/552 号）再次自证。

自身双跑事故勘误：第一轮后台超时转后台后未确认退出即发起第二轮，同机双 cargo 实例因 target 锁 + securityd/keychain 跨进程串行化造成 **~15 分钟停滞假象**（单跑时并发用例 33s 内完成）——一度误判「挂死潜伏缺陷」，随后 4/5 绿证据推翻预判并如实定性为自身操作事故。教训入 988 备案与 sweep 脚本头注释（单跑纪律）。

## ③ 真红命中：sync_watcher_io 固定 sleep 注册竞态（M1a 形状精确复演）

`watcher_spa_write_writebacks_to_keychain` 确定性红：单跑无负载复现（2.01s，`got {}`——回写未发生）。诊断链：

1. MockStore 用例与 keychain 无关；stderr 零 `[secrets-writeback]` 日志 = watcher init/watch/process 全部错误分支未触达。
2. Rust 解析器（jsonio collect_api_keys）期望形状与测试字面量一致 = schema 无罪；TS 写方同形状。
3. 独立探针（/tmp/notify-probe，镜像生产用法）实证 notify 6.1.1 FSEvents 送达面正常：一次写 → 3 事件（Create/Metadata/Data），路径含目标文件名 → notify 无罪。
4. 判别实验（只改测试时窗）：注册 sleep 200ms→500ms 仍红，→2000ms 绿——**注册时延环境相关且波动大**（本机同日实测 500ms 不够 / 2000ms 够 / 另轮 ~1.7s），固定 sleep 猜不准，写入落在 watch 生效前被静默丢弃。

危害双形：spa 测必红；更危险的是 `watcher_anti_loop_programmatic_write_does_not_writeback` **空真通过**——事件丢失时「store 为空」断言恒成立，真反回环回归同样测不出。产品面无罪定性：生产中注册在启动期、SPA 写在用户操作后，注册窗口无实际写风险 = 纯测试设计缺陷。

## ④ 修复 = 探针自同步（消除猜 sleep）+ 防回环中途断言

- `probe_watcher_ready`：写探针键（`__watch_probe__`）→ 轮询 store 观测；未观测到则**换值重写重试**（注册完成前的探针写被静默丢弃，重写自愈；上限 6 轮）。观测到即 watcher 活体证明——此后事件不再可能丢，断言全改 `wait_until` 轮询等待。
- 探针**返回实际观测值**，后续步骤动态取互异新值（首轮教训：硬编码 `p1` 在重试轮数 > 1 时假红——观测值随注册快慢漂移）。
- 防回环测**中途断言**设计：探针活体保证跳过周期必跑 → 程序化写（syncing=true，json 同时改探针值+新增 A 键）→ 中途断言（store 仍基线值、无 A）→ 末写（syncing=false，仅探针新值）轮询观测后终态断言。中途点必要性的论证：A 若中途泄漏，末写的删除传播会把它清掉——事后断言不可见，只有中途点能抓「syncing=true 期间泄漏」。
- NEUTER988 必红活体：`should_writeback` 恒真摘门 → anti_loop 恰红信号一（`got {"__watch_probe__": "p1_prog", "A": "va"}` 泄漏形完整可见）→ 精确互逆恢复复绿 3/3 → 连跑 3 轮稳态全绿。
- 生产面顺带一行：watch 成功后 `eprintln!("[secrets-writeback] watching <dir>")`——成功路径此前全静默，「watcher 活体」与「disabled」在排障时不可区分（本次定位模糊的根源）。

## ⑤ 产品面顺带修复 = debounce 窗内 shutdown 切片等待

shutdown 测对齐后暴露真实尾延迟：回写处理的 chmod 0600 恢复**自身触发文件事件** → 尾随 debounce 周期；旧 `debounce_and_process` 整段 `recv_timeout(remaining)` 让置位落在窗中的 shutdown 白等满剩余窗才响应（实测尾延迟 492ms / 620ms / 1.164s 随负载波动）。修复：窗内等待按 `POLL_TIMEOUT_MS` **切片**（`wait = remaining.min(poll)`），Timeout 切片 ≠ 静默期满（`now >= deadline` 才 break）——debounce「聚合连续写」语义与 **flush-on-shutdown**（break 后仍执行 process_writeback 排掉悬置回写，防「SPA 末写未回写 + 下次启动 keychain 主覆盖」丢密钥）完整保留，退出延迟均匀 ≤ 名义 300ms。模块契约注释同步勘误（原「延迟 ≤ POLL_TIMEOUT_MS」未计窗内情形）。测试界 `POLL×10` = 名义 3×POLL + 满载调度尾（sweep 语境 620ms 一次实测——并行会话走查负载在场；结构面无白等路径，超 1s 才可能是真挂）。

## ⑥ 新资产：cargo:ignored-sweep + 清单地板卫兵

`scripts/cargo-ignored-sweep.mjs`（`pnpm cargo:ignored-sweep`）：lib 7 + watcher_io 3 全清单实跑，**清单地板卫兵** = 实测集合与 INVENTORY 登记清单精确相等——登记用例未观测到绿（被删/改名/过滤器失效）、观测到未登记用例（新增挂起须有意更新清单）、用例红、cargo 非零退出，任一即非零退出：静默缩水到 0 用例的清偿面必须红（847/848 恒绿纪律）。头注释备案：豁免族（e2e_secrets 9 枚授权框阻塞——无人值守禁跑；release 二进制族 6 枚——专项轮清偿）+ 单跑纪律（同机双 cargo = 停滞假象，见②）。活体：10/10 绿 exit 0；同日一次地板卫兵抓红复验（shutdown 旧界 300ms 未吸收调度尾时恰红、报文含现场日志路径）——守卫力实证非恒绿。

## ⑦ src-tauri 有界化第三面定性（985 族扫收账）

src-tauri 全局面仅 `GLOBAL_STATE_LOCK: Mutex<()>` 一把（observability），无任何进程内表族——进程内表有界化三面收官：engine-rs（984）+ studio TS（985）+ src-tauri（本轮定性零需求）。

## ⑧ 门禁

clippy:gate 双 0（engine-rs 0 告警含并行会话在途树 + src-tauri 0 告警）；cargo:testgate src-tauri 25 目标全绿 584 passed（持平）+ audit:rust 双 0（必红夹具 7+1 自检活）；sweep 10/10 exit 0；watcher_io 三枚连跑 3 轮稳态；`node --check` 新脚本语法核过；U+FFFD 净；TS 面本轮仅 package.json scripts 一行（gate:ts 引用 986 十步全绿，TS 树零改动）。本提交严格路径限定五文件：`package.json` / `scripts/cargo-ignored-sweep.mjs` / `src-tauri/src/secrets/sync.rs` / `src-tauri/tests/sync_watcher_io.rs` / 本文档（工作区 engine-rs 两文件与 review-cycle-matrix.mjs = 987 在途，不卷入）。

## 备案

1. e2e_secrets 9 枚（授权框阻塞）与 release 二进制族 6 枚维持挂起；触发条件 = 专项轮或 CI 环境具备。
2. shutdown 满载调度尾 620ms 实测已写入测试界注释；若未来出现 >1s 退出再归因。
3. 987（resync 审计占位分）并行在途未收口；本轮证据未依赖其改动面。

下一号自 989 起；出口回归常备；用户侧待办 = Fork 推送（含 955-988）。
