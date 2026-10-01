# 556 号：R39 工具执行三段管线——pre/around/post 单点接入 + 正交观测通道 + fetchUrl 出站 host 防线（SSRF）

日期：2026-10-01
类型：feat(engine)+feat(security)，544 号 dsh 施工图 R38–R46 批次第三项（R38a/R38b 工具注册表已全落后）

## 选题来路

544 号施工图 R39 = 三段执行管线（dsh 三态调度器形态的本仓落地）。R38b 已把全部
34 件收拢到 `registry::execute_routed` 单点分发，本专项在该单点内接入
`pre（守卫）→ around（超时+重试+计时）→ body → post（钩子派链）`——注册表内
无旁路，任何调用面（ChatToolRouter / project_tools 兜底链）自动获得全套防线。
顺带清偿走查发现的 SSRF 真实缺口：fetchUrl 对搜索结果 URL 无任何回环/私网防护。

## 设计定案

- **管线序**：`lookup→available→suppress（R38b 语义原样）→guards（装配序，
  Deny 短路；Ask 归一 Deny("审批未启用")）→around{tokio timeout+重试+计时}
  →body→post 派链`，全部在 registry::execute_routed 单点内接入。
- **正交观测**（defensive-patterns 规范条目——超时与业务失败互不吞没）：
  `PipelineObservation { tookMs, attempt, timedOut, errorKind }` 与 ToolResult
  分离返回；`run_pipeline -> (ToolResult, PipelineObservation)`；execute_routed
  保持签名 = 丢观测薄壳（既有调用面零破坏）。
- **观测通道**：`LoopToolExecutor` trait 加默认方法 `last_observation()`（缺省
  None，未接管线的执行器零成本适配）；ChatToolRouter 加
  `Mutex<Option<PipelineObservation>>` 存取并覆写（execute 直连 run_pipeline
  取观测）；agent_loop 组装 LoopToolExecution 时读取，卡片经
  tool_execution_cards 投影 tookMs/attempt/timedOut（仅真值）/errorKind——
  **Rust 超集加法先例**（health backend 同法），TS 面不消费（未知键忽略）。
- **重试裁剪**：仅 `MutationKind::ReadOnly` 对传输类瞬时错误重试（幂等安全），
  MAX_RETRIES=2；写工具/超时一律不重试（副作用非幂等）。工具内重试与 R25
  模型链 failover 计数天然正交（两条独立重试面互不计入）。瞬时分类 =
  `is_transient_llm_http_error` + 工具面短语表（error sending request/
  connection refused/reset/broken pipe/dns error/operation timed out）。
- **超时映射**：pipeline_timeout(kind)：ReadOnly=30s / ProjectWrite=300s /
  ProductionMutation=600s（无新增配置面）；测试载体 =
  `run_pipeline_with_timeout` 短超时注入。
- **error_kind 语义**：None=成功或分发面拒绝（unknown/suppress，attempt:0）；
  "guard"=被守卫拒；"timeout"；"transient"=瞬时重试耗尽；"error"=业务错误/
  panic/写工具的瞬时面错误（政策性不重试，终态 error 而非 transient——
  观测描述管线结局语义而非文本分类）。
- **panic 隔离**：工具体 `catch_unwind(AssertUnwindSafe)` 转错误结果；post
  钩子 panic 记 tracing::warn 返回原结果（原结果先克隆留底——钩子 future
  独占持有入参，panic 即随栈展开丢弃），不炸 agent 循环。
