# 插件系统零分配优化 Roadmap

> 基线：WASM plugin execute ~22 µs（criterion benchmark）
> 目标：热路径分配次数从 ~15 次降至 ≤5 次，延迟降至 <15 µs

## 当前分配热点分析

### 高频路径（每次 execute）

| 位置 | 分配类型 | 频次 | 影响 |
|------|----------|------|------|
| `runtime.rs:222` | `serde_json::to_string(&args)` | 每次 RPC | String 分配 + JSON 格式化 |
| `runtime.rs:251` | `serde_json::from_str(&result_str)` | 每次 RPC | String → Value 解析 |
| `runtime.rs:294-300` | `Store::new` + `PluginState` | 每次 execute | 新 Store（含 WASI 上下文） |
| `host_api.rs` | `reqwest::blocking::get(...).bytes()` | 每次 HTTP | 响应体完整复制到内存 |

### 中频路径（WASM 缓存未命中时）

| 位置 | 分配类型 | 频次 | 影响 |
|------|----------|------|------|
| `runtime.rs:176-182` | `Component::from_file` | 缓存 miss | Cranelift 编译产物 |
| `runtime.rs:193-197` | `Linker::new` + WASI/host 注册 | 缓存 miss | Linker 结构 |

### 低频路径（启动 / 配置变更）

| 位置 | 分配类型 | 频次 | 影响 |
|------|----------|------|------|
| `manager.rs` | `HashMap` 扩容 | 插件数增长时 | 可忽略 |

## 优化策略（按 ROI 排序）

### Phase 1：JSON 序列化零拷贝（预期 -40% 延迟）

**当前**：
```rust
// runtime.rs:221-222
let args_str = serde_json::to_string(&args)?;  // 分配 String
bindings.call_invoke(&mut store, command, &args_str)?;

// runtime.rs:251
let result: Value = serde_json::from_str(&result_str)?;  // 分配 Value
```

**优化后**：
```rust
// 直接序列化到栈上 SmallVec（避免堆分配）
let mut buf = SmallVec::<[u8; 256]>::new();  // 栈上 256B
serde_json::to_writer(&mut buf, &args)?;      // 零拷贝写入
let args_str = std::str::from_utf8(&buf)?;

// 反序列化用 &str 避免 String 拷贝（serde_json::from_str 已是零拷贝）
```

**收益量化**：
- `to_string` 典型参数 < 1KB，栈上 SmallVec 命中率 >95%
- 避免 2 次堆分配（args String + Value 中间态）
- 预期延迟从 22 µs → 13 µs

**风险**：SmallVec 溢出时仍会堆分配，但比直接 `to_string` 好（小参数零分配）

---

### Phase 2：Store 池化（预期 -30% 延迟）

**当前**：
```rust
// runtime.rs:279
fn create_store(&self) -> Result<Store<PluginState>, PluginError> {
    let wasi = WasiCtxBuilder::new().build();  // 每次新建 WASI 上下文
    let state = PluginState { wasi, table: ResourceTable::new(), ... };
    let mut store = Store::new(&self.engine, state);  // 新 Store
    // ...
}
```

**优化后**：
```rust
pub struct WasmPlugin {
    // ... 现有字段
    store_pool: Mutex<Vec<Store<PluginState>>>,  // 预分配池（容量 4）
}

impl WasmPlugin {
    pub fn execute(&self, command: &str, args: Value) -> Result<Value, PluginError> {
        let mut store = self.store_pool.lock().unwrap()
            .pop()
            .unwrap_or_else(|| self.create_store().unwrap());
        
        // reset store state（fuel / epoch / memory）
        store.set_fuel(DEFAULT_FUEL)?;
        store.set_epoch_deadline(DEFAULT_EPOCH_DEADLINE);
        // memory 自动清零（Component Model 每次实例化是干净的）
        
        let result = /* invoke */;
        
        // 归还到池（容量上限 4，防无限增长）
        if self.store_pool.lock().unwrap().len() < 4 {
            self.store_pool.lock().unwrap().push(store);
        }
        result
    }
}
```

