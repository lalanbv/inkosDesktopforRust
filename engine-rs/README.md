# inkos-engine（Rust 业务引擎移植目标）

> packages/core（TS，~107k 行 / 16 域）→ Rust 的渐进式移植落点。
> 阶段：**Phase 0 骨架**（模块占位 + 类型同步通道 PoC）。
> 规划依据：`../开发时SpecCoding'sPlan/inkosDesktop/01_设计文档/全面Rust化迁移规划_v1.md`

## 现状（Phase 0）

- 16 个业务域模块占位（`src/{agent,agents,forecast,...,utils}.rs`），与 `packages/core/src` 一一对应
- `src/models.rs` 为 **canonical 类型真源**，含 ts-rs 类型导出 PoC
- crate 级 `EngineError`（thiserror，对齐壳层 AppError 模式）
- 技术栈与 `src-tauri` 壳层对齐：serde / serde_json / anyhow / thiserror / tokio / tracing

## 用法

```bash
# 默认编译（最小依赖，无 TS 导出）
cargo build

# 单元测试
cargo test                          # → 2 passed

# 类型同步 PoC：生成 TS 绑定到 ./bindings/
cargo test --features export-bindings
#   → 生成 bindings/ChapterStatus.ts、BookMeta.ts
#   → 前端 import 后，Rust 改类型 → 重新生成 → tsc 报错（单一真源不漂移）
```

## 移植纪律（每域）

1. **自下而上**：按依赖图（见规划附录 A），先叶子（utils/translation/models）→ 中层 → 高层 + agent
2. **golden 差分**：每域移植配 `tests/golden/*.json` 向量，`assert_eq!(rust_out, ts_out)`
3. **1:1 复刻**：提示词模板、状态机、退避算法逐字搬运，禁「顺手优化」
4. **零功能丢失**：每域接入前，端点契约测试（同 HTTP 请求打 Rust 与 Node，响应 diff=0）
5. **上下文来源优先级契约**（G2/330 号）：Selected Context 组装序 == 优先级序
   （本书事实 100 > 本书规划 80 > 本书记忆 60 > 参考资料 40 > 拆书 30 > 写法 20 > 临时 10），
   走 `utils::context_source_tier::enforce_context_priority_order` 固化；参考资料及更低层
   是"仅供参考"，不得覆盖本书事实与章纲。新增 ContextSource 来源必须先在
   `context_source_tier()`（双端）注册层级，未注册一律 ephemeral 垫底。共享向量：
   `packages/core/src/__tests__/golden/context-priority-vectors.json`。

## 接入方式（Phase 1+）

两条路（规划 §4）：
- **库依赖**：`src-tauri/Cargo.toml` 加 `inkos-engine = { path = "../engine-rs" }`，Tauri 命令直接调
- **HTTP 服务**：独立 axum 进程挂载已迁端点，保持 `/api/v1/*` 契约，前端无感切换（推荐，strangler）

## 不做（Phase 0 边界）

- 不实现任何业务逻辑（仅占位 + 类型骨架）
- 不接入 src-tauri（避免影响现有桌面端构建）
- 不生成最终 TS 绑定到前端目录（PoC 仅产到本 crate 的 bindings/）

## 下一步（Phase 1 启动条件）

待 ADR-001（fork-and-own）确认后：
1. 固化 `packages/core` 的 golden 测试向量到 `tests/golden/`
2. 移植 `utils`（纯函数，最易，建立移植模板）
3. 移植 `models`（类型真源，ts-rs 全量导出）
