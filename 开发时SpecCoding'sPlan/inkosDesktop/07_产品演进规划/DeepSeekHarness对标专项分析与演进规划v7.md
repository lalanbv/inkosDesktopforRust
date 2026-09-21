# DeepSeek Harness 对标专项分析与演进规划 v7（deepseek-ai/deepseek-harness）

> 日期：2026-09-22 ｜ 编号：540 号 ｜ 前序：v1（328 号）→ v2（355 号，R1–R9）→ v3（374 号，R10–R19）→ v4（388 号，R20–R24）→ v5（396 号，R25–R29 全落）→ v6（539 号，Pi 对标专项，R30–R37）。
> 本轮输入：①外部对标 **deepseek-ai/deepseek-harness**（下称 dsh；MIT；developer preview；~232k stars / 18k+ commits，@2026-09-22）——官方中文文档 **12 篇全文精读**（architecture / cordis-primer / tool-execution-pipeline / agent-lifecycle / defensive-patterns / glossary / invariants / skills / token-meter / compaction / spill / slots）+ **6 篇结构扫描**（session / tools / subagent / core / workflow / ptc-runtime / system-prompt）+ **3 篇生成目录结构扫描**（capability-seams / module-graph / tool-catalog）；②本项目架构实证勘察（本会话只读 Explore：engine-rs 工具分发 5 处 match 点 / skills 三件套 / server 分层工厂 / SSE 双端事件表 / packages 双端逐文件镜像 / src-tauri 插件半实现 / studio api 三层）；③500–539 号工程债台账与 v6（539 号）裁决的无缝衔接。

## 1. 对标对象画像：dsh 是什么

一句话：**DeepSeek 官方开源 agent harness（CLI 名 `dsh`），口号 "Everything is a Plugin"——产品的每一部分（模型适配器、工具注册表、会话日志、乃至 agent loop 本体）都是可从配置替换的插件。**

四层结构：

1. **Cordis 插件框架（vendor 引入，"时空可组合"范式 arXiv 2608.25512）**：插件向共享上下文贡献服务（稳定 `ctx.<key>`，如 `ctx.tools` / `ctx.llm` / `ctx.sessions`）；以 `inject` 声明服务依赖（加载顺序=依赖图而非手工编排）；以**五类分发模式**的类型化事件通信（emit / waterfall / parallel / serial / bail——观察 / 环绕包装 / 并行扇出 / 按序 / 首 bail 截停）；**一切注册都是可逆副作用**，插件卸载即撤销。
2. **Profile / Bundle / Patch 组装**：运行实例=一棵插件树，由具名 profile（web / headless / sdk / acp / desktop）按序叠加组合包 + YAML patch 构成；`dsh --dump-config` 打印整棵树，任何条目可被 patch 替换。其 Electron 桌面形态与本仓同构（壳 + Web UI + Host 进程），默认端口 3080（web）/ 19387（desktop）。
3. **会话日志即真相**：append-only `SessionEvent` 日志是模型上下文唯一来源；**「模型可见即已记录」是运行时不变量**——抵达模型请求的一切必须能从日志重建；`deriveMessages()` 从日志投影模型历史；三事件域分离：持久事实（session 事件，重载后仍在）/ 实时协调（`agent/*`，观察拦截进行中工作）/ 能力策略（`fs/*`、`tools/*`，无导入循环附加策略）。
4. **能力 seam 三角色**：Service Definition（拥有 ctx 键与词汇类型）+ Service Provider + Consumer；**「换一个 provider 搬走整个能力面」**——fs 与进程提供方共享同一执行世界，把它们指向远程沙箱，Bash/PTY/LSP 整体搬迁；subagent 提供方同接口下从新建子 agent 到委派另一产品。

与本项目直接相关的机制细则（七件）：

