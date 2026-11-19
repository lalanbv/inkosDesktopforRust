# 1583 回环代理族测试面漏网族清偿 + wasmtime 供应链升级 + 工具链 1.100 轮（2026-11-20）

## 面一：sync_watcher_io shutdown 界 ×10→×25（sweep 真红勘误）

首轮 sweep 真红（exit 1）：`watcher_shutdown_flag_exits_thread_cleanly` 断言界 1000ms（POLL_TIMEOUT_MS×10），实测 **1.734s**。定性=非代码缺陷：结构面所有 shutdown 响应路径（主循环 recv_timeout 100ms 切片/debounce 窗 100ms 切片/置位后 flush 毫秒级）≤100ms+flush，无白等路径——是**满载+同腿三枚并行（=sweep 语境）下调度尾破界**（事件投递/切片唤醒/join 唤醒各环节放大数倍）。满载复现 4×yes 负载下 7 跑 4 红（1.18s/1.73s 两破界点），轻载/单跑全绿=线程正常退出非真挂。988 号当时以实测 620ms 定 ×10 未给足演进余量。修=界 ×10→**×25**（2500ms，实测峰 1.73s+44% 余量；真挂=永久挂由 sweep 腿预算哨兵兜底，界只覆盖噪声带不弱化哨兵语义）。连带勘误 sync.rs 文档注释两处漂移（「×3 窗口」为 988 号拆分前旧值；文件名指已拆分的 sync_unit.rs）。四拍：红=sweep 实跑+满载复现（无需人为撤修），绿=同语境 ×25 三轮全绿（loadavg 6.57）。

## 面二：rust_backend_launch probe_client 代理劫持（testgate 真红，窗口性假红）

testgate 首跑 src-tauri 腿红两枚：health 契约 GET **ConnectionReset by peer** + SPA 面非 text/html。勘误链：单跑/满载复现全绿（窗口性）→ 新增死因观测面（失败分支打印 `child.try_wait()` 三分类：exit code=自退/signal=外部杀/None=仍运行）→ **死代理 env 红路活体实证：2/2 红、观测面报「引擎进程状态: None（仍运行）」——引擎根本没死**。真因=`probe_client()` 默认 reqwest 构造读 env/系统代理（auto_sys_proxy→macOS SCDynamicStore，**不读 ExceptionsList**，1571-1581 十轮实证的机制）：系统代理 127.0.0.1:1082 开着时 GET 被劫持去代理进程——代理转发回环成功时绿（劫持不可见）、代理对回环 RST 时假红（ConnectionReset 的 "peer" 是代理不是引擎）。health_probe 臂（1572 谓词面）照样过=两臂不对称。修=`.no_proxy()` 恒直连（同 1581 语义）。观测面红路已闭环（本次红跑自动工作）。

## 面三：回环代理族测试面漏网族全量清偿（按族扫，22 处）

「修洞按族扫」纪律扩面：src-tauri/engine-rs **测试面**全部 reqwest 构造点普查——
- `engine-rs/tests/strangler_duel.rs`：`Client::new()` ×17 + 顶层便捷函数 `reqwest::get()` ×2（wait_ready 探活/SSE 事件采集——**顶层便捷函数内部构造默认 Client，形态不同极易漏眼**）
- `engine-rs/tests/short_capability_router_smoke.rs`：×2
- 全部收敛 `loopback_client()` helper（builder().no_proxy()，单点注释化）

**红路四拍（duel bin 函数）**：撤修形态+死代理 env → 60.11s 红（wait_ready 经死代理 60s 全败）→ 复原+拔除 wait_ready 的 `reqwest::get()` 漏网 → 死代理 env 0.52s 绿 → **全量 duel 10/10 死代理绿**（1.04s；TS 腿函数走 688 守卫响亮跳过=既有备案形态，真跑面由 bin 函数 0.52s 全链实证）。smoke 死代理 1.51s 真跑绿。过程勘误：①复原 Edit 后 cargo 增量判定曾引编译态疑云——touch 强制重编+完整输出定案（复原版仍红=第二层漏网 wait_ready，非编译态）；②duel 须 INKOS_DUEL=1 真跑（0.00s 早退假绿再踩再证）；③红拍 panic 路径 command.kill() 不可达泄漏 3 枚引擎孤儿——kill+LISTEN 归零（sweep post-clean 同族形态）。

## 面四：wasmtime 36.0.21→50.0.2 供应链升级（audit 真红）

testgate 前置 audit:rust 拦截：**RUSTSEC-2026-0567**（threads feature 缺失 abort）+**RUSTSEC-2026-0573**（GC heap reservation 非 page size 倍数 abort）——均 abort 类 DoS（非内存不安全），2026-11-16 新公告，36 系无修复版（修复线 48.0.9+/49.0.6+/50.0.2+）。依赖面单文件 plugin/runtime.rs（component API 稳定面）。三修复线 MSRV 均超本机工具链（48→1.95/49→1.96/50→1.97 vs 本机 1.94）→ **rustup stable 1.94→1.100**（2026-03 版 8 个月陈旧）→ wasmtime 50.0.2 编译零 error（component API 跨 major 兼容）。audit 复跑双 crate 0 漏洞，必红自检活着。

## 面五：工具链 1.100 新 lint 面清偿（26+ 处）

1.100 相对 1.94 的新 clippy lint 对既有代码浮出 26 处（src-tauri 5+engine-rs 21+tests 3）：`cargo clippy --fix` 自动清 10 处（collapsible_match/while_let_loop 等机械类，diff 审查）+ 手工清 16 处——sort_by 降序→`sort_by_key(Reverse)` ×12（稳定排序语义不变）、result_large_err ×4 局部 `#[allow]` 带注释定性（axum handler `Result<T,Response>` 惯用形/ProductionError 大体，错误路径冷，Box 化改动面大收益微）、governed_working_set range 索引→切片迭代。src-tauri 侧：fetch_update→try_update ×2（std 1.100 改名）、checked_div ×2、collapsible_match ×1。engine-rs 顺带清 anyhow 未用依赖（cargo 1.100 新默认 lint cargo::unused_dependencies 浮出，src/tests 零使用实证后删除）。

## bench:gate 复试（仍拒，维持备案）

loadavg 3.90/18 核=22% > 20%（四轮最低 32%→24%→28%→22% 回落趋势，未入静默）。维持 1448 号定性。

## 验证

- cargo:testgate 双 crate **2729 passed**（engine-rs 58 目标 2129 + src-tauri 26 目标 600，与 1581 基线持平）+ audit 双 crate 0 漏洞
- clippy 双 crate 0 告警（-D warnings）；fmt 双 crate 0 漂移；gate:ts 十腿全绿（check-routes 自检绿）
- U+FFFD 净；duel 全量死代理 10/10；rust_backend_launch 死代理 2/2 绿
- 工具链升级影响面：全量门禁重跑全绿=线性可控

## 后续

- 下号自 1584 起；用户侧待办更新：Fork 推送 955-1583 + 正式打包三脚本重烤（**wasmtime 50 大版本变化，重烤后须 e2e_startup 冒烟复核插件执行面**）+ 静默时段 bench:gate（22% 逼近阈值，回落趋势）。
