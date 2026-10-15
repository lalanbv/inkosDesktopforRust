# 713 号：导演域三面 + context-meter 观测面回收（ChatPage 集群；回收清单 23→20）

> 目标线：自主持续开发（TS/Rust/桌面壳）。本批=UI 活消费排序的下一批：
> 聊天页最高频消费集群——DirectorPanel（书籍设置页挂载）三面 + ContextMeterBadge
> 计量观测面（聊天输入状态条挂载，5s 轮询）。

## 一、嫁接内容（TS 侧 2.0 基底，Rust 侧为零改动）

### 1. context-meter 计量链（R41/563/564 号，fork 逐字 + 同点挂线）
- **core agent-session.ts**（纯增量）：
  - `AgentSessionResult.contextMeter?: TokenMeterSnapshot`；
  - `extractMessageMeterText` 计量投影 helper（fork 逐字）；
  - 结果提取区计量块（fork 逐字自包含 try/catch）：全量消息回放入
    TokenMeter（`state.messages` 已含本轮 assistant 回复 = 下一轮请求面），
    usage 入锚（`finalAssistant.usage`），581 号窗口兜底 128k（contextWindow≤0
    按缺省窗计，超窗告警面才可能触发）。观测不阻断，usage 缺席走纯启发式。
- **studio server.ts**：
  - `contextMeterRegistry`（cap 64 逐出最旧，Map 保序删旧再插）+ 
    `storeContextMeterSnapshot`（fork 逐字）；
  - `GET /api/v1/context-meter`（缺参 400 INVALID_SESSION_ID / 无记录 404
    SESSION_NOT_FOUND，typed 错误体与 Rust api_error 双端同形）；
  - 计量点=主聊天轮 `runAgentSession` 返回后（fork 同点：6355→现 6848 区；
    确认式任务 continuation 轮不计量，fork 亦然）。

### 2. 导演域三面（G6/353/354 号）
- **studio server.ts**：
  - `POST /api/v1/director/directions`：灵感守卫 + R4/364 库资产 guidance
    （refs 命中三库 → renderAssetGuidanceBlock）+ `PipelineRunner.generateDirections`
    （DirectorAgent chat 编排，temperature 0.8，fork 契约纯函数解析）；
  - `GET /api/v1/books/:id/director`：`.inkos/director/{id}.json` 解析（坏载荷
    静默 null=与 Rust serde None 同水位）+ `state.loadChapterIndex` 已存章数 +
    `formatResumeHint(savedChapters,"en")`（G9 续跑建议，TS 真源双端 golden）；
  - `PUT /api/v1/books/:id/director`：patch 六键白名单合并（478 号整体替换
    语义）+ runMode 白名单 400 + **updatedAt 对齐 Rust `utc_now_ms` 数字毫秒**
    （fork 旧版 ISO 串——线格式以 Rust 产品引擎为真源）+ **patch 非对象跳过
    合并与 updatedAt 仍落盘现值**（Rust `as_object()` None 分支精确对齐）；
    补 isSafeBookId 守卫（709 批次防 traversal 同标准，fork 旧版无）。
- **core**：
  - `runner.generateDirections`（fork 逐字委托 DirectorAgent——类本体 2.0 在树
    未裁，只缺 runner 委托与 server 路由）；
  - index 导出补齐：director 模型族（InspirationCardSchema/候选解析/运行编排/
    DIRECTOR_CONTRACT）+ resume-advice 族（formatResumeHint 等）+ token-meter
    族（TokenMeter/TokenMeterSnapshot 等）——705/707「模块在树导出被裁」同型
    倒查三处齐收。fork index 的 `mergeTaskRouting` 混入 director 块系合并残迹，
    不照抄。

## 二、双 BUG 实锤与修复（contract-diff 首轮抓到）

### BUG-1：directions 双腿状态不对称（node=500 rust=200）
- **根因链**：mock 对「你是故事导演。」提示词无分派 → 落 `lastRole==="user"`
  分支 → 返回 propose_action **工具调用**（非文本）。Rust 读 content 空串 →
  parse 空数组 → 伪 200；TS pi-ai worker 收未注册工具调用 → 流解析失败 → 伪
  500。708 号 radar analyze「双 500=路由在位」实为同因对称巧合，本轮证据升级。
- **修复（双层）**：
  1. mock 增设「故事导演」分派支（返回 prompt 请求的 `{"directions":[…]}`
     契约形态 2 候选）——walkthrough 生成链路从此有真素材；
  2. **TS 路由灵感卡 schema 归一**（709 写键教训同型）：fork `as never` 裸传
     跳过 InspirationCardSchema，keywords 缺席载荷在 `buildDirectionCandidatesPrompt`
     读 `.length` 即炸（UI 恒带 keywords 掩盖；Rust serde `#[serde(default)]`
     同键容忍=不对称根因）。修复后双端 200/200 真实解析。

### BUG-2（次生发现）：节点腿 worker chat 链证据甄别
- 进程内三段复现（dist 直调→错形误导；server 路由栈打印→真根因；
  tsx 源直调→修复实证）证实 `.length` 炸点在 prompt 构建非 LLM 链——
  **临时栈打印法**（route catch 加 console.error 后撤除）定位效率远高于猜测。

