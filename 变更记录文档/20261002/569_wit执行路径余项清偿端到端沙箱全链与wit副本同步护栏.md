# 569 号：wit 执行路径余项清偿——真实 component 端到端全链沙箱执行+wit 副本同步护栏+host_api 勘误

- 日期：2026-10-02
- 类型：test(src-tauri)+fix(examples)
- 路线：568 号 wit 状态注记「唯一余项」清偿（Phase 6.3 第 5 步收官）
- 状态：✅ 门禁全绿收口

## 考古再反转（余项已大半建成）

directive 前提「从零建 wit-bindgen 测试插件」过期：`examples/wasm-plugin` crate（wit-bindgen 0.30 客侧绑定+wasm32-wasip2）与 `tests/fixtures/inkos_example_plugin.wasm`（66KB 受跟踪组件）与 `tests/wasm_plugin_execution.rs`（echo/ping/未知命令三测，含「本地新构建优先、受跟踪组件兜底」双源）均已存在——Phase 6.3 第 5 步的大半已建成（施工图前提过期第六例）。真实差距=**覆盖面**：on-event 链、guest→host 能力调用（read/exec/log）、权限拒绝与白名单 fail-closed 的端到端语义均未验证；且 **examples 的 wit 副本已语义级分叉**（缺 exec-command 整块——guest 绑定静默缺能力面）。

## 交付面

### ① wit 副本同步护栏（`wit_contract_copy_in_sync_with_host`）

`examples/wasm-plugin/wit/inkos.wit` 必须与 `src-tauri/wit/inkos.wit` 逐字一致——副本漂移=guest 绑定静默缺失能力面（本专项实测分叉即实证）。护栏首日即抓到一次：wit 注记更新后未重同步副本→cargo:testgate 红→重同步（负样本拦截当场实证）。宿主契约改动的三处同批纪律由此机械化为：wit 改→cp 同步→fixture 重建→测试绿。

### ② 示例插件扩宿主能力调用面（examples/wasm-plugin）

- 新命令：`host-read`（宿主 read_file→content JSON）/`host-exec`（exec_command→{stdout,stderr,exitCode}）/`host-log`（host.log）。
- `on_event` 改经 `host.log` 观测（on-event→host 链端到端可见，tracing JSON 日志可查）。
- wit 副本同步后 guest 绑定获得 exec_command（原分叉态下不可用）；Cargo.toml 增 serde_json（参数解析）。

### ③ 宿主端到端测试扩五件（wasm_plugin_execution.rs 3→9 测试，全绿）

- **wit 副本同步护栏**（①）。
- **能力通过链**：ReadProject+Filesystem{绝对路径} 双 capability 下 host-read 取回文件内容（guest import→Host trait→HostContext→fs 全链）——路径允许列表语义实测确认：Filesystem 只认绝对 allowed_path（相对 fail-closed 纵深防御）+tempdir 须 canonicalize（macOS /var→/private/var 符号链接）。
- **权限拒绝**：零 capability host-read→`缺少 read_project 权限` 语义透传（guest Err→ExecutionFailed 包装，非 trap）。
- **exec 白名单双向**：白名单内 `echo hello` 经宿主真实执行（stdout/exitCode 回传断言）；白名单外 `curl` fail-closed（`缺少 system_command 权限或命令不在白名单`）。
- **on-event 链**：broadcast_event 完成 实例化→init→on_event→host.log 全链。
- **错误类型化**：matches!(PluginError::ExecutionFailed(msg) if msg.contains(权限语义))。

### ④ host_api.rs 第三处陈旧注释勘误（568 同款清偿）

`exec_command` doc 曾称「wit/inkos.wit 无此导入、WASM 插件无法执行系统命令（任意执行面为零）」——与 wit 契约+Host trait 实际接线不符（白名单机制本就「接线就绪」）。勘误为「WASM 路径已接线，受白名单收敛，端到端验证=本测试」；安全语义不变：SystemCommand 白名单 fail-closed、is_safe_command_name 拒元字符。

## 门禁

- clippy:gate 双 crate 0 告警
- cargo:testgate：engine-rs 45 目标 1893 + src-tauri 24 目标 583（+6 端到端测试）全绿
  - 插曲：`strangler_timeline_duel` 首跑 600s 超时（引擎 60s 未就绪）——隔离复跑 0.63s 绿=门禁紧跟重型构建的负载伪象（bench-gate-load-noise 同源），空闲复跑整门禁绿，未动任何基线
- gate:ts 七步全绿

## 教训

- 施工图前提过期第六例：「唯一余项从零建」实为「已建成，缺覆盖面」——wit 注记的「余项」表述下轮起附当前覆盖清单。
- fixture 预编译入库+源码在库双轨：门禁密闭（gate 机无需 wasm target），重建路径=示例 README 命令。
- wit 副本同步护栏首日抓到自己一次（改宿主 wit 未同步副本）——「契约改→cp→重建 fixture→测试绿」四步由护栏机械化。

## 关联

- 前序：568（wit 状态注记+护栏）、M7g（wasmtime 全链）
- 下一号自 570 起
