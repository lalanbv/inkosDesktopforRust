# 111 号变更记录：Scheduler 精简面收口（webhook 通知族 + detection 自动改写环，72 号备案闭合）

## 一、背景

110 号候选之二。72 号 Scheduler 精简面备案的最后两件可独立开发件：**webhook 通知**（notify/dispatcher——TS dispatcher.ts 102 行 + 四通道发送器）与 **detection 自动改写环**（detection-runner.ts 163 行——detect → anti-detect 重写 → 重测循环）。

勘测要点：TS 通知有**双分发路径**——`dispatchNotification`（通用文本通知，writeNextChapter 章末触发，经 buildPipelineConfig 的 notifyChannels 直通 studio 全部写作面）与 `dispatchWebhookEvent`（结构化事件，Scheduler 暂停 pipeline-error + 失败维度聚类 diagnostic-alert）；detection 环为 Scheduler 类能力（config.detection.enabled 门控；studio 启动装配当前未转发——Rust 按类语义经 inkos.json `detection` 节直驱，见偏差备案）。

## 二、交付

### 1. `notify/dispatcher.rs`：四通道发送器 + 双分发（TS 逐字）

- **通道类型复用 models/project 的 `NotifyChannel`**（63 号 schema 已备——本轮勘测发现并复用，避免重复定义）；`parse_notify_channels` 解析 inkos.json `notify` 数组（非法条目剔除）。
- **发送器**：`send_telegram`（api.telegram.org sendMessage，markdown 才带 parse_mode）/ `send_feishu`（text 消息 vs interactive 蓝卡）/ `send_wechat_work`（msgtype text/markdown）/ `send_webhook`（**事件订阅过滤 + HMAC-SHA256 签名头 `X-InkOS-Signature: sha256={hex}`**，sha2+hmac）。
- **分发**：`dispatch_notification`（markdown/纯文本双形态按通道派发；webhook 型以 pipeline-complete 事件携带 title/body/format）+ `dispatch_webhook_event`（只发 webhook 型）；失败只记 stderr 不抛（TS "notification failure shouldn't block pipeline"）。

### 2. write_next 章末通知接线（TS "6. Send notification" + emitWebhook 逐字）

- `WriteNextConfig.notify_channels` + `from_project(root)` 装配（inkos.json notify 数组）。
- 章末双发：文本通知（emoji 标题 `{🧯|✅|⚠️} {book.title} 第N章` + 正文行 `**章题** | N字` / `📝 已自动修正` / 审稿行 / 非 info 问题行）+ `pipeline-complete` 结构化 webhook（title/wordCount/passed/revised/status）。
- 五个写作面统一 `from_project`：确认面 execute_write_next、聊天面 sub_agent writer、REST run_draft、bin、daemon write_one_chapter。

### 3. `pipeline/detection_runner.rs`：检测自动改写环（TS 逐字）

- `detect_chapter`（score ≤ threshold 判过）；`detect_and_rewrite`：首测过阈 → 记 detect 历史；不过 → 循环（reviser **AntiDetect 模式** + AIGC检测 issue 文案逐字 → 重测 → 记 rewrite 历史）至过阈或耗尽 maxRetries。
- `record_history`：追加 `story/detection_history.json`（pretty 形态；读取面 52 号已有——写入面本轮补齐）。
- api key 经 `config.apiKeyEnv` 环境变量解析（TS 同）。

### 4. daemon 接线（ops_routes）

- `DaemonConfig` 增 `detection`（inkos.json `detection` 节）+ `notify_channels`；`WriteCycleState` 增 `failure_dimensions`。
- 成功审计后 `run_detection`（读章文 NNNN*.md → detect → 不过且 autoRewrite → 环）；错误 → daemon:error（TS onError 同）。
- **失败维度聚类**（任一维度 ≥3 次 → `diagnostic-alert` webhook 事件）+ **暂停 pipeline-error webhook**（`{reason, consecutiveFailures}`）。
- `write_one_chapter` 返回审计 issue 类别（聚类原料）+ 配置换 `from_project`。

### 5. 测试

- **单测 4**：HMAC 已知向量（RFC 4231 case 1）、四通道解析（含非法剔除）、payload camelCase 序列化、history 追加落盘。
- **E2E 2（sub111）**：① 确认式 write_next + webhook 通道 → **双发断言**（通知事件 data.title 含书名章号/正文含章题 + pipeline-complete 事件 bookId/chapterNumber/wordCount/status）+ **每个捕获报文的 HMAC 签名头与原始报文重算一致**；② daemon + detection 配置 → 三次 detect（调度首测 0.9 → 环首测 0.9 → anti-detect 改写 → 重测 0.3）→ history 恰一条 rewrite 记录（attempt 1 / score 0.3 / provider custom）。daemon 单例并行冲突加启动重试。

## 三、parity 要点

- 四通道请求形态（Telegram parse_mode / 飞书蓝卡 / 企微 msgtype / webhook HMAC+事件过滤）、通知标题正文构造（emoji/字数格式/问题行过滤 info）、事件载荷字段、detection 环结构与历史条目键全部 TS 逐字。

## 四、偏差备案

1. **detection 环装配位**：TS Scheduler 类支持 config.detection，但 studio daemon 启动装配当前未转发该字段（装配面遗漏——studio 侧休眠）；Rust 按类语义经 inkos.json `detection` 节直驱（用户显式启用即生效——能力面等价超集，行为差仅在"studio 端配置了 detection 也不跑"）。
2. **通知失败可观测性**：stderr 打印（TS process.stderr.write 同构）；无重试（TS 同）。
3. **qualityGates 配置位**（maxAuditRetries 等）仍为常量（72 号既有备案维持——本次仅补聚类维度与事件）。

## 五、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1156** 过（+4） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **176** 过（+2：sub111；连续两次全绿） |
| `cargo test --features export-bindings --lib` | **1315** 过（+4） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 六、影响面与下一步（112 号候选）

72 号 Scheduler 备案清单全清（detection 环 / webhook 通知 / 聚类维度 / cron 精确对齐已备案维持 / daemon:chapter 已于 104 号闭合）。P3 剩余仅：pi-ai 模型卡（无消费面）、聊天卡 details 外露（前端契约前提）、同步钩子族、回放治理输入、评审轮数配置位、authoring 边角。候选：

1. **首选：strangler 实切演练**——仍待用户提供运行/流量条件（Rust engine 起动方式 + 真实 LLM 端点 inkos.json/env + 前端 API base 切换许可）。
2. 其次：P3 尾量清账轮（qualityGates/foundationReviewRetries/writing.reviewRetries 三配置位 + 回放治理输入——全是小参数面，一轮可清）。
3. 或：按需轮。
