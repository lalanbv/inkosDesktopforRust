# 264 号：wasmtime 安全升级——并行批记录归并（原编号 263，撞号让位）

> 编号说明：本文与并行会话已提交的 263 号（0599e3cf，交叉评审记录）描述**同一落库改动**，
> 因两会话同时取号撞 263，按「先提交占号」让位改编 264。补 263 号未含的升级动机与审计数据；
> 文中验证声明已经 263 号轮次独立复验（576 测/clippy 0/真实组件 3/3）。


- 日期：2026-09-10
- 分支：develop
- 关联：259 号（panic 审计首 commenced 轮，server 面）、262 号（并行会话 panic 审计扩展——与本轮批A 独立同向，结论收敛）、M7a（WASM 插件运行时）
- 编号衔接：承接 262 号顺延 263
- 推送核验：HEAD aafa768e；本地领先 origin/develop 60+；本批待提交

## 一、批A：panic 面审计扩展至 interaction/ + pipeline/（259 号遗留 #2 承接）

非测试代码全部 unwrap/expect/panic!/unreachable! 逐位点核 origin：

- `persisted_governed_plan.rs`（7）＝静态正则；`session_restore.rs`（5）＝json! 自构造 + `last["role"]=="assistant"` 判负走 else 的守卫；`write_next.rs`（3）＝AbortHandle 锁 + 静态正则；`sub_agent_tool`/`material_tools` 等＝json! 自构造；`edit_controller`＝无。
- **结论：阴性（零外部可达 panic）**。与并行会话 262 号独立审计（298 位点）结论收敛互证。

## 二、批B：wasmtime 27→36 安全升级（本批主体）

`cargo audit` 首扫（本轮新增工具化验证）：

| crate | 升级前 | 升级后 |
|---|---|---|
| src-tauri（Cargo.lock 767 crates） | **error: 21 vulnerabilities** | **0 vulnerabilities** |
| engine-rs（326 crates） | 0 vulnerabilities（2 警告） | 未触及，维持 |

21 个漏洞中 **20 个为 wasmtime/wasmtime-wasi 家族**（27.0.0），含沙箱逃逸级：
- RUSTSEC-2026-0096（aarch64 Cranelift guest heap 错译 → sandbox escape）
- RUSTSEC-2026-0269（符号链接尾斜杠 → filesystem sandbox escape）
- RUSTSEC-2026-0222/0088（类型索引混用 / pooling allocator 数据泄漏）等 17 条
- 插件宿主恰是运行第三方 wasm 的组件——wasmtimesandbox 即最后防线，属本仓最高优先级安全面
- 另 h2 0.4.15→0.4.19（RUSTSEC-2026-0258 空 DATA 帧无界，reqwest http2 面）

**升级内容**：
- `Cargo.toml`：wasmtime/wasmtime-wasi "27" → "36.0.14"（覆盖全部 20 条通告的解法区间）；`cargo update -p h2`（0.4.19）
- `runtime.rs` 三处 API 适配：
  1. `WasiView`：36 版 table 并入 `WasiCtxView`（trait 仅剩 ctx 成员），改为返回 `WasiCtxView { ctx, table }`
  2. `add_to_linker_sync` 移至 `wasmtime_wasi::p2` 模块
  3. bindgen 新 `HasData` GAT：`impl HasData for PluginState { type Data<'a> = &'a mut PluginState; }` + `add_to_linker::<PluginState, PluginState>` 显式标注

## 三、验证

| 门禁 | 结果 |
|---|---|
| src-tauri cargo test（含插件管理器全链） | ✓ **576 通过 0 失败** |
| src-tauri clippy --all-targets | ✓ 0 |
| src-tauri cargo audit | ✓ **0 vulnerabilities**（8 条 unmaintained 警告为传递依赖遗留） |
| engine-rs | 本批未触及（259 号门禁 1618/0 仍有效） |

## 四、遗留

1. **[263 号轮次更正：不成立]** `src-tauri/tests/wasm_plugin_execution.rs` 即真实 component 执行集成测试（加载 examples/wasm-plugin 编译产物，Linker→实例化→类型化 invoke 全链），升级后实测 **3/3 真跑通过**（非 skip）——遗留项作废。
2. 8 条 unmaintained 警告（unic-*/paste/proc-macro-error/fxhash/glib 等）均为传递依赖，无直接修复路径，随上游。
3. 历史瘦身、推送、两项默认值、ja A/B——仍待用户决策。
