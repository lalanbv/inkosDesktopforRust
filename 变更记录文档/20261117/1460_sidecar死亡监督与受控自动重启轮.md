# 1460 号：sidecar 死亡监督与受控自动重启轮——壳自愈面落地

## 目标与范围

/goal 自主持续开发续轮。工程 backlog（1455-1459）清偿后的进化方向选点：
**sidecar 运行中死亡的自愈面**。考古确认：现监督链=spawn→health_probe→
导航→observer（SSE 自带 500ms 重连）——但**运行中引擎死亡（崩溃/OOM/
误杀）app 零感知**：UI 停留原页但 API 全红，唯一恢复路径=重启整个 app
（1453 用户报障「引擎不在跑」正是此面的用户侧形态）。本轮补齐监督
watcher+受控重启。

## 实施

①**决策器**（supervisor.rs `RespawnPolicy`）：死亡→冷却 3s 允许重启；
连续 5 次（会话内）达上限停（崩溃风暴防护——环境性问题反复拉起只烧
CPU 刷日志，停下+通知用户才是正确行为）；健康存活满 5min 计数清零
（偶发崩溃≠持续故障）。单测三面锁（允许/拒绝/健康清零）。

②**单发探测**（`probe_once`）：与 health_probe 同判活语义（2xx=活），
一次一发 2s 超时——监督节律自带，无需内部轮询。**HTTP 探测而非进程
wait 的选型**：连「进程在但挂死」面同覆盖，且零触碰 SidecarState 的
Child 所有权（watcher 与清理路径无纠缠）。

③**监督 watcher**（main.rs）：健康起跑后 `spawn_supervision_watcher`
接力——2s 周期探测；连续 3 次（≈6s）判定死亡→决策器→通知用户（第
n 次）→冷却→清场本代（child 收尸+loopback guard 释放）→重跑
`spawn_sidecar_task` 整链（新 watcher 随新代接力）；达上限→通知+title
回显+停监督。阈值容忍瞬态（引擎无运行中重启场景，updater=下次启动
生效）。

④**spawn 链可重入化**（结构性障碍拆除）：Tauri 同类型二次 `manage`
panic——`SecretsWritebackState`（预托管+装载条件化：respawn 复用运行
中 watcher——它 watch secrets.json 文件非引擎进程；shutdown 标志随
装载换代防 Exit 清不到活 watcher）与 `ObserverShutdown`（预托管+
`rotate` 换代：旧 SSE watcher 指向死端口空转，换代停旧起新）双改造；
`RespawnState`/`SidecarHealthySince` 新托管。teardown 复用 Exit 清理
同款语义（kill_tree 幂等+guard release）。

## 验收

- RespawnPolicy 单测三面绿（允许到上限/拒绝/健康清零恢复）
- testgate src-tauri **25 目标 594 passed（+1）**全绿；clippy --all-targets
  双 crate 0 告警（needless_borrow 两处清）；gate:ts 十腿全绿（改动树
  全谱）；audit 夹具活
- 深度 e2e（杀 sidecar 看 app 自愈接力）=#[ignore] e2e_startup 族扩展
  方向备案（需 release binary+窗口环境，非本轮门禁面）

零新缺陷，U+FFFD 净。

出口=下一巡检周期（2026-11-18 到期，明日触发）/earendil-mz 单包合并；
用户侧待办=Fork 推送(含 955-1460)+正式打包三脚本重烤。下一号自 1461 起。
