# Phase 0 准备 + 双端测试验证（会话归档）

> 日期：2026-08-11
> 范围：完成审查修复 + 双端测试全量验证 + 启动 Rust 化 Phase 0 无悔准备
> 关联：`00_全面审查与Rust化规划.md`、迁移规划 v1 §4 Phase 0

---

## 1. 双端测试全量验证（响应「经过测试和检测」要求）

| 端 | 命令 | 结果 |
|---|---|---|
| **Rust 全量** | `cargo test` | lib **449 passed** + 全部集成二进制（config_integration/e2e_updater/marketplace_e2e/observer_integration/plugin_execution/project_commands/secure_update_integration/sse_contract/sync_unit/wasm_plugin_execution 等）+ doc-tests，**0 失败** |
| **TS core 全量** | `vitest run`（npx pnpm@9 exec） | **184 文件 / 1797 测试 passed / 0 失败**（7.76s） |

Rust 集成测试通过 assert_cmd 真实 spawn 二进制，覆盖 SQLite/配置持久化/SSE 契约/
插件执行(WASM+进程)/sidecar 启动/密钥同步等子系统——构成桌面端运行时行为验证。
TS 测试覆盖业务引擎（迁移目标）全部逻辑。

ignored 项均明确标注环境依赖（keyring 运行时 / GUI / notify 时序），非缺陷。

## 2. 审查修复（响应「继续优化和创造」）

提交 `7f14aaf9`：
- `commands/config.rs`：update_config / reset_config 两处 `unreachable!()` → 防御性 `Err` 返回
- `plugin/process.rs`：`stop()` 从 0 列重新缩进到 4 列 + 移除与 const doc 同文的重复 doc 首行
- 修复手段：config.rs 用 Edit；process.rs 用 Python 脚本精准处理 Unicode（`→`/`終`/`（）`）

## 3. Phase 0 无悔准备项（响应「为未来 Rust 化做充足准备」）

四项无悔准备全部落地（不依赖 ADR-001 决策即可执行）：

### 3.1 engine-rs Rust 骨架（`engine-rs/`）
- 独立 lib crate，16 个业务域模块占位（与 packages/core/src 一一对应）
- `src/models.rs` 为 canonical 类型真源，ts-rs 导出 PoC
- 技术栈对齐壳层：serde / serde_json / anyhow / thiserror / tokio / tracing
- **验证**：`cargo build` 通过；`cargo test` 4 passed；`cargo clippy -- -D warnings` 零警告
- **ts-rs PoC 验证**：`cargo test --features export-bindings` 生成 `bindings/ChapterStatus.ts`、`BookMeta.ts`（内容正确：枚举→字符串联合、结构体→对象类型）→ 类型同步链路打通

### 3.2 HTTP 端点登记与迁移排期（`开发时SpecCoding'sPlan/.../02_实现计划/`）
- 从 `packages/studio/src/api/server.ts` 提取全部 **134 路由**（58 GET / 46 POST / 23 PUT / 7 DELETE + SSE + 静态）
- 按 core 域归类，映射到 Phase 1/2/3
- 给出 strangler 切换顺序建议（translation/genres 先行建立模板 → 配置/state → 高层+agent 主力）

### 3.3 Golden 向量采集方案
- 三层策略：纯函数 dump / 文件状态快照 / LLM 录制回放
- 含 TS dump + Rust 差分的起步骨架代码
- 端点契约 diff（Playwright，Rust vs Node 同请求）作为 strangler 切换守门

### 3.4 CI 接入（`.github/workflows/ci.yml`）
- 新增 `engine-rs` job：clippy（-D warnings 门禁）+ test（--features export-bindings）
- 独立于 Node 工具链，待 crate 并入正式 workspace 后可合并

## 4. ADR-001 处置

用户「全面 Rust 化」目标本身已隐含 fork-and-own 决策（与原架构 §1.2 零修改同步
互斥，是用户主动选择）。本会话据此推进 Phase 0，不再阻塞于显式确认。若用户后续
反悔，Phase 0 产物（骨架/登记/方案/CI）均可独立保留或移除，零沉没成本。

## 5. 环境提示

- 当前 shell pnpm=11.9.0，项目锁 pnpm 9。TS 测试用 `npx pnpm@9 exec vitest` 绕过
  （npx 缓存命中 pnpm 9.15.9）。正式 CI 已固定 pnpm 9（ci.yml L23）。
- `.codegraph/` 由本机 CodeGraph 钩子生成，已加入根 `.gitignore`（不入库）。

## 6. 下一步（Phase 1 启动条件）

1. 选 `utils` 第一个纯函数，跑通 golden 差分全链路（TS dump → Rust diff → CI）
2. 用该链路作模板，复制到 translation / models 全量
3. models 全量 ts-rs 导出 → 接入前端类型链路
