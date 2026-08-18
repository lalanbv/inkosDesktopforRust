# 120 号变更记录：模型配置域对跑（runbook 第四步）——捕获并闭合探测候选/回退双分歧

## 一、背景

119 号候选首选：对跑第四步（模型配置域）——services 列表、secrets 跨端写读、深链探测（同 mock 上游）双端行为对跑。96 号备案 #2（models 失败详情静默）与 99 号备案 #1（doctor 候选族）的对跑级验证。

## 二、交付

### 1. `strangler_services_domain_duel`（`strangler_duel.rs` 增 1 测试）

- **A** 服务列表读面双端对比（差异面观察输出）；**B** 密钥跨端写读双向（Rust 写 custom:Duel2 → TS 读到；TS 写 custom:Duel3 → Rust 读到）；**C** 深链探测（inline baseUrl 同 mock、无偏好计划）双端响应**逐字段等价**。

### 2. 对跑捕获两处真分歧并修复（TS 逐字）

- **预设静态模型回退缺失**：/models 不可达且探测成功时，TS 返回预设清单（endpoint.models/knownModels，`modelsSource:"fallback"`、`modelCount` 同步、models 为 `{id,name}` 对象数组）；Rust 原返回空列表+`api`。修复：`fallback_text_models`（TS `fallbackTextModelsForEndpoint` 逐字）+ 探测成功路径回退感知 + models 形态统一。
- **候选构造偏差**：TS `/test` **不消费 `payload.model`**——候选 = `checkModel ?? knownModels[0] ?? endpoint.models 首启用`（+ 配置命中服务的 defaultModel/model + discovered 前 2，`useEndpointCheckModel` 时不带 discovered）；Rust 原把 payload.model 作首选候选。修复：候选构造 TS 逐字重排；96/106 号两处 E2E 断言随语义更新（96 号：selectedModel=checkModel + fallback 清单；106 号：svc-r 无预设无候选 → TS 同款 400，改用 deepseek 验证 responses 回退探测）。

### 3. 实跑结果

`INKOS_DUEL=1 cargo test --test strangler_duel`：**4/4 通过**（只读 / 跨端写读 / 会话域 / 模型配置域——runbook 前四步全部有双进程验收门）。

## 三、验证基线（120 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1157 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | 182 过（96/106 两处随 TS 语义更新） |
| `cargo test --features export-bindings --lib` | 1316 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **4/4** |

## 四、下一步（121 号候选）

runbook 五步中前四步（只读/创作链写读/聊天会话域/模型配置域）均已有自动化双端验收门且当前全绿——**切换演练的自动化可验证部分已完备**。候选：

1. **首选：对跑第五步（全量灰度模拟）**——按 runbook 顺序在单测试内串行执行四步全部场景 + sidecar 保温语义（同根双进程共存读）作为终验收套件。
2. 或：实切条件到位后真实流量执行（用户三项条件）。
3. 或：按需轮（用户指定）。