- **工具执行管线**：`tools/pre-execute` waterfall（钩子/权限/沙箱）→ 注册单调守卫（deny / abstain，身份保护不可重排）→ `ctx.approval` 一次性审批（缺席或不可答即拒绝）→ `tools/execute` waterfall（超时/重试/metrics 环绕分发）→ fs 写意图闸门（tool-fs 变更前先读后写）→ 工具体 → `tools/post-execute`（接受/阻断/替换/附加上下文）→ 注册表无损快照（快照抛错先规范化为 isError）+ `finalizeContent` → `tools/result` 冻结权威结果。钩子借此横跨工具族，**工具永不与策略服务耦合**。
- **token-meter**：回放快照式计量——启发式计价 + 最近成功调用 usage 锚点修正（同规范 envelope 且总量不低于路由定价锚点才复用）；`logRevision` 消费位点；O(surface) 测量、O(1) 追加 fold；按路由声明图片定价。
- **compaction**：surface 区间替换为单个摘要节点；`compaction/start|end` 锁事件括住**整个**操作（中途崩溃=可检测遗留锁，而非假完成）；工具调用/结果配对平衡校验（`toolPairingBalancedBefore/After`）；`toolResultPruner` 确定性头/中/尾裁剪（Unicode 码点计量）；pressure / context-overflow 双触发；摘要走 `user/message` + `surfaceOp: replace`——唯一 surface 变更。
- **spill**：工具大结果落盘（`<root>/session-<hash>/<random>-<safeName>`；0700 私有目录 + `wx` 独占创建防符号链接竞态 + 0600），模型侧得 locator + retrievalHint；**尽力而为语义**——落盘失败保留原内联结果，绝不把成功调用变 isError。
- **skills 分层注册表**：宿主层 + scope 层合并，rank 裁决重名（项目 100 → 用户 600 六级）；invocation policy 双布尔（modelInvocable / userInvocable，四组合保留）；目录快照 complete 语义（不完整观测不缓存，消费方保留 last-good 重试）；目录 digest 变更 → 持久 `<system-reminder>` 全量替换注入（目录只含 name + description，永不含正文与绝对路径）；`skill` 工具加载前再校验策略。
- **运行时不变式注册表**（`ctx.invariants`）：每包一个 `./invariant` 配套插件注册检查，失败归因到包（`InvariantError` 带包名）；`verify-package-invariants` 机械拒绝「无解释的空安装器」等敷衍形态——只断言权威事件流或可变数据，绝不检查「服务是否存在」。
- **文档即生成物**：config-catalog / tool-catalog / module-graph / event-producer-consumer / dependency-catalog / persistence-catalog / cordis-surface 全部脚本从代码生成 + verify 脚本入 doc-sync 防漂移 + 双语配对校验（`verify-translation-pairing`）。
- **defensive-patterns 缺陷类别规则**（自述每条都是真实或差点发布的缺陷）：正交结果独立上报（超时却 exit 0 ≠ 成功，勿嵌套标志分支）；公共约定两侧规范化（消费方不必猜异常来自哪层）；dispose 必须达到停稳而非请求停止（等子进程退出再返回）；分发器隔离回调异常（坏订阅者不能破坏核心生命周期）；环境变量 `*KEY*/*SECRET*/*TOKEN*/*PASSWORD*` 清洗 + 随机文件名（可预测全局可读路径=符号链接竞态+信息泄露）。

## 2. 本项目现状对照（本会话实证）

### 2.1 不动摇的领先根基（对标确认，dsh 无或弱于本项目）

| 领域 | 本项目 | dsh |
| --- | --- | --- |
| 双端契约工程 | 41 端点活体差分 + golden 18 键码点锁 + 工件内容面零备案硬门禁 + duel 真跑 10/10 + node/rust 双腿 smoke | 单实现单端，无移植对齐问题；verify 脚本只防文档/目录漂移 |
| 写作领域纵深 | 写作链八级质量治理 / ContextLens 装配回放 / 防复读门 / 题材三库 / 反AI种子包 | 无任何领域层 |
| 桌面工程 | Tauri 2 壳（轻）+ 双通道更新 fail-close 签名 + 引擎进程监督 + Keychain 同步 | Electron 携带完整 dsh 运行时（重）；profile 体系更强 |

