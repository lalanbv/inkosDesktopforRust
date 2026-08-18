# 116 号变更记录：revisionGate 配置链闭合（按需轮——112 号备案 2 升级落地）

## 一、背景

verifier 指示：实切条件未备时走按需轮。选定方向：115 号终态三项维持备案中**唯一有行为面的**——revisionGate 热配置（112 号备案 2：TS `buildPipelineConfig` 读 `writing.revisionGate`（book 级覆盖 project 级），Rust 生产链只吃 bin env `INKOS_REVISION_GATE`，inkos.json/book.json 配置不生效）。`models::book::resolve_revision_gate`（book ?? project ?? strict 逐字）已在模型层备好但**零生产消费**——勘测确认。

另两项终态备案复核：pi-ai 模型卡（无消费面）、authoring phase 标记（74 #2——TS `authoring-state.json` 的 phase 字段无任何工具条件消费，纯 bookkeeping；Rust 69 号 rev/phase 保持面已备）——维持成立。

## 二、交付

### 1. `resolve_effective_revision_gate(runtime, book_id)`（`books_routes.rs`）

- 解析链（TS 逐字）：`book.writing.revisionGate ?? inkos.json writing.revisionGate ?? runtime.revision_gate`（bin env / 缺省 strict）；复用模型层 `resolve_revision_gate`（book 覆盖 project）+ `RevisionGateVal → RevisionGate` 映射。

### 2. 消费点接入

- REST `/books/:id/revise/:chapter`（books_routes:416）与聊天面 sub_agent reviser（sub_agent_tool:340）换解析值；`/rewrite` 端点的显式 `RevisionGate::Always` 注入不变（TS 同款）。

### 3. E2E（`books47_e2e` 增 1）

- `revise_book_level_always_gate_overrides_project_default`：47 号 strict 拒绝场景（审计恶化 blocking 1→3）+ book.json `writing.revisionGate="always"` → **同输入下 applied=true 且修订文本落盘**——book 级覆盖链的行为证据（应用路径无 diagnostics——88 号拒绝路径专属形态，断言随契约修正）。

## 三、parity 要点

- 三级解析链与 TS `buildPipelineConfig`（project）+ book writing 覆盖语义逐字；缺省 strict；未知值回退 strict（模型层既有）。

## 四、偏差备案

1. 确认面 reviser 无独立意图（TS 同——reviser 走聊天 sub_agent/REST 面，均已接）。
2. `INKOS_REVISION_GATE` 仍为 runtime 启动级兜底（链末位）——与 TS env 位次一致。

## 五、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1157 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **181** 过（+1：gate 覆盖链） |
| `cargo test --features export-bindings --lib` | 1316 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 六、影响面与下一步（117 号候选）

115 号终态三项中 revisionGate 闭合；余两项（pi-ai 模型卡、authoring phase）经本轮复核确认为零行为面/零消费——**P3 清单至此无可开发项**。唯一推进方向：**strangler 实切演练**，待用户环境条件（Rust engine 起动方式 + 真实 LLM 端点 inkos.json/env + 前端 API base 切换许可）。按需轮可接受指定方向（具体功能缺口或缺陷修复）。
