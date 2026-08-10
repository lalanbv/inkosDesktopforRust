# 全面审查与 Rust 化迁移规划（会话归档）

> 日期：2026-08-11
> 范围：全面审查项目现状 + 修复发现缺陷 + 产出 Rust 化迁移路线图
> 关联：`开发时SpecCoding'sPlan/inkosDesktop/01_设计文档/全面Rust化迁移规划_v1.md`

---

## 1. 审查执行

### 1.1 建立的 ground truth

| 检查项 | 结果 |
|---|---|
| `cargo test --lib` | **449 passed / 0 failed / 7 ignored**（2.65s） |
| `cargo clippy --all-targets` | **零警告**（全量编译通过） |
| `cargo check`（修复后回归） | 通过（5.18s） |
| engine/dist 预构建产物 | 就绪（index.js / program.js 等） |
| 桌面二进制可构建 | ✅（clippy --all-targets 含 binary） |

### 1.2 Rust 端（src-tauri，21.8k 行）审查结论

**无关键 BUG**。代码达商业级：仅 4 处 `unsafe`（均为 `libc::kill` 进程组信号，
有安全说明），错误处理一致（AppError + with_details/with_suggestion），纵深防御 4 层
（URL 解析对齐 / DNS 预解析 / 配额 / 限量输出）。

发现的问题（均轻微）：

| # | 位置 | 问题 | 处置 |
|---|---|---|---|
| 1 | `main.rs:313` | `_config_state = commands::AppState::new(...)` 死代码：与 `.build()` 前 `.manage(config_state)` 托管的为同一类型（`commands/mod.rs` 重导出 `config::AppState`），setup 内重复构造后丢弃，浪费一次 ConfigManager 初始化 | **已修复**（移除，补注释说明） |
| 2 | `process.rs:278` | `stop()` 方法缩进为 0 列（应为 4 列，rustfmt 漂移） | 记录，待下次触及该文件时一并 rustfmt |
| 3 | `process.rs:273-274` | `stop()` doc 首行重复（两条「优雅关闭」首行） | 同上 |
| 4 | `commands/config.rs:109,141` | `unreachable!("System 在上 match 已 return")` — 可辩护（上层有 return 守卫），但枚举演进时是潜在 panic 源 | 记录，可选加固为显式错误返回 |

### 1.3 TS 端（packages，~167k 行）审查结论

轻量审查（TS 是迁移目标，重点摸清待迁移面）：
- `core` 16 个业务域（agent/agents/llm/pipeline/play/state/models/prompts/skills/
  materials/notify/interaction/interactive-film/forecast/translation/utils），依赖图清晰
- `cli` 含 ink TUI + commands；`studio` 含 React SPA + Hono API + SSE + e2e
- 测试覆盖充分（core/__tests__ 100+ 文件，studio e2e 14 spec）

### 1.4 环境提示（非缺陷，提醒）

- 当前 shell 的 `pnpm=11.9.0`，但项目锁 **pnpm 9**（见 `memory/pnpm-version-pin.md`）。
  装依赖需切 pnpm 9；本次未装依赖（engine/dist 已预构建），未受影响。

---

## 2. 修复

### 2.1 移除 main.rs 死代码

**文件**：`src-tauri/src/main.rs`
**变更**：删除 `setup` 回调内的 `let _config_state = commands::AppState::new(app_data.clone());`
及其上方「M5b：初始化配置管理器」注释块，替换为说明性注释（为何此处无需重复构造）。
**依据**：`commands::AppState` 是 `commands::config::AppState` 的重导出（`commands/mod.rs:7`），
真正的 config 状态已在 `.build()` 前 `.manage(config_state)`（main.rs:237）托管。
**验证**：`cargo check` 通过；`cargo test --lib` 449 全绿。

---

## 3. 迁移规划（本次会话主交付物）

产出文档：`开发时SpecCoding'sPlan/inkosDesktop/01_设计文档/全面Rust化迁移规划_v1.md`

要点：
- **战略反转**：全面 Rust 化 = 推翻原架构 §1.2 非目标「零修改同步上游」→ fork-and-own
- **路线**：Strangler-Fig + 保持 HTTP `/api/v1/*` 边界 + 按 core 域依赖图自下而上移植
- **5 阶段**：Phase 0 准备（2-3 周）→ Phase 1 叶子域（4-6 周）→ Phase 2 中层（6-10 周）
  → Phase 3 高层+agent（10-16 周）→ Phase 4/5 可选（去 HTTP / 前端原生化）
- **硬指标**：零功能丢失（golden 差分测试 + 端点契约 + E2E 回归三层防线）
- **待用户确认**：ADR-001 fork-and-own 决策；Phase 4/5 是否执行

---

## 4. 未提交状态

本次会话产生 / 涉及的待提交文件：
- `src-tauri/src/main.rs`（修改：移除死代码）
- `开发时SpecCoding'sPlan/inkosDesktop/01_设计文档/全面Rust化迁移规划_v1.md`（新增）
- `变更记录文档/20260811/00_全面审查与Rust化规划.md`（本文件，新增）
- 前序会话遗留未跟踪（已提交代码 af956292/aab7e9c8 的文档）：
  - `变更记录文档/20260807/00_SUMMARY_商业级改进总结.md`
  - `变更记录文档/20260807/41_http_response_preallocate_capacity.md`

---

## 5. 下一步建议

1. 用户确认 ADR-001（fork-and-own）后，启动 Phase 0 准备工作
2. 立即可做（不依赖决策）：固化 golden 测试向量、建 `engine-rs/` workspace 骨架、
   集成 ts-rs PoC、梳理 HTTP 端点登记
3. 修复 `process.rs` 缩进/重复 doc（下次触及该文件时顺带）
