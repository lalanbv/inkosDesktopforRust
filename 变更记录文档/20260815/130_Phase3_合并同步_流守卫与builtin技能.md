# 130 号（Phase3）：上游合并同步——流活性/完整性守卫 + builtin 技能装载 + 契约面数据对齐

日期：2026-08-18 · 提交：见 git log（130号）

## 背景

合并 d3ba425e 引入上游大量更新（236 文件，+13947/-6829，核心为 e7c04465
"unify production harness and context flow"）。engine-rs 与 sidecar 入口
（server.ts）未被合并触及，但：

1. **契约面数据漂移**：sidecar `/api/v1/skills` 从 configured-only 升级为
   `loadAvailableAgentSkills`（builtin + configured）；builtin 技能包目录
   `packages/core/skills/`（15 个专业包）为合并新增。duel 全灰测试捕获该
   分歧（Rust 1 技能 vs TS 16 技能）。
2. **LLM 传输可靠性缺口**：TS provider.ts 新增流不活动看门狗
   （`LLMStreamInactivityError`）与流完整性守卫（finish_reason 终态追踪
   + output-limit 拒绝），Rust streaming 面完全无超时（裸
   `reqwest::Client::new()`，挂起流会永久卡死写管线）。
3. **TS 测试击穿**：上游删除 `agents/length-normalizer.ts`、
   `writer.buildUserPrompt`、`extractQueryTerms`、`extractRelevantThreads`
   等，我们创建的 golden-leaf-dump.test.ts import 失败（195 文件中唯一
   失败）。

## 实现

### A. 流不活动看门狗 + 完整性守卫（streaming_client.rs / sse_parser.rs）

- `StreamDeadlineSpec { first_event_ms, idle_ms }`：INTERACTIVE（120s/90s，
  TS guardAssistantMessageStream 默认）与 PIPELINE（300s/180s，TS
  chatCompletion 流式默认）双面；解析序 env
  （INKOS_LLM_FIRST_EVENT_TIMEOUT_MS / INKOS_LLM_STREAM_IDLE_TIMEOUT_MS）
  > 显式覆盖 > 面默认（TS readPositiveTimeout 嵌套语义逐字：trim + f64 +
  floor，非有限/≤0 回退）。纯核心 `resolve_deadline` 可测。
- 看门狗布防于 send 之前（首事件窗覆盖连接+响应头+首块——TS 同一时钟窗）；
  流循环 `tokio::time::timeout_at`：首块前 FirstEvent 阶段，其后每块重置
  Idle 窗。chat 与 responses 两传输同构（TS 两传输共用 deadline）。
- `StreamError::Inactivity { stage, timeout_ms }` 错误文案与 TS 逐字：
  "LLM stream produced no event within {ms}ms" /
  "LLM stream produced no new event for {ms}ms"。不参与重试（TS
  isRetryableLLMError 白名单不含；文案不落 transient 短语集）。
- `SseEvent::FinishReason(String)`：sse_parser 块级返回改 Vec（content 与
  finish_reason 同帧双双产出——TS 逐项处理语义；顺带 content+reasoning
  同帧不再互相遮蔽，pi-ai 双分支语义）。空帧占位事件移除。
- chat 流式终态守卫（TS 守卫序逐字）：`finish_reason ∈ {length, max_tokens}`
  → "model reached the output limit ({reason})"；空响应（reasoning-only →
  "LLM returned reasoning without a final answer"；全空 → "LLM returned
  empty response from stream"；**工具调用轮豁免**——TS 自定义传输不做工具
  聚合，此为 Rust 工具能力的适应性扩展）；缺终态 → "stream closed without
  [DONE]/finish_reason"（网关掐断=截断非完成）。
- 接线：AgentRouter::chat → PIPELINE（TS BaseAgent.chat 面）；
  RouterLoopChat（agent_route 交互聊天）→ INTERACTIVE；service_routes 探测
  → PIPELINE（外层 8s 探测窗主导）。
- 测试 +15：解析序/文案/挂起服务集成（首事件窗 150ms 掐断、空闲窗 150ms
  掐断、四守卫负向、finish_reason+content 正向、工具轮豁免）。

### B. builtin 技能装载（skills 面）