- **守卫（pre 段）**：`PathTraversalGuard`——PATH_ARG_TOOLS=["read","edit",
  "write","write_truth_file"] 的 args.path 含 `..` 段（`/` 或 `\` 任一分隔）
  进工具体前拒绝；绝对路径不拦（read 系统读 env 语义在工具内，守卫不可见
  env 不越权）；工具内 safe_child_path 仍是权威边界，本层为纵深预检（201 号
  segment_guard 的工具面同族）。**fetchUrl host 校验不落 ToolGuard**：URL 在
  工具体内部来自搜索结果（模型间接触达），args 面拦截不可达——落
  utils/web_search.rs 函数级统一入口 `assert_public_egress_host`。
- **post 段**：PostHook trait（BoxFuture）+ default_post_hooks() 空集骨架
  （首消费 = R42 spill，届时注册，派链序 = 注册序）。
- **RunLogBuffer 不混入工具记录**（对 544 施工图修正）：540 号 daemon 八级
  判定链读 RunLog 判定，混入工具执行记录破坏判定链语义——观测走
  LoopToolExecution→tool_execution_cards→SSE 卡片通道。
- **debug/tools 加 `pipeline: bool`（恒 true）**：加法超集字段，TS 端点不投
  影，差分器三键对照不受影响（活体验证 0 分歧）。
- **streaming clientAttempt 备案评估**：穿参面中等（agent_router 链层 2-3 层）、
  价值窄（仅 kkaiapi 端点观测头）——备案不动（同 thinkingBudget 先例）。

## SSRF 出站 host 防线（双端同构）

- **Rust**（utils/web_search.rs）：`assert_public_egress_host(url)`——仅
  http/https；host 提取（userinfo/端口剥除、IPv6 方括号内）；本地字面快筛
  （localhost 全族/.local mDNS）；IP 字面直判（v4 回环/私网/链路本地/未指定/
  0.0.0.0/8/广播 + v6 回环/未指定/ULA fc00::/7/链路本地 fe80::/10，
  v4-mapped 还原按 v4 判）；域名 resolve（tokio lookup_host）后对**全部地址**
  判——resolve 失败不拦（连接阶段自然失败，守卫只负责「解析成功但指向内网」
  与明确本地字面）。拒入文案 `Fetch blocked: …`。
- **TS 对偶**（packages/core/src/utils/web-search.ts）：同名三分类器逐项镜像
  （urlHost/isBlockedHostLiteral/isBlockedIp），resolve 走 node:dns/promises
  动态 import（零浏览器打包风险）；分类器导出 = 测试载体 + 镜像对照锚。
- **闸位定性（初判防线备案）**：DNS rebinding（解析后另行连接）与 30x 重定向
  指内网两条旁路需连接级 pin / redirect 策略封死，本层为统一入口 host 面初判；
  R40+ 评估连接级 pin。search_web 的 base_url 不做此校验（用户显式配置，
  自建网关合法）。

## 实施面

- 新建 `engine-rs/src/interaction/pipeline.rs`：守卫段/观测段/管线本体/panic
  提取/测试 11 件（unknown 短路 attempt:0 / 守卫 4 evil 向量+非文件工具放行 /
  只读瞬时重试 attempt=3 最终成功且 error_kind 归零 / 耗尽报 transient /
  写工具零重试 / 业务错误不重试 / 超时短注入 timedOut+attempt=1 / 超时映射表 /
  panic 转错误 / post 派链序+panic 隔离 / 分类器矩阵）。
- registry.rs：execute_routed 改管线薄壳；`from_defs` 测试构造器（cfg(test)——
  lib 目标无调用点）。
- agent_loop.rs：LoopToolExecution 加观测四字段（Option）；trait 加
  last_observation 默认方法；组装点消费。transcript 落盘形态不动（append_chat_turn
  写 assistant(toolCall)+toolResult 对），观测仅进 SSE 卡片。
- agent_route.rs：ChatToolRouter 存取观测 + 卡片投影 + 构造点更新。
- server/mod.rs：DebugToolEntry 加 pipeline 字段 + 测试断言 31 条全 true。
- project_tools.rs：ToolResult 加 `#[derive(Clone)]`（post 钩子留底所需，
  语义零变更）。
- pnpm-workspace.yaml：新披露 advisory 出清（audit:npm 拦 10 条，白名单已退役
  506 号故硬拦截）——brace-expansion 三连递归 DoS（GHSA-q2hr/qhr7/6j4f）+
  ip-address 解析诊断无界（GHSA-h3mg）+ dompurify afterSanitize 脱管子树
  （GHSA-p98j）+ fast-uri host 归一不一致（GHSA-hrr3）→ **精确键改 major
  范围键全系收敛**（`brace-expansion@2/^2.1.7、@5/^5.0.12、ip-address@10/^10.7.2、
  dompurify/^3.4.16、fast-uri@3/^3.1.8`）：精确键只覆盖当初命中版本，
  lockfile 重解析出的新版本（2.1.4/5.0.9/10.7.0/3.1.7）逃逸覆盖——范围键根治。

## 门禁（八项全绿）

engine-rs cargo test 1435（落盘解析）；src-tauri 468；clippy:gate 双 crate 0
告警（--all-targets）；gate:ts 七步（typecheck/test/audit/build/
node-fallback-smoke/engine-contract-diff 42 端点 0 分歧/export-epub-smoke）；
duel 真跑 INKOS_DUEL=1 10/10；core 新增 web-search-egress 5 测试。

## 教训与备案

- **成功路径须显式归零观测态**：首版循环在 continue 重试后 break 成功路径
  残留上轮 error_kind="transient"——观测态是跨迭代变量，每条 break 路径终态
  自定（改 loop 直接产出三元组后该类缺陷结构上不可能）。
- **clippy dead_store 三连**：占位初始化 `let mut x = default` 后 loop 内
  每路 break 前重赋值 = 初值死存储；loop-return 形态同时消三警。
- **trait 带宏则 impl 必须同宏**：ToolDef 带 #[async_trait]，mock impl 漏写
  即 E0277（async fn in trait 需宏才能 dyn）。
- **精确键 override 的覆盖逃逸**：安全修复 override 用精确版本键，lockfile
  重解析出新版本即逃逸——advisory 硬拦截兜住（白名单退役的价值实证），
  范围键根治。
- **立案前提当轮验证的延续**：三处修正清单（async_trait/ctx 简化/超时真注入）
  全部来自写后自查，未等编译错误兜底。
- streaming clientAttempt 备案评估结论入档（同 thinkingBudget 先例）；
  SSRF 初判防线的 rebinding/重定向旁路备案 R40+。
