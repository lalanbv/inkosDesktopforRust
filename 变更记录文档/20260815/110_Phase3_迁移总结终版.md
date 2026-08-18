# 110 号变更记录：迁移总结终版（30-109 号全链回顾 + 切换 runbook 修订）

## 一、背景

109 号候选之二。103 号收官审计后又完成四轮（106-109），配置/协议/提示词三面收口——在 strangler 实切演练（需真实环境）之前，以终版总结文档锁定迁移状态。

## 二、103 号审计后的增量（106-109 号）

| 轮 | 交付 | 闭合备案 |
|---|---|---|
| 106（d21f3594） | **responses 协议传输全链**：streaming_client /responses 传输（input/instructions/SSE 事件/终态守卫）+ buildProbePlans apiFormat 维度 TS 逐字 + 97 层/bin 服务项 apiFormat 接线 + INKOS env 族对账表 | 96 号 #1（探测一律 chat）|
| 107（333338db） | **prompt-pack 消费面收口**：play mutator/renderer + film draft_structure 附加段（项目级 prompt 覆盖生效）+ INKOS_FILM_IMAGE_SIZE；三项对账勘误（prompt-pack 体系实已移植、SKILL_DIRS 已接、USER_AGENT 两侧常量） | 74/76/77/82 号附加段族；103 审计 #1 勘正为已闭合 |
| 108（e1a7b9ec） | **stream 传输维度生产面**：AgentRouter.with_stream → 全端点流式偏好 + chat 传输非流式 JSON 分支（message/tool_calls）+ bin INKOS_LLM_STREAM（parseBoolean 逐字）+ 97 层服务项 stream 直通；CLI env 族维持备案终版落档（studio 模式 env 忽略为 TS 设计，Rust 无 CLI 消费面） | 传输维度对称收口（协议 × 流式 × 探测/生产） |
| 109（6caa698e） | **运行时配置装配**：`BooksRuntime::effective_router`（resolve_effective_llm_studio 热解析 + mtime/启动态指纹缓存 + apiFormat/stream 透传）→ 非 agent 面 43 位点清扫 + AuditRuntime 委托；配置写入即时生效 E2E | 62 号"重启生效"备案 |

## 三、迁移能力面终版

- **端点面**：107/107 对齐（94 号口径；Rust 超集 4 条备案）。
- **聊天面**：工具注册矩阵 TS 逐字（105 号）+ sub_agent 五代理 + 编辑六件 + forecast 三件 + play 三件 + 文件三件书域化 + material 双件；确认面 11 意图执行器全接 + write 启发式。
- **传输层**：chat/responses × 流式/非流式 × 探测/生产全对称；多模态 vision 注入（chat 协议）。
- **配置面**：97 层 per-request 四层解析（/agent）+ 109 热装配（其余全 face）+ 63 号 studio 有效配置 + secrets 保序（104）。
- **中止体系**：write_next 四安全点 + import 三安全点 + 多章轮间 + 聊天轮/任务注册表 + scope 族。
- **提示词面**：zh/en 双语逐字族（writer/renderer/mutator/forecast/play…）+ prompt-pack 附加段全消费点 + 确认面诊断文本族。
- **材料域**：PDF 真抽取（100）+ attachments 归一化/注入。
- **P3 余量**（不阻断）：provider 特判族（95 架构备案）、pi-ai 模型卡（无消费面）、Scheduler 精简面余量、聊天卡 details 外露/SSE 结构化（前端契约前提）、同步钩子族、回放治理输入、评审轮数配置位、authoring 边角、56 号 CLI env 族（无消费面终版）。

## 四、切换 runbook 修订（98 号 + 106-109 增补）

98 号灰度五步与无状态回滚路径不变，增补三条注意：

1. **responses 协议端点**：inkos.json 服务项 `apiFormat:"responses"` 的端点现由 Rust 原生支持（106）——切换前无需改配置；探测面会自动回退验证（chat 失败 → responses）。
2. **非流式端点**：服务项 `stream:false` 现全程生效（108）——不支持 SSE 的端点可直接切换。
3. **配置生效语义**：非 agent 面与 /agent 面均为运行时热解析（109）——切换后改配置无需重启 Rust engine；bin env（INKOS_LLM_*）仅作无 inkos.json 时的启动兜底。

回滚路径不变：双端同磁盘格式，API base 回切 + 快照仲裁。

## 五、验证基线（110 号时点，本轮无代码变更）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1152 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **174** 过（sub37-sub110 模块族） |
| `cargo test --features export-bindings --lib` | 1311 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 六、下一步（111 号候选）

1. **首选：strangler 实切演练**——**需用户环境条件**：桌面端/CLI 起 Rust engine（`cargo run --release -p inkos-engine-server` 或打包产物）+ 指向真实 LLM 端点的 inkos.json（或 INKOS_LLM_* env）+ 前端 API base 切换许可。具备后按 98 号 runbook 只读面起跑（books/世界/材料/日志读面），对跑验证后逐桶放量。
2. 其次：P3 剩余精修（Scheduler webhook/detection 自动环、聊天卡 details 外露——后者需前端契约确认）。
3. 或：按需轮（实切中发现的具体缺口按优先级即时处理）。