- `SkillSource::Builtin`（TS z.enum "builtin" 补齐；ts-rs 类型同步）。
- `builtin_skills_root()`：env `INKOS_BUILTIN_SKILLS_DIR`（genres 同款约定）
  > cwd 相对默认 `assets/skills`（绝对化——discover 强制绝对路径）。
- `load_builtin_agent_skills` / `load_available_agent_skills(_with_builtin_root)`：
  TS loadBuiltinAgentSkills / loadAvailableAgentSkills 对应物（builtin 在前
  configured 在后——同名后者覆盖）。**差异备案**：TS builtin 根 file-relative
  恒存在、缺失即端点 500；Rust 默认路径独立部署可能未部署——缺失静默降级
  为零 builtin，非缺失错误计 diagnostics（不炸端点）。
- **registry 去重缺陷修正**：`create_skill_registry` 原 `or_insert` =
  first-wins；TS `dedupeSkills` 用 `Map.set` = **last-write-wins**。改为
  `insert`——项目/用户同名覆盖 builtin 默认的正确语义（builtin+configured
  合并前不可见，属真实移植缺陷）。
- `/api/v1/skills` 切换 `load_available_agent_skills`（sidecar 合并后等价）。
- duel fixture：write_fixture 设进程 env + 3 个 bin spawn 点显式注入
  （repo packages/core/skills——TS file-relative 同一目录）。

### C. longform 内置提示词文案同步（prompts/mod.rs）

duel `/api/v1/prompt-packs` 捕获：上游为 longform writer/reviser/auditor
新增"精确放置/伏笔复用"纪律段（exact placement 字面验收标准 / hook 不得
改名重开 / 修复先于润色 / 审计先枚举约束再打分）。Rust 三常量按
builtin-prompts.ts 逐字同步。

### D. TS golden-leaf-dump 修复 + golden 结构同步

- TS dump 测试：移除已删函数的六套件（length_normalizer_suite、
  writer_build_user_prompt、writer_extract_dialogue_fingerprints、
  writer_find_relevant_summaries、extract_query_terms、
  planner_extract_relevant_threads）+ 相应 import/断言；planner 输入面
  lengthSpec → lengthBudget 适配。TS 基线恢复 **195 文件 / 1859 测试全绿**。
- leaf.json 再生；golden_leaf.rs 同步删六字段+六测试。
- **值级漂移备案（9 套件 #[ignore]）**：build_length_spec（normalizeMode
  字段移除）/ settler 双提示词 / planner 双提示词 / writer governed /
  state-validator / reviser / parse-memo——上游 e7c04465 改动了这些纯函数
  与提示词输出。**131 号轮按新 golden 逐面移植**（golden 差分已捕获，先
  备案后 remediation，不静默删除断言）。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1184** 过（+15） |
| `cargo test --test golden_leaf` | **61** 过 + 9 ignored（131 号备案） |
| `cargo test --test e2e_write_next_contract` | **188** 过 |
| `cargo test --features export-bindings --lib` | **1343** 过（+15） |
| `cargo clippy --all-targets` | 零警告 |
| TS vitest（packages/core） | **195 文件 / 1859** 全过（恢复） |
| duel 对跑（INKOS_DUEL=1） | **8/8**（skills builtin / prompt-packs 双端等价恢复） |

## 合并漂移全景备案（后续轮路线）

1. **131 号（下一轮）**：纯函数/提示词面同步——9 个 ignored golden 套件
   按新 TS 真值移植（normalizer 阶段移除连带 normalizeMode 字段与
   choose_normalize_mode 删除）。
2. runner.ts harness 类重构（agentCtxFor / writeProductionRunSnapshot /
   settleChapterState 修复路径）对 write_next.rs 的结构性对齐。
3. writer.ts 瘦身（-476）→ worker-agent.ts / production/harness.ts 新
   pi-harness 架构的语义对齐。
4. skills 专业包与 production modes 绑定（agent-session
   loadAvailableAgentSkills 面）。
5. think 剥离器（MiniMax 内联 `<think>` 块，issue #329）与 trajectory
   遥测头（agentTrajectoryHeaders）。

## 风险与边界

- 看门狗非流式路径不设防（TS client.stream 同款门；TS undici 300s 隐式
  bounds vs Rust 无界——备案）。
- duel env 注入在并发测试间写同值（无害，已注记）。
- 129 号 reasoning 同帧遮蔽断言更新为双事件（pi-ai 双分支语义，聚合消费
  面不变）。