### 2.2 缺口矩阵（dsh 机制 → 本项目实证现状 → 判定）

| dsh 机制 | 本项目实证现状 | 判定 |
| --- | --- | --- |
| 工具注册表 | 无注册表：`match name` 硬编码分发散落 5+ 处（interaction/project_tools.rs:448、book_edit_tools.rs:447、film_authoring_tools.rs:217、play_tools.rs:67、forecast_tools.rs:233）；TS 侧 34 个 createXxxTool 工厂挤在 agent-tools.ts 单文件；**双端工具面（名称/描述/schema）无任何机械对照** | **真缺口 → R38（P0）** |
| 三段执行管线 | 守卫两处专项（loopback_guard / segment_guard）未归一进统一管线；无统一超时/重试/metrics 环绕段（llm/streaming_client.rs:200 备案无重试环、无 thinkingBudget）；无后置结果改写钩位 | **真缺口 → R39（P0）** |
| 生成式目录 + verify | docs 手写且已实证漂移（plugin-system.md 称 Svelte 前端 + 完整 wasmtime 执行；实为 React studio + `WasmPlugin::execute` 恒 ExecutionFailed 骨架，plugin/runtime.rs:3-18 自述"WASM Runtime 骨架"）；变更记录注释撞号 533/534/535/536 四连发 | **真缺口 → R40（P0）** |
| token-meter | R26 Run Log 是调用级事后遥测；无请求面压力计量；长篇会话/写作链无预算感知 | **真缺口 → R41（P1）** |
| spill | 材料检索 / audit 报告 / web research 大结果全量内联进 prompt 与响应 | **真缺口 → R42（P1）** |
| 会话日志即真相 + 投影 | 会话/草稿/运行状态散落 StateManager + task_store + RunLog 三处；无 append-only 统一日志；无「模型可见即已记录」不变量；agent.rs 仍是 2 行占位 | **真缺口 → R43（P1）** |
| skills 注册表语义 | 五目录隐式优先级（skills/external_loader.rs：env → ~/.openclaw → ~/.agents → 项目 .agents → 项目 skills）+ production_bindings.rs 硬编码 8 条绑定；无 invocation policy；技能变更无失效事件（UI 不热刷新） | 改造 → R44（P2） |
| LLM / subagent 提供方 seam | LLM 适配器内聚 llm/ 但非显式 seam（R30 正迁移传输层至 @earendil-works scope）；subagent 提供方单一（agent_trajectory.rs:10 嵌套接线备案）；引擎选择 rust/node 是原始形态的"提供方切换" | 改造 → R45（P2） |
| approval / 单调守卫 | 桌面单机无远程攻击面；但 research_web / 封面生图出网 + 未来插件市场需要闸位 | 闸位预留并入 R39，审批 UI 本轮不启用 |
| compaction | 聊天面 12 轮粗上限；写作链靠 per-chapter 隔离 + 控制文档（领域内自洽的轻量等效） | **v6/R32 已立项**（Pi 系压缩对齐，TS 面）——归 R32 双端范围，本轮不重复立项 |
| Cordis 本体 / 53 包 / profile-patch / HMR / PTC / agent-team / webhook / ACP / LSP / Electron / UI slots | — | **不采纳**（§3.2） |

## 3. 总纲裁决

### 3.1 学模式、不搬框架

**不引入 Cordis，不拆 53 包。** 理由：①dsh 的价值在扩展点契约 + 可逆注册 + 事件域 + 文档生成纪律的**模式**，不在框架本身；②本项目 Rust 主引擎 + TS 回退端的双端镜像现实下，引入第三套跨语言框架等于把移植面再乘一倍——529–536 系列每一步都在清偿对齐债，是实证；③53 包粒度远超团队规模与双端移植预算。

