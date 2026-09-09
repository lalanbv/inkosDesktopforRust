# 263 号：wasmtime 27→36.0.14 升级交叉评审——src-tauri 全量门禁绿 + 真实组件端到端验证

- 日期：2026-09-10
- 分支：develop
- 关联：257 号（wasm 插件面先例）、229 号（plugin host_api 深度抽查）、262 号（并行会话在途备案清偿）
- 推送核验：origin/develop 仍停 6bc65564；本批落库后本地领先 67 提交。两项默认值无新答复。

## 一、升级内容（并行会话在途批，本批交叉评审后落库）

- `src-tauri/Cargo.toml`：wasmtime / wasmtime-wasi **27 → 36.0.14**（精确版本锁定，不用浮动次版本——安全敏感组件的确定性依赖）。
- `src-tauri/src/plugin/runtime.rs` 三处 API 适配，逐一对上 wasmtime 36 官方破坏性变更：
  1. `WasiView::ctx` 由双方法改为返回 `WasiCtxView { ctx, table }`（36 的 table 并入视图）；
  2. 新增 `impl HasData for PluginState { type Data<'a> = &'a mut PluginState }`（bindgen `add_to_linker` 新增宿主数据 GAT）；
  3. WASI p2 同步注册入口迁移至 `wasmtime_wasi::p2::add_to_linker_sync`；`InkosPlugin::add_to_linker::<PluginState, PluginState>` 显式标注宿主数据泛型（闭包返回类型与 HasData 一致）。
- `Cargo.lock`：常规依赖树更新（ar_archive_writer 移除、allocator-api2 引入等，全部 crates.io 源，无异常）。

**安全面核查**：diff 仅链接机制适配，未触碰 229 号评审过的 host 能力面（域名白名单/命令白名单/配额）；wasmtime 27→36 跨 9 个 minor 含多批沙箱加固补丁，升级本身是安全正收益。

## 二、复验证据（全绿）

| 门禁 | 结果 |
|---|---|
| src-tauri cargo test（升级后全量重编译） | ✓ **576 通过 0 失败**（lib 466 + 集成/多 bin 110） |
| cargo clippy --all-targets | ✓ 0 告警 |
| **真实组件端到端**（wasm_plugin_execution） | ✓ **3/3 真跑**（非 skip）：8 月 7 日编译的示例 .wasm（旧工具链产物）在 wasmtime 36 运行时 Linker → 实例化 → 类型化 invoke 全链通过——组件模型二进制跨 9 个 minor 的向后兼容实测成立 |

## 三、结论

批准落库。wasmtime 升级为纯适配无行为变化，且以旧工具链组件的真实执行验证了运行时兼容性——比「仅编译通过」更强的证据。

## 四、遗留

- examples/wasm-plugin 若要 rebuild，需 wasm32-wasip2 工具链（现产物可继续用于测试）。
- 推送（255–263 共 67 提交）、两项默认值、ja A/B 仍待用户决策。
