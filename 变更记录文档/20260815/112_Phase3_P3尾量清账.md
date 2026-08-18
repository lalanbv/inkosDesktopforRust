# 112 号变更记录：P3 尾量清账（qualityGates / foundation.reviewRetries / writing 双旋钮 + 回放治理输入）

## 一、背景

111 号候选之二。P3 剩余小参数面一轮清：**qualityGates**（daemon 三常量 → config）、**foundation.reviewRetries**（92 号备案）、**writing.reviewRetries + reviewMode**（from_project 未读配置）、**回放治理输入**（91 号备案 #2——import 回放逐章 prepareWriteInput）。

## 二、交付

### 1. qualityGates 配置位（`ops_routes.rs`）

- `DaemonConfig.quality_gates: QualityGates`（inkos.json `qualityGates` 节，models 已备 schema——补 `Default` derive）；三常量（MAX_AUDIT_RETRIES=2 / PAUSE=3 / STEP=0.1）换配置值（缺省与 TS `QualityGatesSchema.default` 一致；pause 下限 1）。

### 2. foundation.reviewRetries（`book_create_routes.rs`，92 号备案闭合）

- `foundation_review_retries(root)` 助手（0-10 截断，缺省 2——TS FoundationConfigSchema）；`generate_and_review_foundation_import` 与 `generate_and_review_foundation_multi`（建书面——TS buildPipelineConfig 的 `foundationReviewRetries` 同源）均参数化，两调用点传配置值。

### 3. writing 双旋钮（`WriteNextConfig::from_project`）

- `writing.reviewRetries`（0-10，缺省 1）→ `writing_review_retries`；`writing.reviewMode`（manual/auto，缺省 auto）→ `chapter_review_mode`——五个写作面（确认/聊天/REST/bin/daemon）经 from_project 统一生效（TS buildPipelineConfig 同源；REST draft 面的 Manual 覆盖在展开序后保持优先）。

### 4. 回放治理输入（`import_chapters_chain_with_resume`，91 号备案 #2 闭合）

- Step 2 回放循环逐章构造 v2 治理输入：`prepare_write_input`（plan 持久化复用链 + composer 三件）→ analyzer 入参 `chapter_intent / context_package / rule_stack`（原 None）——TS importChapters 的 `prepareWriteInput` 同构；回放章 `story/runtime/chapter-NNNN.plan.md` 工件与写作链同形。

### 5. E2E（`sub112_e2e`，2 测试）

- `writing_review_mode_manual_config_reaches_confirmed_write`：inkos.json `writing.reviewMode=manual` → 确认式 write_next 写完即停（audit-failed + "审稿未通过"需复核文案）——配置位到写作链全链生效证据。
- `import_replay_persists_governed_plan_artifacts`：续放导入第 2 章 → `chapter-0002.plan.md` 工件落盘（治理输入进回放的行为证据）。

## 三、parity 要点

- 四配置位缺省值与区间截断逐字对齐 TS schema（2/3/0.1、0-10×2、manual|auto）；回放治理输入复用写作链同一 `prepare_write_input`（plan 复用语义自动继承）。

## 四、偏差备案

1. **inputGovernanceMode 配置位未读**（from_project 恒 V2——TS studio 默认 v2；legacy 为 CLI 兼容模式，Rust 无消费面）。
2. **revisionGate 配置位维持 bin env**（`INKOS_REVISION_GATE`——runtime 级装配，热配置升级件后续评估）。

## 五、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1156 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **178** 过（+2：sub112） |
| `cargo test --features export-bindings --lib` | 1315 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 六、影响面与下一步（113 号候选）

P3 清单仅剩：pi-ai 模型卡（无消费面维持）、聊天卡 details 外露/SSE 结构化（前端契约前提）、同步钩子族（markdown→json 反向同步——settler 直写已覆盖主数据流）、authoring 边角（无端点消费）。候选：

1. **首选：strangler 实切演练**——待用户提供运行/流量条件（Rust engine 起动方式 + 真实 LLM 端点 inkos.json/env + 前端 API base 切换许可）。
2. 其次：迁移总结终版 v2（103 审计 + 106-112 七轮增量的合订修订）。
3. 或：按需轮。