落地形态：Rust trait/枚举 + TS 接口在**既有包结构内**同构实现注册表 / 管线 / 事件域 / seam 四种形态，双端一一对应；差分器继续当对齐法庭，每引入一种形态就为它开一个差分维度。

### 3.2 不采纳清单（显式弃置，防未来重复评估）

Cordis 框架本体与 vendor 化；53 包拆分粒度；profile/bundle/patch 组装体系（server 分层工厂 `router → router_with_runtime → router_full → router_books` 已是其单端等价物）；HMR 插件热替换；PTC `run_code`（写作工具应收窄而非扩大执行面）；agent-team / mailbox；webhook；ACP；LSP；Electron 形态迁移；UI slots 全套（studio 四区结构不重构——v5 已裁）。与 v6 不采纳项合并维护：pi-agent-core 替换自有循环、CBOR/RPC 面、chord、全信任扩展模型。

### 3.3 与 R30–R37（v6/Pi 专项）的衔接（零重叠声明）

| v6 项 | 与本轮关系 |
| --- | --- |
| R30 pi-ai scope 迁移（provider.ts 单适配缝） | R45 把该缝升级为显式 seam 三角色——R30 迁移在先，R45 只做形态升级 |
| R31 传输守卫面 golden（守卫参数/事件形态/failover 序） | 层次分界：R31 锁 **LLM 传输层**，R39 管线守卫在**工具执行层**；R39 的守卫 golden 沿用 R31 体例 |
| R32 压缩对齐 Pi（reserve 阈值/切点回溯/迭代摘要/compact-and-retry） | 覆盖聊天面压缩双端落地；本轮**不单列 compaction**，R32 完成后若 Rust 端对齐成本超预期再立专项 |
| R33 遥测显式化 + 双端共享 span schema | R41 计量字段并入其 span schema——计量是遥测的取数源之一 |
| R34 供应链政策 / R35 版本握手 / R36 transcript 树化 | 无交集 |
| R37 自扩展技能创作链（agent 生成 SKILL.md → 热载） | 热载通道复用 R44 的 `skills/change` 失效事件——R44 在先或同轮 |

0GC 口径沿用 v6 显式化：**Rust 热路径以 bench:gate 守住为零 GC 成立标准；TS 回退端不追 0GC；studio 无此语义**。本轮所有性能落点只动 Rust 面 + bench 扩维。

## 4. 七维自检（需求硬指标逐条对应）

| 维度 | 本轮保障 |
| --- | --- |
| 最优解 | 每项带采纳/改造/不采纳裁决与理由；不采纳清单显式化；与 v6 零重叠声明防重复建设 |
| 兼容性 | REST/SSE 现面零破坏——差分器 41 端点全绿=每轮硬验收；Node 回退端 P0/P1 全对齐（P2 允许 Rust 先行 ≤1 轮窗口）；书库存档格式零迁移（spill 目录与事件日志均为新增文件，不触碰既有 truth/md/json 布局）；上游 sync 冲突面最小化（新增文件为主，不挪既有行） |
| 可扩展性 | 注册表 + 三段管线 + seam 化 = 新工具/新提供方/新技能全部走注册而非改分发点；扩展实操手册随 R40 目录生成 |
| 安全可靠 | 守卫归一进 pre 段；spill 0700/wx/随机名/密钥类 env 清洗；approval 闸位预留；「模型可见即已记录」不变量 + 差分器硬门禁 + duel 真跑全程 |
| 高性能 0GC | 静态注册零堆分配（`&'static` 表）；SSE typed enum + 序列化缓冲复用核对；spill 流式写盘不驻内存；meter O(1) 追加/O(surface) 快照；bench:gate 扩维（工具分发/管线/meter）守门 |
| 可实施执行 | 每项：双端成本 + 验收矩阵 + 门禁挂钩；P0 三项一轮可收口；迁移期双轨策略明文（§7） |
| 真实需求 | 全部服务两条主链（写作链一章生产 / 聊天面改稿）与工程债止损；不为通用 agent 化过度建设（对照面显式弃置项即证据） |

