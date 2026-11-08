# 948 号 · 备份完整性补强轮：backup/export 前 per-work source_sync（双端）——947 竞态发现的防御性补强

> 947 备案的防御性补强：backup/export 打包前对 works/ 全部 work 跑
> source_sync 快照补齐（幂等，accept=false 只快照不 accept）——防「手
> 工放置/启动竞态」下 works 数据未同步即打包的完整性窗口（947 端到端
> 验证发现备案：引擎首起 source_sync 异步未完成时 export 不含 works）。
> sync 失败不阻断备份（warn 语义）。双端同改。
> 验证：TS backup-routes 6/6（works 种子断言过）+Rust backup_routes
> 7/7+testgate 2056/584+typecheck 净+gate:ts 十步全绿+U+FFFD 净。

## 一、实现（双端同构）

- **Rust backup_routes export 端点**：staging 创建前读 works/ 子目录逐
  work 调 `sync_work_source_artifacts`（accept:false+writes 空+status
  None=纯快照补齐语义）——失败 tracing::warn 不阻断备份。
- **TS backup/export 端点**：同位置 `syncWorkSourceArtifacts`（从
  @actalk/inkos-core 既有导出解构引入——harness/index 已导出）逐 work
  同步，try/catch 不阻断。
- 语义说明：sync 是幂等快照补齐（引擎首起与备份前双时点双保险）；
  accept=false 保证不翻转 accept 面（只补 manifest/快照不改变用户
  accept 状态）。

## 二、验证汇总

- TS backup-routes 6/6（works 种子断言过=sync 后 export 含 works 条
  目——集成路径实证）；Rust backup_routes 7/7；testgate engine-rs
  **2056**/src-tauri 584；typecheck 净；gate:ts 十步全绿；U+FFFD 净。

## 三、后续出口

- 947 竞态备案清偿（备份完整性双时点双保险）。
- 出口清空回归常备。

**下一号自 949 起；出口清空回归常备；用户侧待办=Fork 推送 66 个提交。**
