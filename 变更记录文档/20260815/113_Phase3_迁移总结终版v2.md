# 113 号变更记录：迁移总结终版 v2（103 审计 + 106-112 七轮增量合订）

## 一、背景

110 号总结终版后又完成两轮（111-112）。会话内可独立开发项至此**穷尽**——P3 剩余四项全部为"无消费面/需外部契约/已被等价覆盖"的维持备案（见第四节）。本记录为合订终版，此后唯一推进方向是 strangler 实切演练（需用户环境）。

## 二、106-112 七轮增量合订

| 轮 | 交付 | 闭合 |
|---|---|---|
| 106（d21f3594） | responses 协议传输全链 + 探测 apiFormat 维度 + env 对账表 | 96 #1 |
| 107（333338db） | prompt-pack 消费面收口（play/film）+ env 三项勘误 | 74/76/77/82 族；103 #1 勘正 |
| 108（e1a7b9ec） | stream 维度生产面 + chat 非流式 JSON 分支 + CLI env 族终版 | 传输对称收口 |
| 109（6caa698e） | 运行时配置装配（effective_router 热解析 43 位点） | 62 号"重启生效" |
| 110（9bda9cf9） | 迁移总结 v1 + runbook 修订三条 | 文档里程碑 |
| 111（215d13f7） | notify 四通道+HMAC webhook+章末通知 + detection 自动改写环 + daemon 聚类/暂停事件 | 72 号备案全清 |
| 112（c2a3ce23） | qualityGates/foundationReviewRetries/writing 双旋钮 + 回放治理输入 | 91 #2、92 #2 |

## 三、能力面终版（对 110 号 v1 的增补）

- **通知体系**：四通道（telegram/飞书/企微/webhook+HMAC）双分发路径；章末通知五写作面统一（from_project）；daemon diagnostic-alert/pipeline-error 事件。
- **检测体系**：detector（52 号）+ 自动改写环（anti-detect reviser + history 读写闭环 + insights 聚合）全链。
- **配置体系**：四层解析（/agent）+ 热装配（其余面）+ 全配置旋钮（daemon 调度/qualityGates/detection/notify/foundation/writing×2/notify 通道/apiFormat/stream）。
- **回放治理**：import 逐章 v2 治理输入（plan 工件与写作链同形）。

## 四、P3 终态清单（全部维持备案，无可开发项）

| 项 | 状态依据 |
|---|---|
| pi-ai 模型卡元数据（contextWindow 等） | Rust 流式层无消费面（97 #3 维持） |
| 聊天卡 details 外露 + SSE tool:end 结构化 | 需前端消费契约确认（#9 维持） |
| 同步钩子族（markdown→json 反向同步） | settler 直写 state/*.json 已覆盖主数据流（#10 维持） |
| authoring-store 边角（revertToSnapshot 等） | 无端点消费面（#14 维持） |

另：inputGovernanceMode 配置位（Rust 恒 V2——studio 默认同）、revisionGate 热配置（bin env 维持）、provider 特判族（95 架构备案）、CLI env 族（108 终版）。

## 五、验证基线（113 号时点，本轮无代码变更，引 112 号实测）

`cargo test --lib` **1156** / golden_leaf 76 / E2E **178** / export-bindings 1315 / clippy 零警告 / TS 185 文件 **1798**。

## 六、终局结论

**迁移开发完成**：功能面、协议面（chat/responses × 流式/非流式 × 探测/生产）、配置面（四层+热装配+全旋钮）、提示词面（双语逐字+prompt-pack 全消费点）、中止体系、通知/检测体系——与 TS sidecar 的已备案差异全部收敛到上表四项维持备案。

**下一步唯一推进方向：strangler 实切演练**（98 号 runbook 只读面起跑），需要用户环境条件：
1. Rust engine 起动方式（`cargo run --release -p inkos-engine-server` 或打包产物 + INKOS_PROJECT_ROOT）；
2. 真实 LLM 端点配置（inkos.json 服务项 + secrets，或 INKOS_LLM_* env）；
3. 前端 API base 切换许可。

具备后按灰度五步执行：只读面 → 创作链 → 聊天面 → 模型配置 → 全量 + sidecar 保温 72h；回滚路径为 API base 回切 + 快照仲裁（双端同磁盘格式）。