## 5. 演进路线（R38–R46）

### P0（结构与工程纪律止损）

| # | 事项 | 设计要点 | 双端 | 验收 |
| --- | --- | --- | --- | --- |
| R38 | **工具注册表单点化** | Rust：`trait Tool { name / description / parameters(schema) / execute(ctx, args) }` + `OnceLock` 静态注册表（`&'static` 表项，零堆分配；phf 或显式数组宏），5+ 处 `match name` 分发收敛单点；TS：34 工厂改注册表 Map 同构形态；**差分器新增工具面维度**：双端名称/描述/参数 schema 逐工具 diff（当前对齐最大盲区入门禁） | TS+R | 分发单点化；差分器工具面 0 分歧；bench 新增工具分发基准无回退 |
| R39 | **工具执行三段管线** | 对齐 dsh 管线三段：pre（loopback/segment 守卫归一 + approval 闸位预留不启用）→ around（超时/重试/metrics；顺带清偿 streaming_client.rs:200 无重试债，重试语义与 R25 接管链协同：可重试错误在当前模型重试后按链 failover）→ post（结果改写钩位，为 R42/R44 预留）；defensive-patterns 两条入编码规范：正交结果独立上报（超时≠失败嵌套分支）、分发器隔离回调异常 | TS+R | 全部工具调用过管线；Run Log（R26）记录 around 段耗时与重试；守卫 golden 沿 R31 体例锁定 |
| R40 | **生成式目录 + 编号工具化** | ①从代码生成三目录：工具目录（R38 后 schema 单一事实源）/ REST 端点目录 / SSE 事件目录（Rust sse.rs 与 studio use-sse.ts 常量表对生成物校验），`gen:catalog` + `verify:catalog` 并入 gate:ts——根治 plugin-system.md 式手写漂移；②`scripts/next-change-no.mjs`：当日目录最大号 + git log 近 30 条注释号双查输出下一号，写档脚本化根治撞号（533/534/535/536 四连发实证） | 脚本+TS+R | verify 入门禁全绿；故意注入漂移被 verify 拦截（负样本测试）；下轮起撞号归零 |

### P1（上下文管理三件套——长篇主链核心痛点）

| # | 事项 | 设计要点 | 双端 | 验收 |
| --- | --- | --- | --- | --- |
| R41 | **token 计量服务** | 对齐 token-meter：启发式计价（chars/4 级）+ 最近成功调用 usage 锚点修正；O(1) 追加 / O(surface) 快照，快照不可变即取即弃；`GET /api/v1/context-meter` 只读端点；studio 面板挂 RunLog（R26）旁；写作链输入准备处接预算检查——超限告警进 ContextLens，先观测一轮不阻断；计量字段并入 R33 span schema | TS+R+S | golden：计量纯函数 + 锚点修正三态（usage 锚/估算/替换）；端点快照测 |
| R42 | **工具结果 spill** | 对齐 spill seam：超 `maxInlineBytes` 的材料检索 / audit / research 结果落盘 `{book}/.spill/`（0700 私有 + 随机名 + 独占创建 + 密钥类 env 清洗——defensive-patterns 全套），模型侧首尾预览 + locator + 检索提示（复用既有 read/grep 工具检索，不加新工具）；尽力而为：落盘失败保留原内联，不降级成功调用 | TS+R | golden：阈值/预览/降级三态；工件差分器把 `.spill/` 纳入存在性对照 |
| R43 | **会话事件日志 + 投影** | 范围裁剪版「日志即真相」：仅收敛**聊天/agent 会话面**——append-only 事件日志（user/assistant/tool_call/tool_result/session_title）+ `deriveMessages()` 投影替换散装拼装；「模型可见即已记录」作 Rust debug 断言 + 差分器会话面新维度；RunLog / ContextLens / 未来回放 fork 从同一日志派生（三处重复状态归一）；写作链持久面**不动**（已有 buildPersistenceOutput 输出面）；事件词汇对齐 dsh surface 语义（仅产生消息的事件进模型） | TS+R | 差分器会话面 0 分歧；崩溃恢复=日志回放重建（注入中断尾部修复） |

