# 62 号变更记录：Node sidecar 下线核对——端点全量对照清单（Phase3 · 全局）

- 日期：2026-08-14
- 范围：无代码变更（纯核对产出）。比对 Node sidecar（`packages/studio/src/api/server.ts`）
  全量路由与 engine-rs（`src/server/mod.rs`）注册路由，产出缺口/偏差/可切换清单，
  作为 strangler 切流的依据基线
- 方法：脚本提取两侧 `method + path`（Hono `:file{.+}` 与 axum `*file` 通配形态归一），
  集合差运算；缺口配 server.ts 行号；偏差汇总 49-61 号各变更记录的「偏差备案/暂缓件」

## 一、总览

| 项 | 数量 |
|---|---|
| Node sidecar 业务路由（不含 `get /assets/*` 静态） | 132 |
| engine-rs 注册路由 | 85 |
| **共同（方法+路径一一对应）** | **80** |
| Node 有 Rust 无（缺口） | 52 |
| Rust 有 Node 无（附加） | 5 |

Rust 侧业务端点 82 条 = 80 条与 Node 一一对应 + settle（Node 的 settle 为
pipeline 内部函数，Rust 暴露为独立端点）+ health/utils×3（Rust 专属基础设施，
Node 无对应）。

## 二、可切换端点清单（80 条，均有 E2E 契约测试覆盖）

按域分组（括号为实现号次）：

- **SSE**：`GET /events`（42/43）
- **写作与检测链 16 条**：write-next（42）、audit/:chapter（44）、plan（45）、draft（46）、
  revise/:chapter（46）、rewrite/:chapter（47）、resync/:chapter（47）、compose（47）、
  consolidate（48）、repair-state/:chapter（48）、analytics（48）、eval（48）、
  export（48）、export-save（50）、detect/:chapter + detect-all（51）、detect/stats（52）
- **编辑事务 10 条**：chapters/:num GET/PUT/DELETE、workspace、workspace/brief、
  workspace/inspiration（50）、versions/:versionId GET、restore、approve、reject（49）
- **truth 3 条**：truth GET、truth/:file GET/PUT（50/53）
- **书籍管理 7 条**：books GET、books/:id GET/PUT/DELETE（48/58）、create-status（53/58）、
  books/create（58）、import/chapters（58）
- **genres 6 条**：GET、GET/:id、POST create、PUT/:id、DELETE/:id、POST/:id/copy（53）
- **project 配置 19 条**：GET/PUT /project（54/56）、POST language（54）、
  八组键 GET+PUT：default-model、detection、input-governance-mode、model-overrides、
  notify、chapter-review-mode（项目级+书级）、research-search（54/56）
- **风格与导入 4 条**：style/analyze、style/import、import/canon、books/:id/fanfic GET（55/59）
- **衍生创建 4 条**：fanfic/init、fanfic/refresh、spinoff/init、imitation/init（59）
- **skills/prompt-packs 6 条**：GET skills、POST skills/import、DELETE skills/:skillId、
  GET prompt-packs、PUT/DELETE prompt-packs/:promptId（60）
