# 103 号变更记录：迁移收官审计（30-102 号偏差备案全量销账 + 94/98 基线终版 + 切换就绪度结论）

## 一、背景与审计方法

102 号候选首选：迁移收官审计轮。触发条件已满足——P1（97 号闭合）/P2（100/101 号清零）全清、检查点族（101 write_next 四安全点 + 102 import 三安全点）闭合、11 确认意图执行器全接。

方法：`变更记录文档/20260815/` 37-102 号共 66 份记录的偏差备案/暂缓件全量提取 → 逐条在当前代码核实（关键符号 grep / 执行器分支 / 装配点现查）→ 三分类：**已销账**（后续轮闭合，标注闭合轮）、**维持备案**（等价决策，非缺口）、**仍开放**（P3，附处置建议）。

## 二、销账清单（备案 → 闭合轮）

| 备案来源 | 内容 | 闭合轮 |
|---|---|---|
| 54 | PUT /default-model `syncTopLevelLlmMirror` 顶层镜像 | 63（`service_routes.rs:199/555`，本轮核实） |
| 63 | /test 的 chatCompletion 深链暂缓 | 96 |
| 63 | proxyUrl 探针代理 | 93（reqwest 直连等价备案，销账为等价决策） |
| 64 | POST /agent 整体暂缓 | 65/66（agent loop 主路径） |
| 64 | restore 的 agent 重放面 | 68（restoreAgentMessagesFromTranscript） |
| 64 | abort 恒 false | 65/68（聊天轮注册表 + 确认任务注册表） |
| 65 | 工具调用面暂缓 | 66（read/ls/grep 起步）→ 84-90（全工具族） |
| 65 | 模型四层解析暂缓 | 97 |
| 65 | attachments 归一化暂缓 | 93（归一化）+ 95（vision 注入） |
| 65 | surfaceLanguage 推断暂缓 | 67（surface_language 链） |
| 66 | 确认式生产任务分支暂缓 | 67 |
| 66 | restoreAgentMessages 历史回放暂缓 | 68 |
| 67 #1 | 9 个确认意图未接线 | **全接**（short_run/generate_cover 78、script/storyboard 78、interactive_film 77/78、translation 78、play_start 67、draft_structure/connect_choice/remove_node 74；本轮核实 `is_confirmed_production_action` 11 意图集与 TS L1403 逐字一致，write_next 经启发式归一入确认面） |
| 67 #2 | 单章写作中途不可截断 | 101（四安全点检查点） |
| 67 #3 | actionPayload 非 strict 校验 | 75（zod strict 逐字） |
| 67 #4 | model 校验缺失 | 93（八片段非文本模型 400 双语） |
| 67 #5 | abort 端点未接确认任务注册表 | 68（find_running_task_controller 链） |
| 69 #1 | 生图链 503 | 71（node-image 接线） |
| 69 #4 | draft_structure/connect_choice/remove_node 执行器 | 74 |
| 70 #1 | pdf 源不支持 | 100（pdf-extract + lopdf） |
| 73 #1/#2 | sceneReconciler / regenerate 变体面 | 79（reconciler + replayContext） |
| 73 #3 | 三代理 en 提示词 | 82（en 逐字；zh 91 号逐字） |
| 73 #5 | play_step/play_revise/play_edit 聊天工具面 | 80/81 |
| 79 #3 | renderer zh 标签措辞 | 91（TS zh 逐字对齐） |
| 83 #1 | PDF 抽取暂缓 | 100 |
| 85 #1/#2 | resumeFrom>1 续放 / importMode=series | 91 / 92（评审环） |
| 87 #1/#2 | reviser / architect.revise 暂缓 | 88（五模式 + revision gate） |
| 91 #1 | series 评审环 | 92 |
| 93 #2 | attachments 注入面 | 95（ChatImage vision 数组） |
| 101 #2 | import 链检查点未接 | 102（三安全点） |

## 三、维持备案（等价决策——非缺口，切换不阻断）

1. **错误文案近似**（48/49/50/51/52/53/55/56/58/59/83/84 等）：TS `String(e)` 原生长文案 vs Rust 结构化短文案——状态码 / 错误码 / SSE 事件面全部一致，仅 message 文本非逐字。
2. **垃圾输入防御性收窄**（48/50/53/55/88/89/93）：TS 穿透 NaN/undefined/TypeError 的路径，Rust 以等义错误拦截（如 base64 严格解码、非整数章号过滤）——取安全侧。
3. **键序**（54/69/70/76）：serde_json BTreeMap 字母序 vs TS 插入序——JSON 键序无语义，前端按键索引。
4. **排序降级**（69 #2/83 #3）：localeCompare 拼音序 vs 码点序——同名首字不同拼音场景顺序有差，展示层。
5. **UTF-16 近似**（70 #4/73/84/89/93）：`chars().count()` vs JS `.length`（UTF-16 码元）——BMP 内等价，BMP 外字符展示计数有 1 vs 2 差。
6. **架构决策**：41 文件锁→进程内 per-book tokio 锁（strangler 分域下不并发写同书）；47 EPUB 字节级（结构合规即可）；95 多模态统一 OpenAI vision 形态；100 抽取质量差分以 fixture 行为对齐为准。
7. **67 #6/#7**：manualToolAssistantMessage 的 provider/model 深链（97 号四层已部分改善，configuredEntry 链维持）；agent:start 广播时序（装配后，65 号起既有）。

## 四、仍开放清单（P3——全部不阻断切换）

