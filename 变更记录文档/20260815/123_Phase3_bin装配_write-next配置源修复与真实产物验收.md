# 123 号变更记录：bin 装配层 write-next 配置源修复 + 真实产物进程级验收

## 一、背景

117-121 对跑五轮覆盖 runbook 灰度五步，但均为**进程内** Rust 路由（`spawn_rust_engine` 直构 `router_books`）——**bin 装配层**（`src/bin/inkos-engine-server.rs` 的 runner 闭包接线）是盲区。复核发现真缺陷：

**bin 的 write-next runner 在启动时固化 env 构造的 `AgentRouter`**（`INKOS_LLM_*`，缺省 `http://127.0.0.1:9` 不可达），与其余写面端点（plan/draft/revise/daemon，均经 `BooksRuntime::effective_router()` 热解析 inkos.json 服务项 + secrets）**配置源分裂**。后果：实切条件②若以 inkos.json 服务项配置（非 env），write-next 端点必打不可达缺省端点而全链失败——**切换阻断级缺陷，且现有全部测试（进程内对跑 + E2E stub runner）结构性不可见**。

## 二、交付

### 1. 修复：bin write-next 配置源统一走 effective_router

- `books_routes::build_write_next_agents` / `build_write_next_ctx` 由 `pub(crate)` → `pub`（共享装配对 bin 开放）。
- bin runner 重写（净删 ~70 行手搓 Box::leak 装配）：复用共享装配 + **44 号全周期审计器 `FullCycleAuditor` 挂 `effective_router()` 结果重建**（共享装配缺省 `full_auditor: None` 会降级为最小协议审计——bin 原有完整审计能力保持不回退；`for_chapter` 按章重绑语义不变）。
- 语义：inkos.json 服务项 + secrets 优先，配置不可用回退 `INKOS_LLM_*` 启动端点——**与其余写面同源，bin 不再持有独立 LLM 配置路径**。

### 2. duel 第 6 测试：`bin_process_write_next_llm_resolution`（真实产物验收）

- `env!("CARGO_BIN_EXE_inkos-engine-server")` spawn **真实 bin 进程**（cargo 自动为集成测试构建 bin 产物）——补上进程内对跑的装配层盲区。
- fixture inkos.json 服务项 → 计数 mock A；`INKOS_LLM_BASE_URL` → 计数 mock B（反向证明端点）。
- 断言：health/books 装配冒烟 200；POST write-next 后 **mock A 触达 ≥1**（实测 6 次，链路首调用即 planner）且 **mock B 零触达**（TS studio 消费者忽略 env 语义）。
- **阴性对照**（121 号"以代码为准"纪律的延续）：临时回滚 bin 至修复前重跑——如预期 60s 超时失败（"write-next 未触达 inkos.json 服务端点"），证实测试对本缺陷的捕获力，随后恢复修复。

### 3. duel 基建增量

`spawn_counting_mock_llm`：双形态（流式 SSE / 非流式整体 JSON）回包 "OK" + 原子计数——不驱动链路走完，仅证明配置源指向。

## 三、验证基线（123 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1157 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | 182 过 |
| `cargo test --features export-bindings --lib` | 1316 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **6/6**（五步 runbook + bin 装配验收） |

## 四、对切换条件的影响

实切三项条件之①（Rust engine 起动方式）现具备**产物级验收**：`INKOS_PROJECT_ROOT` + `INKOS_PORT` + `INKOS_BUILTIN_GENRES_DIR` 起真实 bin，配置解析与路由服务已自动化证明。条件②（真实 LLM 端点）到位后 inkos.json 服务项路径即被本测试同构覆盖。duel 门由 5/5 → **6/6**（122 号 v4 文档中"duel 5/5 一键门"表述按本号更新为 6/6）。

## 五、下一步候选

1. 实切条件到位后真实流量切换（SOP 见 122 号 v4 runbook；duel 6/6 为前置门）。
2. 按需轮（用户指定功能缺口或缺陷修复）。