- **文件浏览 3 条**：GET project/files/*file、GET/PUT project/artifacts/*file（61）

覆盖书籍全生命周期：创建（原作/导入/同人/番外/仿写）→ 写作 → 审计 → 修订 →
导出 → 配置 → 技能/提示词 → 文件浏览。

## 三、缺口清单（52 条，Node 有 Rust 无，按域分组）

### services 域（11 条）——LLM 服务配置
`GET /services`(L3705)、`GET /services/config`(L3758)、`POST /services/config/import-env`(L3773)、
`PUT /services/config`(L3821)、`DELETE /services/:service`(L3955)、
`POST /services/:service/test`(L3978)、`PUT /services/:service/secret`(L4055)、
`GET /services/:service/secret`(L4079)、`GET /services/models`(L4087)、
`GET /services/models/custom`(L4109)、`GET /services/:service/models`(L4138)
——依赖 LLM provider 预设表（40+ 预设）与 secret 存储（54/56 号备案的
「LLM provider 配置域大件」，同时解锁下述两项共同端点功能性缺口）

### cover 域（4 条）——封面生成配置
`GET/PUT /cover/config`(L3854/L3885)、`GET/PUT /cover/secret/:service`(L3918/L3927)
——依赖同 services secret 件

### sessions 域（8 条）——交互会话机
`GET /interaction/session`(L4536)、`GET /sessions`(L4703)、`GET /sessions/:id`(L4709)、
`POST /sessions`(L4717)、`PUT /sessions/:id/play-mode`(L4741)、`PUT /sessions/:id`(L4759)、
`DELETE /sessions/:id`(L4774)、`POST /sessions/:id/abort`(L4788)
——依赖交互运行时会话机（session 状态机 + task.* 事件流 + 多 intent 路由，50 号暂缓大件）

### agent 域（1 条）
`POST /agent`(L4805)——自由 agent 端点，依赖交互运行时 + agent-tools 工具面
（49 号暂缓的 edit-controller 其余 kind 均在此工具路径）

### play 域（4 条）——互动游玩
`GET /play/runs/:worldId/:runId`(L4551)、`PUT .../image-settings`(L4605)、
`POST .../generate-image`(L4619)、`GET .../images/:file`(L4682)
——依赖 play 世界运行时（mutator/renderer/reconciler）

### daemon 域（3 条）+ doctor/logs（2 条）
`GET /daemon`(L4463)、`POST /daemon/start`(L4469)、`POST /daemon/stop`(L4508)、
`GET /logs`(L4520)、`GET /doctor`(L6459)——桌面常驻进程管理 + 诊断面（轻件）

### radar 域（2 条）
`POST /radar/scan`(L6434)、`GET /radar/history`(L6448)——素材雷达

### translations 域（6 条）
`GET /translations`(L6525)、`POST /translations/upload`(L6554)、`POST /translations/create`(L6560)、
`GET /translations/:id`(L6588)、`POST /translations/:id/run`(L6625)、
`POST /translations/:id/export`(L6655)——翻译工作流

### interactive-films / projects 域（10 条）——故事图谱与互动影视
`GET /interactive-films`(L6504)、`POST /projects/:id/story-graph/delta`(L6668)、
`GET /projects/:id/story-graph`(L6678)、`GET /projects/:id/export`(L6695)、
`GET .../story-graph/validation`(L6718)、`GET .../story-graph/analysis`(L6730)、
`GET .../export/json`(L6738)、`GET .../export/ink`(L6751)、`GET .../export/html`(L6764)、
`POST /projects/:id/nodes/:nodeId/image`(L6789)——独立可视化大域（54 号暂缓件）

### books 补充（1 条）
`POST /books/:id/foundation/revise`(L3615)——架构稿修订（57 号暂缓的
buildRevisePrompt/reviseFrom 四文件装配，59 号遗留清单）

## 四、Rust 附加端点（5 条，Node 无对应）

`GET /health`（探活）、`POST /books/:id/settle`（Node 的 settle 是 pipeline 内部
函数，Rust 为编排调试暴露）、`POST /utils/derive-book-id|count-length|cap-context`
（Node 侧为内部函数，Rust 暴露为 HTTP 工具端点）。切流时可直接透传，无契约冲突。

## 五、偏差汇总（共同端点的已知差异，切流核对用）

### 全局性（跨域）
1. **错误体文案**：TS 原生长文案（`String(e)`/ENOENT 文本）vs Rust 短文案——
   状态码与 code 一致（49/50/53/58/59 号备案）
2. **落盘 JSON 键序**：serde_json BTreeMap 字母序 vs TS 插入序——功能等价、
   diff 噪声不同（48 号起既有，54 号统一备案）
3. **LLM 调用无 abort signal**（57 号，全端点一致暂缓）
4. **acquireBookLock 跨进程文件锁**：Rust 为进程内互斥——strangler 双跑期间
   Node/Rust 不同域不同时写同一书（41 号起口径）
5. **SSE 单进程 hub**：双跑期间两进程各广播各自事件，前端需双订阅或按域分流
6. **垃圾输入路径不复刻**（50/53 号）：如 format 传数字、id 传数字的边界行为

### 功能性缺口（共同端点内的暂缓子功能）
1. `GET /project` env 层合并暂缓（resolveEffectiveLLMConfig 的 INKOS_LLM_* 覆盖，
   56 号）——桌面单仓场景行为一致
2. `PUT /project/default-model` 不做 syncTopLevelLlmMirror 顶层镜像（54 号）
3. `resync/:chapter` 的 governed artifacts 注入暂缓（51 号）
4. detect 的 detectAndRewrite/detectAIContent 暂缓（52 号，依赖外部 AIGC 检测
   provider 配置域）
5. 创建链同步钩子 markBookActiveIfNeeded/syncCurrentStateFactHistory/
   syncNarrativeMemoryIndex 暂缓（58 号）
6. import 回放的 resumeFrom 断点续导暂缓（58 号）
7. spinoff 完成事件 book:created 不带 book 摘要（59 号）
8. 61 号边缘差异：空通配段 404（Hono 路由不匹配）vs 400（axum 命中校验）；
   percent 严格性已修复对齐

## 六、切流建议

- **立即可切换**：第二节 80 条（书籍核心业务闭环）。建议前端代理按路径前缀分流：
  `/api/v1/(books|genres|project|skills|prompt-packs|style|fanfic|spinoff|imitation|events)`
  → engine-rs；其余 → Node sidecar 保持
- **不可切换（保持 Node）**：第三节 52 条
- **SSE**：events 端点切流需最后执行（双跑期两 hub 各自广播）；或前端双订阅聚合
- **大件优先级建议**（后续号次）：
  1. services + cover 域（15 条）——解锁第五节两项功能性缺口（env 层合并 +
     顶层镜像），消除 54/56 号最大偏差
  2. sessions + agent 域（9 条）——交互运行时会话机大件
  3. foundation/revise（1 条）+ resumeFrom + 同步钩子——补全 books 域
  4. interactive-films/projects 域（10 条）——独立可视化域
  5. translations（6 条）、play（4 条）、daemon/doctor/logs/radar（7 条）

## 七、影响面

- 无代码变更；本记录为切流决策基线文档
- 验证基线不变（lib 953 / golden 76 / e2e 65 / export-bindings 1112 /
  clippy 零警告 / TS 1798）