| # | 项 | 来源 | 现状核实 | 处置建议 |
|---|---|---|---|---|
| 1 | prompt-pack 附加段体系 | 74/76/77/82 | 提示词整体未拼项目 promptPack 指引 | 切换后按需（无前端消费契约） |
| 2 | 56 env 层请求级合并 | 56 | bin 端点级 `INKOS_LLM_*` 已接；GET /project 请求级 resolveEffective 合并维持 | 桌面单仓无差异；多环境部署前补 |
| 3 | responses 传输协议 | 96 | preferred=responses 时 Rust 用 chat 超集探测（chat 通则判通） | 协议层扩展轮（与 #5 同轮） |
| 4 | 层 3 secrets 服务迭代序 | 97 | HashMap 按名排序 vs TS JSON 插入序 | P3 精修（保序 BTreeMap/索引序） |
| 5 | provider 特判族 | 95 | Anthropic 原生 image 块等——统一 OpenAI vision | 与 #3 同轮 |
| 6 | pi-ai 模型卡元数据 | 97 | contextWindow/compat 不参与 Rust 流式层 | 无消费面，维持 |
| 7 | Scheduler 精简面 | 72 | webhook 通知 / detection 自动改写环 / cron 精确对齐 / daemon:chapter 恒 0（`ops_routes.rs:257` 核实） | 常驻进程域，切换后 |
| 8 | 聊天工具 schema 中文简述 | 66 | read/ls/grep description 中文（`project_tools.rs:174-199` 核实） | 低风险提示词面 |
| 9 | 聊天卡 details 外露 + SSE tool:end 结构化 result | 66/84/85/86 | 聊天面恒带 details.toolExecutions 且卡内只含 result 文本；tool:end 只带文本 | 前端消费契约确认后补 |
| 10 | 同步钩子族 | 47/51 | markdown→json 反向同步钩子未接（settler 直写 state/*.json 已覆盖主数据流） | 维持 |
| 11 | 回放 governed input | 59/91 | import 回放分析器治理入参传 None | 既定收窄 |
| 12 | foundationReviewRetries 配置位 | 92 | 固定 2 轮 | 低频 |
| 13 | play_language 内容判定 | 73 | `agent_production.rs:1885` 默认 zh；utils/infer_language 已备未接 | 一行接线轮 |
| 14 | authoring-store 边角面 | 69 #5 | revertToSnapshot/recordPhaseVisit 无端点消费 | 维持 |

## 五、94/98 基线终版刷新

- **端点面**：107/107 对齐（94 号口径不变；3 条为 Hono `:param{.+}` vs axum `*param` 通配符语法等价；Rust 超集 4 条：health/settle/utils×3——62 号备案）。
- **意图面**：11 确认意图执行器全接 + write_next 启发式归一（本轮核实与 TS `isConfirmedProductionAction` 逐字同集）。
- **中止体系**：write_next 链内四安全点（101）+ import 链内三安全点（102）+ 多章轮间轮询（67）+ 聊天轮注册表（65/68）+ abort 端点 scope 族（68）——域内闭合。
- **多模态/模型域**：attachments 归一化 + vision 注入（93/95）+ 模型四层解析（97）+ 服务深链探测与诊断（96/99）——P1 全闭合。
- **材料域**：PDF 真抽取（100）闭合 83 号备案。
- **E2E 资产**：52 模块 / 167 用例（sub37-sub102，除 94 纯文档轮）。

## 六、切换就绪度结论

**结论：功能面达到 strangler 切换就绪**。P1/P2 全清；P3 清单逐项核实的结论为——或无前端消费契约（#1/#9）、或行为超集/等价（#3/#6/#10）、或桌面单仓无差异（#2）、或切换后域（#7）、或低风险提示词面（#8/#11/#12/#13/#14）、或与协议扩展同轮（#4/#5）。

98 号 runbook（灰度五步：只读面 → 创作链 → 聊天面 → 模型配置 → 全量 + sidecar 保温 72h）与无状态回滚路径（双端同磁盘格式、API base 回切 + 快照仲裁）仍有效；本审计为切换前文档终点。

## 七、本轮附带交付：sub102 偶发根治

102 号基线观察到的 1 例负载偶发失败，本轮**并发复现定位**：`sub102` 的 `second_seen` 标记位在**第一次**分析（第 2 章）到达时置位，而第 2 章落盘发生在其后——负载下 fs 写入慢于测试轮询唤醒即闪断（"第 2 章应已落盘"断言失败）。修法：标记因果后移——mock 内计数分析调用，**第二次**分析（第 3 章）到达才置位（彼时回放循环已推进过第 2 章全部落盘点，因果必然成立）。验证：3 次「E2E 全量 + lib 全量并发负载」复跑全绿（E2E 167×3 + lib 1141×3），单独跑亦稳定。

## 八、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1141 过（含 3 次并发负载全绿） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | 167 过（含 3 次并发负载全绿；偶发已根治） |
| `cargo test --features export-bindings --lib` | 1300 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 九、下一步（104 号候选）

1. **首选：P3 一行接线批**——play_language 接 `utils::infer_language`（#13）+ daemon:chapter 真实章号（#7 的最小件）+ 层 3 secrets 保序（#4，BTreeMap 插入序）：三件小接线一轮清，P3 清单再缩。
2. 其次：strangler 实切演练——按 98 号 runbook 只读面起跑（真实流量分桶验证）。
3. 或：聊天卡 details 外露 + SSE tool:end 结构化 result（#9，需先与前端确认消费契约）。
