# 122 号变更记录：迁移收官文档终版 v4（117-121 对跑资产并入 + runbook 自动化验收章）

## 一、背景

121 号候选首选：115 号 v3 后又完成对跑五轮（117-121）——runbook 灰度五步全部具备自动化双进程验收门，且实战捕获并修复 4 个真实契约分歧。本 v4 终版锁定切换 SOP 最终形态。

## 二、117-121 五轮增量（对 v3 的追加）

| 轮 | 交付 | 捕获/修复 |
|---|---|---|
| 117（2be2a516） | 只读面双端对跑（TS sidecar tsx 真实进程 + Rust 全量路由，10 端点） | —（runbook ①验收门） |
| 118（8b74a673） | 跨端写读对跑（Rust 写 approve / TS 写 review-mode + 写后复跑） | **review-mode 键缺失误 404**（TS 回退 auto） |
| 119（e639dde3） | 会话域对跑（双向建会话/改名 + TS 真实聊天回合） | —（sidecar 聊天流式偏好勘误记入注意项） |
| 120（0aeaffb4） | 模型配置域对跑（密钥跨端 + 深链探测逐字段） | **预设静态模型回退缺失** + **/test 候选构造偏差**（payload.model 不参选） |
| 121（af9bd91a） | 全量灰度模拟（四步串行 + 每步复跑） | **会话摘要 playMode 恒序列化**（TS 未设省略） |

## 三、runbook 自动化验收章（新增——切换 SOP 终态）

切换前后与本机均可一键执行：`INKOS_DUEL=1 cargo test --test strangler_duel`（约 1.2s，5 用例）：

| 用例 | runbook 步 | 断言 |
|---|---|---|
| `readonly_face_duel` | ① 只读面 | 10 端点状态码 + 结构化 JSON 等价（波动键归一） |
| `cross_write_read_duel` | ② 创作链 | 双向写后跨端可见 + 写后只读复跑等价 |
| `session_domain_duel` | ③ 聊天面 | 会话建/改名跨端 + 真实聊天回合后读面等价 |
| `services_domain_duel` | ④ 模型配置 | 密钥双向跨写读 + 深链探测逐字段等价 |
| `full_grey_simulation` | ⑤ 全量灰度 | 四步串行 + 每步双端只读复跑等价（sidecar 保温共存） |

排障三条（runbook 增补）：sidecar 对 inkos.json 严格 schema 校验（`version:"0.1.0"` 字面量必填）；前端 dist 缺失触发 vite 自动构建（对跑场景预置跳过）；sidecar 端口须 OS 分配（滞留进程跨根污染——基建已内置）。

## 四、切换 SOP 终态

1. **自动化验收**（本机随时）：duel 5/5 全绿为切换前置门。
2. **真实流量切换**（需用户三项条件）：Rust engine 起动方式 + `INKOS_PROJECT_ROOT`、真实 LLM 端点 inkos.json 服务项+secrets 或 `INKOS_LLM_*` env、前端 API base 切换许可——按 v3 第九章灰度五步执行，回滚 = API base 回切 + 快照仲裁。
3. **切换后保温**：sidecar 共存共读（⑤已验证共存语义），72h 观察后下线。

## 五、验证基线（122 号时点，本轮无代码变更，引 121 号实测）

lib **1157** / golden 76 / E2E **182** / bindings **1316** / clippy 零警告 / TS **1798** / duel **5/5**。

## 六、终局结论（v4）

迁移开发与**自动化可验证的切换演练**均已完成：九个契约面（v3）+ runbook 五步双进程验收（本轮）全绿；实战对跑捕获的 4 个契约分歧全部修复并回归。剩余唯一事项为真实流量切换（用户三项条件）或按需轮（用户指定方向）。