## 三、探针与域表

- SHARED_FACE 增 5 探针（4 条目语义）：director GET（先于 PUT 比缺省基线全
  body：session=null + resumeAdvice 双端 golden 同源；savedChapters/updatedAt
  在 VOLATILE）+ director PUT（幂等 stage 值状态码口径）+ directions POST
  （mock 导演分派后双端 200/200）+ context-meter 缺参 400（typed 错误体全
  body 对比）+ 未知会话 404（同形）。FORK_ONLY 移 3 条（director 双条目 +
  context-meter）→ **23→20**。
- 度量：**共享面 49 对比 0 分歧 + fork-only 20/20 + 上游新增 9/9**（+2 深链
  静态声明）。

## 四、模拟真人走查（walkthrough-env node 回退腿 + IAB 黑盒，712 同标准）

1. **导演域全链**（书籍设置页 #/book/{id}/settings）：
   - GET 面活体：`阶段: inspiration · 已存章节: 1` + 英文续跑建议全文渲染；
   - 生成方向候选：灵感框输入 → 点击 → 2 张候选卡全要素（镜中藏书楼 82%/
     坊市拾遗人 74%，钩子+差异化）；mock 无分派时此链为双伪态，分派修复后
     首次真通；
   - 选用 → PUT selectedDirection → 「已选用方向」提示；
   - 保存（runMode 切范围写作）→ PUT → 「已保存。」；
   - 刷新回读：premise ✓ / runMode=范围写作 ✓ / 已选用方向=镜中藏书楼 ✓
     （478 号 keywords 回填教训的活体 roundtrip 确认）。
   - 走查小坑：页面双「保存」同名钮（面板 + 书籍设置区）——`getByRole.first()`
     命中面板钮依赖 DOM 序（面板在 Header Section 之前），runMode 下拉须按
     选项文本过滤（页前部另有反AI严重级别 combobox）。
2. **context-meter 计量链**（书籍聊天页）：发送「给第一章写一句更抓人的开场
   钩子」→ mock 应答（propose_action 确认卡流）→ 轮末计量 → **徽标「187 tok」
   现身输入状态条**；title 全要素=`上下文计量: 187 tok (estimate) · 启发式 187
   · 无 usage 锚点 · 窗口 128000`（581 号窗口兜底活体生效；锚点缺席=mock 工具
   调用响应无 usage，纯启发式口径正确降级）。截图存档本目录。
   - 走查小坑：React 受控 textarea `press("Enter")` 不触发提交——须点 bg-primary
     发送钮（evaluate JS click 兜底，712 同法）。

## 五、门禁与回归锁

- 单测新增 `director-context-meter-routes.test.ts`（4 测）：GET 缺省/回读/坏
  载荷、PUT 白名单+runMode 400+updatedAt 数字毫秒+patch 非对象分支+roundtrip、
  directions premise 400+**premise-only 无 LLM 非 TypeError 回归锁**（键归一
  生效判别信号——炸点前移与 LLM 失败在无 mock 单测环境同为 500，靠错误形态
  区分）、context-meter 双 typed 错误体全等。
- matrix：studio 674/674（+4）· core 930/930（零漂移）· gate:ts 八步全绿
  （build/typecheck/test/audit/rust-bin/node-fallback-smoke/engine-contract-diff/
  export-epub-smoke）· typecheck 双 tsconfig 过。
- Rust 零改动 → bindings 186 面不变（ts_rs 表面无新源）。

## 六、教训

1. **探针对称性是甄别器**：radar analyze「双 500」与 directions「500/200」
   同根因不同表象——对称巧合会把断链伪装成「路由在位」，不对称才暴露真相；
   LLM 面探针要配 mock 真分派而非依赖无分派回退的对称性。
2. **schema 归一属于写面边界**（709 教训第三次生效）：`as never` 裸传 = 把
   zod default 语义让给运气；「UI 恒带某键」不是省略归一的理由，探针/第三方
   调用方不会遵守 UI 的字段习惯。
3. **错误体形态是单测环境的判别信号**：无 LLM 环境下两个同 500 的失败链，用
   「是否 TypeError」锁住归一回归。
4. **临时栈打印**（route catch 一行 console.error 后撤）比跨层猜测快一个量级。
5. 走查 UI 细节：同名钮/同名 select 页面级唯一性不成立时，必须按容器或选项
   文本作用域化；受控组件 Enter 提交依赖合成事件路径，JS click 是可靠兜底。

## 涉及文件

- packages/core/src/agent/agent-session.ts（计量块 + helper + result 字段）
- packages/core/src/pipeline/runner.ts（generateDirections 委托）
- packages/core/src/index.ts（director/resume-advice/token-meter 导出）
- packages/studio/src/api/server.ts（注册表 + 四路由 + 计量点 + 灵感卡归一）
- packages/studio/src/__tests__/director-context-meter-routes.test.ts（新增 4 测）
- scripts/walkthrough-mock.mjs（导演分派支）
- scripts/engine-contract-diff.mjs（SHARED_FACE +5 / FORK_ONLY −3）
