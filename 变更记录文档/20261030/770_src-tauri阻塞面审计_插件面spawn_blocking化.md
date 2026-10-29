# 770 号：src-tauri 异步阻塞面审计——插件面 spawn_blocking 化（763 路线③方法论对桌壳补课）

> 目标线：高性能/可靠。763-766 的异步阻塞面清偿只覆盖 engine-rs——桌壳
> （src-tauri，Tauri async command 跑在 tokio runtime 上，同步重 IO 同样
> 占 worker）从未做过同型审计。本批补课。

## 一、审计定性（75 非测试站点全量清点）

### async 宿主 20 处 → 分级

| 宿主 | 形态 | 处置 |
|---|---|---|
| `plugin/manager.rs` install_plugin/update_plugin | **伪 async**（声明 async 函数体零 await），重 FS=整目录复制 copy_dir_all + 旧目录 remove_dir_all，且全程持 manager tokio 锁 | **清偿**：两方法去伪 async（改同步签名），命令层 `spawn_blocking + blocking_lock` 承载 |
| `plugin/commands.rs` install/uninstall_plugin 命令 | uninstall 体含 remove_dir_all | **清偿**（同批 spawn_blocking） |
| `commands/registry.rs` cmd_install_from_registry（无依赖直装）/ cmd_install_deps 循环 / cmd_update_plugin_from_registry | 同上三态调用 manager 安装体 | **清偿**（四处调用点全部 spawn_blocking 化） |
| `updater/engine.rs` apply/apply_inner | 重 FS（bundle 读取可上百 MB+解压+目录 rename），但混合 await（fetch_release）且为更新模态下的用户一次性操作 | **豁免入档**：重构 async/sync 切分属大手术，模态 UX 下无可并行任务受扰；后续 updater 专项再议 |
| `engine/node/mod.rs` resolve | bootstrap 一次性缓存管理（create/remove 小件） | 豁免（启动一次性） |
| `commands/registry.rs` fetch_registry_index 缓存写读 / `observability/diagnostics.rs` cmd_get_diagnostics | 小文件写读/诊断快照 | 豁免（亚毫秒级） |

### sync 宿主 55 处 → 豁免台账

config/loader/paths/persistence、engine/manifest、main 启动路径、
observability/logging/crash（panic hook 同步写=崩溃路径必须同步）、
plugin registry safe_extract（由 spawn_blocking 调用方承载）、
projects/workspace/migration（启动迁移一次性）、secrets jsonio/sync
（0600 同步契约，762 同型）、updater 辅助纯函数。

## 二、变更内容（4 文件）

1. `plugin/manager.rs`：install_plugin/update_plugin `async fn` → `fn`
   （函数体零 await 实证后去伪）；对应测试调用去 await。
2. `plugin/commands.rs`：install/uninstall 命令改
   `spawn_blocking(move || manager_arc.blocking_lock()...)` 模式（clone Arc
   进闭包——tokio MutexGuard 借用非 'static 不能直接 move，blocking_lock
   在阻塞线程取锁）。
3. `commands/registry.rs`：四个调用点（无依赖直装/依赖循环/更新/卸载外的
   安装）同模式。
4. `tests/marketplace_e2e.rs`/`tests/plugin_execution.rs`/`manager.rs` 内
   测试：await 清理（含多行形态）。

## 三、锁语义说明（行为等价性）

- 锁持有语义不变：install/update 全程仍互斥（blocking_lock 持有至操作
  完成）——其他插件命令在锁上排队的行为与此前一致；
- 变化仅是承载线程：重 FS 从 tokio worker 移到 blocking 池，worker 不再
  被占（uninstall 的锁等待者不再被重 FS 拖住 worker）；
- 错误形状逐字保留（JoinError 与 PluginError 双层 map_err 消息同文）。

## 四、验证

- src-tauri 全测：lib **467 passed** + 集成（marketplace_e2e/plugin_
  execution 等）全绿；全目标 testgate：engine-rs 46 目标 **1944 passed**
  （与 768 基线同数=engine-rs 零触碰吻合）+ src-tauri 25 目标 **583
  passed**。
- clippy 双 crate **0 告警**。
- bench 面：插件安装非 21 基准组内（桌壳侧无 criterion），无需重锚。

## 五、后续备案（未做，豁免台账已记）

updater apply 的 async/sync 切分（重 FS 但模态一次性）——updater 专项
再议；engine/node bootstrap 缓存管理——启动一次性豁免。