### P2（扩展面升级，按需逐项独立立项）

| # | 事项 | 设计要点 |
| --- | --- | --- |
| R44 | **skills 注册表升级** | rank 显式表替代隐式目录序；frontmatter 增 `disable-model-invocation` / `user-invocable` 双布尔（dsh 同名语义，省略默认 true）；`skills/change` 失效事件 → SSE → studio 热刷新；production_bindings 8 条硬编码迁数据文件（R40 目录生成物核对）；R37 自扩展技能链热载共用此通道 |
| R45 | **LLM / subagent 提供方 seam 化** | `trait LlmProvider` 定型（stream / usage / price 声明——喂 R41 锚点）；R30 的 provider.ts 单适配缝升级为 Service Definition + Provider + Consumer 三角色显式 seam；subagent 提供方接口预留（537 号 daemon verdict 化立案的落地载体）；引擎 rust/node 选择升级为同一 seam 的两个 provider |
| R46 | **WASM 插件宿主契约收敛** | 按 seam 定 wit 最小面（fs / 进程提供方接口先立契约）；plugin-system.md 漂移清偿（React studio / 执行骨架现状如实改写 + 进程隔离 JSON-RPC 主路径记载——plugin/runtime.rs:3-18 实证 `execute` 恒 ExecutionFailed）；或明判进程隔离为主路径、wasmtime 骨架归档，立项时按当时裁决；沿用 v6「全信任扩展模型不采纳、WASM 沙箱+声明式权限维持领先项」裁定 |

## 6. 里程碑与验收门禁

- **M1（P0 一轮收口）**：R38 + R39 + R40——门禁 8 项全绿 + 差分器新维度（工具面）0 分歧 + bench 零回退 + verify 负样本拦截通过。
- **M2（P1）**：R41 + R42 + R43——差分器会话面/工件面扩维全绿；golden 扩展计量/spill/投影纯函数面；崩溃恢复演练（日志回放）通过。
- **M3（P2）**：R44–R46 按需逐项独立编号立项，每项自带验收矩阵。
- 全程沿用既有纪律：duel 真跑 `INKOS_DUEL=1`、bench 隔离复跑（满载误报不洗基线）、门禁统计落盘再解析、推送走 Fork 图形端、变更记录编号脚本化（R40 落地前仍双查）。

## 7. 风险与缓行

- **R38 迁移双轨**：注册表与旧 `match` 并存，逐文件迁移 + 差分器工具面维度逐步亮灯，禁止一次性 flip；`agent.rs` 2 行占位正好由本项转正。
- **R43 触及聊天消费面**（studio 多处读会话状态）：先 Rust 端立日志+投影、studio 只读消费切换、Node 回退端随后对齐——半窗口 ≤1 轮；与 R36 transcript 树化预研共享事件词汇，树化若立项直接在日志上加 id/parentId。
- **compaction 不在本轮单列**：R32 覆盖 TS 面；若其落地后 Rust 端对齐成本超预期，升级独立专项再评。
- **dsh 仍 developer preview** 且自述 breaking changes：本轮只取已稳定的模式结论，不跟踪其 API、不做任何依赖引入。
- 领先项维持：双端契约工程、写作领域纵深、Tauri 轻壳三项对标确认继续领先，不在本轮重构范围内翻动。
