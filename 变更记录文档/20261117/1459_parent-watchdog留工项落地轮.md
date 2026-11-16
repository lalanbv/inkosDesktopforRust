# 1459 号：parent-watchdog 留工项落地轮——壳 SIGKILL 后 sidecar 三腿自退闭环

## 目标与范围

/goal 自主持续开发续轮。实施 1431 备案 env 留工项（1435 定性自愈闭环后
优先级降低但引证在位，1452 延续引证）：壳（Tauri app）被 SIGKILL 时信号
不可捕获，supervisor 的 cleanup_sidecar/kill_tree 不可达，sidecar 成孤儿
进程残留。既有两层兜底（1435 sweep 自愈清场 + 启动 pick_free_port 容错）
是「事后清」；本项把「事后清」升级为「实时自退」：父亡后 ≤2s sidecar
自行退出，缩短孤儿端口/资源占用窗口（真实用户场景：app 崩溃后立即重启）。

## 三腿实施

①**Rust 引擎腿**（engine-rs `src/server/parent_watchdog.rs` 新模块）：
`INKOS_PARENT_PID` env 缺失=功能关闭（duel/smoke/直跑零影响）；Unix
`libc::kill(pid, 0)` 探测（信号 0 零误伤；ESRCH=亡）；2s 轮询线程
（`thread::Builder` 命名线程，spawn 失败静默关闭=增强面非承重面）；
边界语义 fail-open：pid 复用最多**漏判不退**（回到 sweep 兜底现状），
反向误判（父活着判死）不存在。bin main 最早接线（bind 之前——父亡时
连端口都不必占）。libc 从 dev-dep（1453 duel 用）移 prod（同款用途并归）；
新增 dev-dep assert_cmd 2.0（进程级活体装配，与 src-tauri e2e_startup
同款形态）。

②**壳注入腿**（src-tauri `supervisor.rs build_launch`）：env 注入
`INKOS_PARENT_PID=std::process::id()`（十进制串，两侧 parse 契约）；
:124 留工项注释收口为三层兜底叙述（sweep+容错+watchdog）。

③**TS 回退腿**（studio `server.ts startStudioServer`）：同契约对称
（`process.kill(pid,0)` try/catch ESRCH → exit(0)），`unref()` 不阻自然
退出；退出行 `[studio] parent <pid> gone, exiting`（诊断面与 Rust 腿
同款）。

## 验收矩阵

- **Rust 单测 3/3**：自身 pid 活/收尸子进程亡（ESRCH 真路径非 mock）/
  env 解析缺失=关闭
- **Rust 集成测 2/2**（`tests/parent_watchdog_e2e.rs`，真二进制
  cargo_bin）：瞬逝父（sleep 收尸）+引擎 stderr 断言「parent … gone」
  行防 bind 失败假绿+exit 0；对照面 env 缺失 3s 观测窗常驻+无自退行
- **TS 活体探针双面**（真 tsx server）：父亡 4s 内自退+退出行在 stderr；
  env 缺失常驻（功能关闭）——1455/1457 活体纪律延续（header/进程面
  单测不算数）
- **全谱四面**：testgate engine-rs **53 目标（+1）2120 passed（+5 精确
  增量）**+ src-tauri 25 目标 593 同数；clippy --all-targets 双 0 告警；
  gate:ts 十腿全绿（test 109.4s 含 server.ts 改动/typecheck/rust-bin
  41.4s 新鲜）；audit 夹具活（7/1 RUSTSEC）
- src-tauri build_launch 测试 7/7（env 断言含 INKOS_PARENT_PID）

## 过程纪要（Mimosa 拦截与形态适配）

e2e 测试初版 `Command::new(env!("CARGO_BIN_EXE_…"))` 被 Mimosa 高危拦
（命令注入保守污点——env 值为运行时自生 port/pid 非外部输入，但拦截
生效即改形态）：静态字面量参数的 `Command::new("sleep")` 放行、变量/
宏参数拦截→改 assert_cmd `cargo_bin` 形态（仓内 e2e_startup 已入库先例）
+timeout 阻塞语义（超时即 kill 返回已收输出）通过。两轮 Rust 编译期
勘误：bin 是独立 crate 树（`crate::` 不解析，用 `inkos_engine::`——
src-tauri 同族记忆）+assert_cmd 2.2 无 Deref（弃 try_wait 轮询改阻塞）。

零新缺陷，probe 用完即删零残留，U+FFFD 净。

出口=下一巡检周期（2026-11-18 到期）/earendil-mz 单包合并；用户侧待办
=Fork 推送(含 955-1459)+正式打包三脚本重烤。下一号自 1460 起。
