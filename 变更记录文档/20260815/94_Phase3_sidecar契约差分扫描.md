# 94 号变更记录：sidecar 契约差分扫描（strangler 切换前收尾基线）

## 一、背景

93 号变更记录"下一步"首选：以 TS 集成测试清单为基线，对已移植 `/api/v1/*` 端点做系统对齐盘点，输出缺口清单并按风险排序。本轮为勘测+文档轮（代码零改动），产物是 strangler 切换前的收尾基线。

## 二、扫描方法与结果

### 1. 端点层差分（脚本比对）

- **TS 侧**：`packages/studio/src/api/server.ts`（Hono）多行正则提取——132 条路由定义 / **107 条唯一路径**。
- **Rust 侧**：`engine-rs/src/server/mod.rs`（axum 路由挂载）——112 条路径。
- **差分结论**：
  - **TS 有 Rust 无：0 条实际缺口**。名义差异 3 条（`/books/:id/truth/:file{.+}`、`/project/artifacts/:file{.+}`、`/project/files/:file{.+}`）均为 Hono 参数约束语法 `:param{.+}` 与 axum 通配符 `*param` 的**等价写法**，实现已存在。
  - **Rust 超集 4 条**（62 号端点对照清单已备案）：`GET /health`（探活）、`POST /books/:id/settle`（Node 侧为 pipeline 内部函数，Rust 为编排调试暴露）、`POST /utils/derive-book-id|count-length|cap-context`（Rust 专属基础设施）。
- 探测域（services/models/test/secret/config/import-env、doctor、spinoff/imitation、inspiration、resync、review-mode 等）两侧路径**逐一全对齐**。

### 2. 行为层映射（TS 156 条集成用例 → Rust E2E）

TS `server.test.ts`（6484 行 / 156 用例）按行为域映射到 Rust `e2e_write_next_contract.rs` 的 **48 个 E2E 模块**（45-93 号逐轮随移植建立）：

| TS 行为域（用例组） | Rust 验证证据 |
|---|---|
| daemon lifecycle / doctor / radar 持久化 | ops72（doctor 检查 + radar 全链 + daemon 生命周期） |
| truth 读写白名单 / 只读诊断 / 配置错误 | books45-53 域 + config54/56 |
| services 配置/密钥/探测族（~35 条） | services63（bank 分组/custom models probe/test 探测路径/secret 校验/env 导入/删除/cover 配置） |
| book create 重复 id / create-status / LLM 错误面 | books58 / agent67 / books53 |
| 章节拒绝回滚 / briefs / plans / versions / restore / resync / inspiration | books49-52 / workspace E2E |
| export / analytics / eval / detect | books47 / ops72 |
| sessions CRUD / abort / 校验族 | sessions64 / agent65/68 |
| /agent 主链 / confirmed 直跑 / 生产任务持久化 / 409 单任务闸 / abort scope | agent65-68 / short78（确认流） |
| write-next 直跑 / 多章 / audit-failed / BOOK_BUSY | agent67/68（含"写下一章"白名单归一） |
| play 全链（start/step/edit/revise/reconcile/图像/路径穿越） | play71-82 / image74 |
| chapter-review-mode per-book / revisionGate 三档 | books47（gate 拒绝诊断 E2E）+ write_next config 链 |
| 翻译全链 / 短篇 / 剧本 / 互动影游 | translations70/75 / short78 / script76 / films69/77 |
| fanfic / spinoff / imitation | fanfic59（含 spinoff/imitation init 端点 E2E） |
| model 校验 / attachments | sub93（93 号） |
| forecast / 编辑工具族 / 导入续放与 series | sub89-92 |

## 三、缺口清单（按风险排序）

### P1（strangler 切换阻断项）

1. **studio 模型配置域行为族**（93 号备案延续）：/agent 模型四层解析（前端 service+model → defaultModel → secrets 探测 → legacy）依赖 `resolveServiceModel`/`listModelsForService`/`loadSecrets` 域本体；services63 已覆盖主干探测，但 TS 特判分支（Ollama/LM Studio 无 key 直通、Google 400 专属诊断、模型列表按 baseUrl 缓存键、bank check model 优先于全局默认、空 key en 文案）未逐一验证。**影响**：前端"模型选择器直选服务+模型"路径在 Rust 侧退化为项目配置端点。
2. **attachments 多模态注入**（93 号备案）：归一化/落盘/校验已就绪，image base64 内联与 text 注入未进 LLM 消息（`LLMMessage` 无多模态形态）。**影响**：带图聊天的视觉理解不可用（文本附件同理）。

### P2（功能完整项，不阻断切换）

3. doctor transport 回退细节（首探测空响应 → 非 stream 回退；ops72 覆盖主干）。
4. PDF 文本抽取（83 号备案；ingest_material 的 pdf 分支为固定错误文案，TS 用 unpdf）。
5. 单章写作中途截断（Rust 写作链无内建中止检查点，67 号备案）。

### P3（低风险差异，长期收敛）

6. 非行为面差异聚合：TS logger/console 诊断面（Rust 无对应日志面）、JS 宽松 base64 vs Rust 严格解码、系列模式 architect 端 series 评审环的 stages 进度标签粒度等（各轮偏差备案已记录）。

## 四、strangler 切换判定基线

- **核心创作链**（write/audit/revise/import/play/short/translation/fanfic/script/film/forecast）与管理面（books/sessions/config/services/skills/files/doctor）端点 **100% 对齐**且各有 E2E。
- **切换建议**：完成 P1 两件后，Rust 端即可承接全部 studio 流量；P1 未完成前，带模型直选与带图聊天两类请求需继续由 Node sidecar 承接（或接受降级行为）。
- **回归护栏**：六项验证基线（lib 1131 / golden 76 / E2E 157 / export-bindings 1290 / clippy 零警告 / TS 1798）+ 本扫描的端点对照可作为切换前 gate。

## 五、验证基线

本轮代码零改动；快验确认无漂移：`cargo test --lib` 1131 过、`cargo test --test e2e_write_next_contract` 157 过（其余四项上轮全绿未受影响）。

## 六、影响面与下一步（95 号候选）

1. **首选：attachments 多模态注入**（P1-2——`LLMMessage` 扩展多模态内容形态（image base64 段），agent-session 消息组装注入归一化产物；闭合 93/94 两轮备案）。
2. 其次：studio 模型配置域（P1-1——resolveServiceModel/listModelsForService/secrets + 四层解析 + INKOS_LLM_PROXY_URL；规模较大，可拆为 secrets/models 两轮）。
3. P2 清单逐件（doctor 回退 / PDF 选型 / 单章截断）。
