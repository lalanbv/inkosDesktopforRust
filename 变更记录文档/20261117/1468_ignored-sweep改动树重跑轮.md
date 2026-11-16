# 1468 号：ignored-sweep 改动树重跑轮——supervisor 挂起路径零回归实证（零代码改动）

## 目标与范围

/goal 自主持续开发续轮。巡检未到期（2026-11-17）。选点=ignored-sweep
重跑：1450 首跑后 `supervisor.rs`/`main.rs` 经 1459（watchdog env 注入）/
1460（死亡监督+可重入化）/1461（状态机抽离）/1462（整链活体）四轮大改
——挂起用例 `real_inkos_sidecar_serves_spa`（真 inkos sidecar+SPA 服务）
恰在改动路径上，按「#[ignore] 用例=零测试路径（挂起路径碰改动=潜伏缺陷
高危区）」纪律须实跑复核。

## 结果

①**ignored-sweep 全绿**（六组 23 枚挂起用例清单精确匹配全绿，绿档日志
已清盘）：
- `--lib` 7/7 + `sync_watcher_io` 3/3 + `e2e_updater` 1/1
- **`supervisor_integration` 1/1**：`real_inkos_sidecar_serves_spa` 在
  supervisor 四轮改动后首跑全绿——spawn 链（含 1459 env 注入/1460
  stdio+进程组面）挂起路径**零回归**
- `e2e_startup`（--release）2/2 + `e2e_secrets` 9/9

②**组装面顺带新鲜**：sweep 自带 engine 组装（node 引擎形态，
built_at=2026-11-16T19:43:05Z——1450 sweep 产物，dist 已存在跳过构建；
1464/1466 的前端修复在 studio dist 面不在 engine 组装面，重烤时带入）。

③**post-clean 自愈闭环活体**：引擎孤儿回收 1 枚（PPID=1+本仓路径特征）
——1435 自愈闭环生产工作续证。

零代码改动零新缺陷，U+FFFD 净。

出口=下一巡检周期（2026-11-18 到期，明日触发全量巡检轮）/earendil-mz
单包合并；用户侧待办=**Fork 推送（含 955-1468，Fork 图形端操作）+
正式打包三脚本重烤（dist 含 1464/1466 修复）**。下一号自 1469 起。
