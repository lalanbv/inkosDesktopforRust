# 99 号变更记录：doctor transport 回退（P2-1 闭合）

## 一、背景

98 号变更记录"下一步"首选：doctor transport 回退细节——TS 用例 "auto-falls back to a non-stream probe in doctor checks when the first transport returns empty"。勘测确认语义在 TS `probeServiceCapabilities`（doctor 与 test 端点共用）：models 不可达 → chat 深链 → `buildProbePlans` preferred 分支（preferredStream=true 时先 stream 后非 stream；**空响应视为失败**触发回退）。Rust doctor（ops72）此前仅 models GET 单跳（3s 预算），96 号已把深链建在 test_service 内但 doctor 未复用。

## 二、交付

### 1. `service_routes.rs`：`minimal_chat_probe` 增强 + 提权

- 返回类型 `Result<(), String>` → `Result<String, String>`（返回响应文本）；**空响应（trim 后空）判失败**（`probe returned an empty response`）——对齐 TS doctor 用例的"首传输空 → 回退下一计划"语义（对 96 号深链同为行为增强）。
- 提 `pub(crate)` 供 doctor 复用；test_service 调用处同步（`Ok(_content)`）。

### 2. `ops_routes.rs::get_doctor`：探测链升级

- **级联**：models GET（radar 端点）成功即真 → 不可达时 chat 深链：候选 = 端点模型；计划 = inkos.json `llm.stream`（preferred stream）→ `[true, false]`（首传输空/失败回退非 stream），缺省 `[false]`；任一计划成功即真。
- **总预算 3s → 9s**（对齐 `DOCTOR_LLM_PROBE_BUDGET_MS`——深链两计划各 8s 上限在 9s 总预算内被截断，慢/限流上游按未连接上报的 TS 语义保持）。

### 3. E2E（`mod sub99_e2e`，2 测试）

- `doctor_falls_back_to_non_stream_when_stream_probe_empty`：models 404 + stream 探测空 + 非 stream 返回 "OK" → `llmConnected=true`，且 chat 端点调用序恰为 `[stream=true, stream=false]`（回退路径的行为证据）。
- `doctor_reports_disconnected_when_all_transports_empty`：两计划皆空 → `llmConnected=false`（两计划都试过才判未连接）。

## 三、parity 要点

- 判定链级序（models → 深链计划序）、空响应判失败、preferred stream 回退序、9s 总预算逐项对齐。

## 四、偏差备案

1. doctor 深链候选仅端点模型（TS probeServiceCapabilities 有 discovered/check/config 候选族）——doctor 路径 models 已不可达，discovered 为空，configModel 为可选增强。
2. aggregator 静态信任分支（test 路径有）在 doctor 不参与（TS doctor 同样经 probeServiceCapabilities 有该分支——Rust doctor 的 radar 端点为项目配置端点，aggregator 判定不适用；差异仅当 radar 端点属 aggregator 且 /models 挂时，备案）。

## 五、暂缓件（滚动）

- P2 剩余两件：PDF 抽取选型、单章写作中途截断。
- P3：层 3 secrets 保序、provider 特判族、responses 传输、模型卡元数据。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1138 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **164** 过（+2：sub99） |
| `cargo test --features export-bindings --lib` | 1297 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（100 号候选）

P2 清单仅剩两件。下一轮候选：

1. **首选：PDF 文本抽取选型轮**（pdf-extract vs lopdf 自实现文本层；以 unpdf 抽取的 fixture 差分验证；落定后闭合 83 号备案——ingest_material 的 pdf 分支从固定错误文案变为真抽取）。
2. 其次：单章写作中途截断（写作链检查点信号设计——writer 编排本体改造，规模最大）。
3. 或：P3 精修批（secrets 保序/responses 传输）。
