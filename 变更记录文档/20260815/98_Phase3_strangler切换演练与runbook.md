# 98 号变更记录：strangler 切换演练与 runbook（P1 全闭合后的切换前基线）

## 一、背景

97 号闭合 P1-1 后，94 号差分扫描确立的 strangler 切换 **P1 阻断项全部清除**（P1-1 前端模型四层解析 → 97 号；P1-2 attachments 多模态注入 → 95 号）。本轮（98 号）为切换前演练轮（勘测+文档，代码零改动）：刷新基线状态、以 52 个 E2E 模块为流量脚本输出对跑清单、给出切换 runbook 与回滚路径，并实跑六项基线作为演练证据。

## 二、演练实跑（本轮证据）

全量六项基线实跑（2026-08-18）：

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1138 过 |
| `cargo test --test golden_leaf` | 76 过（golden 向量差分） |
| `cargo test --test e2e_write_next_contract` | **162 过 / 52 模块** |
| `cargo test --features export-bindings --lib` | 1297 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 过（契约源健康） |

## 三、94 号基线复核更新

| 项 | 94 号状态 | 98 号状态 |
|---|---|---|
| 端点覆盖 | 107/107（3 条通配符语法等价） | 不变；Rust 超集 4 条（health/settle/utils×3，62 号备案） |
| P1-1 studio 模型配置域 | 缺（四层解析 + 探测特判族） | **闭合**：深链探测+诊断族（96 号）、四层解析+per-request 覆盖（97 号）、无 key 直通/check model 优先/缓存键（63 号既有） |
| P1-2 attachments 多模态 | 缺（注入面） | **闭合**（95 号：vision 数组 + 双语清单块） |
| P2 doctor transport 回退细节 | 未做 | 未做（ops72 覆盖主干；首探测空响应 → 非 stream 回退的特化路径） |
| P2 PDF 抽取 | 未做 | 未做（TS 用 unpdf；Rust 需独立选型轮） |
| P2 单章写作中途截断 | 未做 | 未做（写作链无内建检查点；多章间轮询 abort 已有） |
| P3 聚合 | — | 层 3 secrets 迭代序（排序近似插入序）、provider 特判统一 OpenAI 形态、responses 协议传输（深链以 chat 探测）、pi-ai 模型卡元数据、若干双语日志面——各轮偏差备案在案 |

## 四、流量对跑清单（52 E2E 模块 → 流量域分桶）

以 `e2e_write_next_contract.rs` 的 52 个模块为切换前的流量脚本（每模块 = 一族真实 `/api/v1/*` 请求序列，含 mock 上游）：

| 流量域 | 模块（号） | 覆盖面 |
|---|---|---|
| **长篇创作链** | books46-53（revise 审核环/gate 三档/analytics/eval/export/detect/create-status）、agent67/68（确认面直跑/abort/BOOK_BUSY） | plan/settle/draft/revise/rewrite/audit/export/analytics/truth/chapters CRUD/versions/trash/resync |
| **聊天面** | agent65/66、propose84、research85、import85/91/92、material83、details86、sub87-90（sub_agent 五代理+forecast 三件）、sub93/95/97（model 校验/attachments/四层解析）、play80-82 | /agent 全参数面 + 11 件聊天工具 + 注册矩阵真值表 + 多模态 |
| **互动小说** | play71/73/79（step/reconcile/regenerate/restore）、image74（Play 图像/路径穿越） | play 全回合链 + 图像服务 |
| **衍生创作** | fanfic59（fanfic/spinoff/imitation）、translations70/75、script76、films69/77、short78 | 衍生五族 init/run/export |
| **模型配置** | services63（12 测试：bank 分组/探测/secret/config/env 导入/cover）、sub96（深链+诊断族） | services/models/test/secret/config/import-env + doctor 邻接 |
| **会话与系统** | sessions64（CRUD/abort/校验族）、config54/56、style55、skills60、projectfiles61、ops72（doctor/radar/daemon） | sessions/config/style/skills/files/ops |
| **类型差分** | golden_leaf（76 向量） | Rust vs TS golden 输出逐字节 |

**对跑判据**：任一模块红 = 切换阻断；全绿 = 该流量域可切。

## 五、切换 runbook

### 前置 gate（切换当日）

1. 六项基线全绿（第二节命令清单，逐项执行并留存输出）。
2. TS 侧 `pnpm`（或既有方式）启动 Node sidecar 备用（回滚目标）。
3. 磁盘快照/备份 `.inkos/`（secrets/uploads/transcripts）与 `books/`（strangler 双端同格式读写，快照用于回滚后一致性仲裁）。

### 切换步骤（灰度序）

1. **只读面先行**：前端 API base 切 Rust 引擎的 `/api/v1/books*`、`/project*`、`/sessions*`（GET 族）——对跑清单"会话与系统"桶护航。
2. **创作链**：POST 族（plan/write/audit/revise/import）切 Rust——"长篇创作链 + 衍生创作"桶护航；观察 SSE 事件流（revise:start/complete 等广播事件名与 Node 同名）。
3. **聊天面**：`/api/v1/agent` 切 Rust——"聊天面"桶护航；重点观察 409 单任务闸、abort、四层解析覆盖态的模型行为。
4. **模型配置面**：services/models/test 切 Rust——"模型配置"桶护航；用户已配 key 的服务逐个 `test` 验证。
5. 全量切换后 Node sidecar 保温 72h（随时可回切），再下线。

### 观察指标

- 错误率：5xx 占比与错误文案形态（ApiError `{error:{code,message}}` 面）。
- SSE：事件名与序（tool:start/background 标记、agent:complete）。
- 磁盘产物：章节/索引/真相文件/snapshots 的落盘形态（双端同源格式）。

### 回滚路径

- **无状态迁移**：双端共享同一磁盘格式（books/.inkos），回滚 = 前端 API base 切回 Node sidecar，无需数据转换。
- 触发判据：切换域对应对跑桶出现红、或线上 5xx 超阈值、或 SSE 事件面缺失。
- 回滚后动作：diff 双端对该域的最近写入（磁盘快照对照），人工仲裁冲突文件；`git revert` 引擎侧问题提交后重跑六项基线再切换。

## 六、暂缓件（滚动，不阻断切换）

- P2 三件：doctor transport 回退细节、PDF 抽取选型、单章截断。
- P3：层 3 secrets 保序、provider 特判族、responses 传输、模型卡元数据。

## 七、影响面与下一步（99 号候选）

1. **首选：P2 清单逐件之 doctor transport 回退**（最小件：首探测空响应 → 非 stream 探测回退——ops72 已有 doctor 主干，补特化路径 + E2E）。
2. 其次：PDF 抽取选型轮（pdf-extract vs lopdf 自实现文本层；评估 unpdf 抽取质量差分可行性）。
3. 或：单章写作中途截断（写作链检查点信号设计——涉及 writer 编排本体，规模较大）。
