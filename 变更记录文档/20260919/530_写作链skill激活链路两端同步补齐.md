# 530 号：写作链 skill 激活链路两端同步补齐（craft 方法回归主力生产路径）

日期：2026-09-19
提交：本文件同批路径限定提交
类型：feat(engine + core + studio)——上游未竟意图补齐

## 定性纠错（529 号立案前提修正）

529 号立案"Rust 写作链 skill 激活通道移植（三件套）"的前提是"TS 有、Rust 无"。本号
开工侦查推翻了这一前提：

- **三件套在 Rust 侧已存在**（139 号已落）：`production_bindings.rs` 完整
  （绑定表/resolve/merge/worker_skills_for_agent/OPERATION_SKILLS task-local），
  `append_activated_skill_guidance` 在 agents 模块，`AgentRouter::chat` 出口注入
  已实现（agent_router.rs:236）；唯一 set 点在交互面 `sub_agent_tool.rs:56`；
- **TS 写作链同样无注入**：`operationContext` 只在 `runWithAgentContext` 建立，
  唯一调用点 `runWithAbortSignal` 只传 signal（runner.ts:450），writer 全部创建点走
  `agentCtxFor` → `currentActivatedSkills()` 恒 undefined → hydrate 直通（skill-tool.ts:223）
  → `appendActivatedSkillGuidance` 对 undefined 原样返回（base.ts:192）。runner 里
  两处显式接线（style analyzer :2990 / canon :3193）在写作链上下文里同为无源之水；
- server 侧仅 interactiveFilm/translation 两个交互面工具传 activatedSkills。

**真相**：c56586ec 把 craft 方法移入 `inkos-long-writing` skill（SKILL.md 自述
"Used by InkOS long-form workers as their shared craft method"）后，只有聊天面
sub_agent 路径接了绑定（writer→longWriting），**写作链直呼路径（桌面端主力写章）
两端都断了 craft 注入**——同一 writer agent 两条路径行为分裂。这是上游重构的未竟
意图，非有意去除。

裁决：两端同步补齐链级激活（不是 Rust 单方面领先，不违反移植纪律）；508 号
"半成品接线补齐"先例。

## 改动

### Rust（engine-rs）

- `skills/production_bindings.rs`：+`writing_chain_activations`（longWriting 绑定 ∩
  可用技能，空交集 → None 保持无注入）+`scope_operation_skills`（task-local 作用域
  包裹 future）；+2 单测（交集解析、scope 内外可见性）；
- `server/books_routes.rs` `run_draft`：draft 路由装配——`load_available_agent_skills`
  → 解析 → scope 包住 `write_next_chapter`（链内全部 AgentRouter::chat 出口注入）；
- `server/ops_routes.rs` `write_one_chapter`：连写循环同款装配。

### TS（core + studio）

- `pipeline/runner.ts`：`PipelineConfig` +`activatedSkills?` 字段（不传 = 现状）；
  `writeNextChapter` / `writeChapters`（整批一个 context）/ `writeDraft` 三入口包
  `runWithAgentContext({ activatedSkills: this.config.activatedSkills }, ...)`——
  agentCtxFor 既有透传（runner.ts:766 不分 agent）使链内全部 worker 生效；
- `studio/server.ts`：+`buildPipelineConfigWithWritingChainSkills`（基础装配 +
  `loadAvailableAgentSkills` → `resolveProductionSkillActivations(skills, "longWriting")`，
  交集空 → 不带字段）；write-next 与 draft 两个路由改用；
- `pipeline-runner.test.ts`：+透传测试（config.activatedSkills → writer ctx 断言）。

### 注入语义（双端对称）

链级注入：写作链内 planner/composer/writer/auditor/reviser/settler 的每次
LLM 调用 system 消息追加 `## Activated professional skills` 段（craft body 约
1.2KB）。TS 走 BaseAgent.chat 出口 `appendTaskSkillGuidance`，Rust 走
`AgentRouter::chat` 出口（139 号等价集中点）——拼接格式双端码点一致（Rust 侧
append 已有格式断言单测），skill body 双端部署等价（489/501 号证实）。

## 门禁插曲（两处真实缺陷，均已修复）

1. **writeDraft 重写吞行**：python 结构化包裹时 `ensureControlDocuments(bookId)`
   被留在旧头部未带入新箭头函数——"bootstraps missing control documents" 测试红
   （ENOENT author_intent.md）抓获。教训：结构化重写后必须核对方法体首行语义锚点，
   不能只看类型检查（语法合法但语义丢行，tsc 不报）；
2. **core dist 陈旧产物**：`packages/core/dist/pipeline/runner.d.ts`（09:48 旧构建）
   缺 planChapter/composeChapter 声明，studio 的 `tsconfig.server.json`（extends 根
   配置无 paths 源码映射）解析到 dist → TS2339。`pnpm --filter @actalk/inkos-core
   build` 重建后消解（dist 不入库）。教训：server 二段 typecheck 依赖 core 构建产物
   新鲜度，gate 前先 build core。

## 门禁（8 项全绿）

| 门禁 | 结果 |
| --- | --- |
| cargo test engine-rs | 1803 passed / 0 failed（+3 新测试） |
| cargo test src-tauri | 578 passed / 0 failed |
| clippy:gate（--all-targets 双 crate） | 0 告警 |
| gate:ts:fast | typecheck 17.3s + test 75.6s + audit:npm 全绿（两处插曲修复后） |
| 契约差分器（活体） | 41 对照 0 分歧 |
| node-fallback-smoke | 双引擎一致性全部通过 |
| export-epub-smoke | EPUB 结构校验通过 |
| INKOS_DUEL=1 strangler_duel | 10/10 真跑 |
| bench:gate | loadavg 2.49（13.8%/核）窗口通过，9 基准零回退 |

## 教训

- **立案前提要当轮验证**：529 号"三件套移植立案"基于对 TS 侧的推断（"TS 有"），
  实际 TS 也没有——立案信息若未经源码钉死，下一轮开工第一件事就是验证前提；
- **"接线存在"≠"通路活着"**：TS 的 appendActivatedSkillGuidance 接线遍布三层
  （agentCtx/runner 直呼/base 出口），但上游 store 无人 set 时全是死接线。判定
  通路活性要追到数据源（谁 set），不能停在调用点存在性；
- python 结构化重写长函数后，语义锚点（副作用首行）必须人工核对——类型检查对
  "丢行"天然免疫；
- gate:ts 的 studio 二段 typecheck 消费 core dist，改动 core 后先 build 再 gate。

## 下一号

自 531 起（开号前双查当日目录最大号）。备案候选：skill 激活的 run-log 留痕面
（skill_ids 快照已有、激活正文无留痕）；写作链 golden 补 chat 出口注入段形态
（append 后 system 消息码点级锁定）。