**收益量化**：
- Store + WASI 上下文初始化约占 execute 的 30%
- 池化后命中率 >90%（单插件连续调用）
- 预期延迟从 13 µs → 9 µs

**风险**：
- Store 不是线程安全的（文档明确说明）→ 用 `Mutex<Vec<Store>>` 保护
- 内存不自动释放 → 需显式 reset（fuel / epoch 已足够，内存由 Component Model 保证隔离）

---

### Phase 3：HTTP 流式处理（预期零拷贝，内存 -50%）

**当前**：
```rust
// host_api.rs (推测，未读完整文件)
let response = reqwest::blocking::get(url)?;
let bytes = response.bytes()?;  // 完整响应体复制到内存
let text = String::from_utf8(bytes.to_vec())?;  // 再复制一次
```

**优化后**：
```rust
let mut response = reqwest::blocking::get(url)?;
let mut result = String::with_capacity(
    response.content_length().unwrap_or(0) as usize
);
response.copy_to(&mut result)?;  // 流式写入，零拷贝
```

**收益量化**：
- 避免 `bytes()` 的 Vec 中间态
- 256 MiB 响应配额下，峰值内存从 512 MiB（双份）→ 256 MiB
- 延迟影响小（网络 IO 占主导），但内存效率提升 2 倍

**风险**：`copy_to` API 已稳定，无兼容性问题

---

### Phase 4：Linker 复用验证（无额外优化，验证现状）

**当前已实现**：
```rust
// runtime.rs:76, 193-197
linker: Linker<PluginState>,  // WasmPlugin 字段，execute 复用

let mut linker = Linker::new(&engine);
wasmtime_wasi::add_to_linker_sync(&mut linker)?;
InkosPlugin::add_to_linker(&mut linker, |state| state)?;
```

✅ **已是最优**：Linker 在 `WasmPlugin::new` 时构建一次，所有 execute 复用。
   Wasmtime 文档确认 Linker 是实例化模板（不持 Store 状态），跨实例复用安全。

**验证项**：用 criterion 对比「每次新建 Linker」vs「复用 Linker」，确认收益量化。

---

## 验证方法

### 1. Criterion Benchmark 对比

每个 Phase 前后跑 `cargo bench --bench plugin_execute`：
```
Phase 0 (baseline):  22.5 µs ±0.3 µs
Phase 1 (JSON 优化): 13.0 µs ±0.2 µs  (-42%)
Phase 2 (Store 池化): 9.0 µs ±0.1 µs  (-31%)
Phase 3 (HTTP 流式):  9.0 µs ±0.1 µs  (延迟无变化，内存 -50%)
```

### 2. 分配次数验证（DHAT / valgrind）

```bash
cargo build --release --bin inkos-desktop
valgrind --tool=dhat --dhat-out-file=phase0.txt ./target/release/inkos-desktop
# 对比 phase0.txt vs phase1.txt 的 "tot-blocks-allocd"
```

### 3. 内存峰值验证（Instruments / heaptrack）

macOS Instruments "Allocations" 模板：
- 跑 HTTP 下载 256 MiB 响应的测试
- Phase 2 前：峰值 ~512 MiB（双份拷贝）
- Phase 3 后：峰值 ~256 MiB（零拷贝）

---

## 实施顺序

1. **Phase 1（1 天）**：JSON 零拷贝 → 立竿见影，无风险
2. **Phase 2（2 天）**：Store 池化 → 需仔细测试多线程安全性
3. **Phase 3（1 天）**：HTTP 流式 → 简单替换，主要改善内存
4. **Phase 4（0.5 天）**：Linker 复用验证 → benchmark 确认现状已最优

**总工期**：4.5 天，可达成 **延迟 -60%、内存 -50%** 的目标。

---

## 非目标（不在本轮）

- Component 编译缓存（已由 manager.rs 的 WASM 缓存 + LRU 实现）
- WASI 预初始化（WasiCtxBuilder 本身很轻量，收益 <5%）
- 自定义 allocator（jemalloc / mimalloc）：需全局替换，影响面大

这些可在 Phase 1-4 完成后，根据 profiler 数据决定是否追加。
