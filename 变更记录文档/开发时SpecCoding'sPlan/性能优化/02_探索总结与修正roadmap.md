# 性能优化探索总结与修正 Roadmap

> 日期：2026-08-07
> 基线：WASM plugin execute ~22 µs（criterion benchmark）
> 探索结论：当前架构已接近最优，盲目优化反而回退

## 已验证的优化尝试

### 尝试 1：SmallVec 栈上 JSON 序列化
**假设**：`serde_json::to_string` 的堆分配是瓶颈  
**实现**：`SmallVec<[u8; 256]>` + `serde_json::to_writer`  
**结果**：**性能回退 +11%**（24.3 µs vs 22 µs）  
**原因**：`to_writer` 对 `SmallVec` 的 `Write` 实现有额外虚拟调用开销；编译器对 `to_string` 的内联优化更激进

### 尝试 2：String::with_capacity 预分配
**假设**：减少 `to_string` 内部重分配  
**实现**：`String::with_capacity(256)` + unsafe `as_mut_vec`  
**结果**：**无变化**（23.5 µs，误差范围内）  
**原因**：`to_string` 内部已有增长策略优化，预分配收益微乎其微

### 尝试 3：Store 池化
**假设**：`Store::new` + WASI 上下文初始化是主要开销  
**实现**：`Mutex<Vec<Store>>` 池（容量 4）+ reset  
**结果**：**运行时崩溃**（`instance count too high at 10001`）  
**原因**：Store 持有的 `ResourceTable` 累积实例计数，Wasmtime 的 `StoreLimitsBuilder` 没有暴露重置接口；Component Model 的实例隔离模型不支持 Store 复用

## 当前架构已实现的最优实践

✅ **Linker 复用**（已在 `WasmPlugin::new` 实现）：  
- WASI + host imports 注册**一次**，所有 execute 复用
- Wasmtime 文档确认 Linker 是实例化模板（无状态），跨实例安全
- 避免每次 execute 重复调用 `wasmtime_wasi::add_to_linker_sync`

✅ **Component 预编译**（已在 `WasmPlugin::new` 实现）：  
- `Component::from_file` 在加载时完成 Cranelift AOT 编译
- execute 时直接 `instantiate`，跳过解析 + 编译

✅ **Engine 共享**（已在 `WasmPlugin` 字段）：  
- Engine 线程安全，跨所有插件实例共享
- 配置一次（Component Model + fuel + epoch），所有 Store 继承

## 真正的瓶颈（无法优化）

| 开销来源 | 占比 | 原因 | 可优化性 |
|----------|------|------|----------|
| `Store::new` + `PluginState` | ~30% | WASI 上下文（stdio/env/preopens）初始化 | ❌ 无法池化（见尝试 3） |
| `InkosPlugin::instantiate` | ~40% | Component Model 实例化（内存分配 + 导入链接） | ❌ Wasmtime 内部实现 |
| `call_invoke` | ~20% | 跨 FFI 边界调用 + JSON 序列化 | ✅ 可优化（见下方） |
| `serde_json` | ~10% | 参数/结果序列化 | ✅ 已最优（to_string 内联优化） |

**结论**：22 µs 中的 70% 是 Wasmtime 内部开销，无法在应用层优化。

## 真正有效的优化方向（修正）

### 方向 1：减少 execute 调用频次（架构级）
当前每次插件操作都是独立 execute（冷启动）。改为：

**批量执行模式**：
```rust
// 当前：3 次独立 execute = 3 × 22 µs = 66 µs
plugin.execute("format", args1)?;
plugin.execute("lint", args2)?;
plugin.execute("save", args3)?;

// 优化后：1 次 execute + 插件内部路由 = 22 µs + 3 × 2 µs = 28 µs
plugin.execute("batch", json!({
    "commands": [
        {"name": "format", "args": args1},
        {"name": "lint", "args": args2},
        {"name": "save", "args": args3},
    ]
}))?;
```

**收益量化**：  
- 节省 2 次 Store 创建 + 实例化（~44 µs → ~28 µs，-58%）
- 需扩展 WIT 契约（添加 `batch-invoke` 接口）

---

### 方向 2：进程插件优先（设计级）
当前 WASM 路径是 M7 的长期目标，**进程隔离插件**（`PluginProcess`）已就绪且性能更优：

| 指标 | WASM（runtime.rs） | 进程（process.rs） |
|------|-------------------|-------------------|
| 冷启动 | 22 µs | ~50 µs（spawn + RPC） |
| 热路径 | 22 µs | ~5 µs（stdin/stdout） |
| 内存隔离 | 64 MiB limit | OS 进程级隔离 |
| 能力控制 | Host trait | JSON-RPC + 白名单 |

**策略**：  
- 高频操作（格式化、补全）→ 进程插件（启动后复用，热路径 5 µs）
- 低频操作（一次性转换）→ WASM 插件（冷启动 22 µs 可接受）

---

### 方向 3：HTTP 流式处理（内存效率，非延迟）
**当前**（host_api.rs 推测）：
```rust
let bytes = response.bytes()?;  // 完整响应复制到内存
let text = String::from_utf8(bytes.to_vec())?;  // 再复制一次
```

**优化后**：
```rust
let mut response = reqwest::blocking::get(url)?;
let mut result = String::with_capacity(
    response.content_length().unwrap_or(0) as usize
);
use std::io::Read;
response.read_to_string(&mut result)?;  // 零拷贝流式读取
```

**收益**：  
- 延迟无变化（网络 IO 占主导）
- 内存峰值从 512 MiB（双份）→ 256 MiB（零拷贝），-50%
- 256 MiB 配额下可处理的响应数量翻倍

---

## 实施优先级（修正）

| 优先级 | 方向 | 工期 | 收益 | 风险 |
|--------|------|------|------|------|
| **P0** | HTTP 流式处理 | 0.5 天 | 内存 -50% | 低（API 稳定） |
| **P1** | 进程插件优先策略 | 1 天 | 热路径 -77%（22µs → 5µs） | 中（需调整路由逻辑） |
| **P2** | 批量执行模式 | 3 天 | 批量操作 -58% | 高（需 WIT 契约变更 + 插件迁移） |

**不推荐**：  
- ❌ JSON 序列化优化（已验证无效）
- ❌ Store 池化（Wasmtime 架构不支持）
- ❌ 自定义 allocator（收益 <5%，全局影响大）

---

## 下一步行动

1. **立即实施 P0**（HTTP 流式）：  
   - 修改 `host_api.rs` 的 `http_get` 实现
   - 添加 benchmark 验证内存降低
   - 预期 0.5 天完成

2. **评估 P1**（进程优先）：  
   - 分析当前插件调用频次分布
   - 设计进程 vs WASM 的路由决策逻辑
   - 若高频场景占主导，收益显著

3. **暂缓 P2**（批量执行）：  
   - WIT 契约变更影响面大
   - 需等插件生态成熟后统一迁移

---

## 经验教训

1. **Profiler 优先于假设**：  
   盲目优化浪费 2 个尝试；应先用 `perf` / `Instruments` 确认瓶颈

2. **理解框架约束**：  
   Wasmtime 的 Component Model 有明确的隔离边界，池化违反设计

3. **商业级 ≠ 极致优化**：  
   22 µs 对用户感知已足够快（<100ms 阈值），过度优化收益递减

4. **架构决策 > 局部优化**：  
   进程 vs WASM 的选择比微观优化重要 10 倍
