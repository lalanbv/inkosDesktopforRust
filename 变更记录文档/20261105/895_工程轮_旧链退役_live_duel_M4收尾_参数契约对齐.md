# 895 号 · 工程轮：旧链退役（live duel 证据）+ M4 收尾（能力动作参数契约对齐 + 工具注册表评估）

> 894 备案两件收口：78 号旧链三件 duel 绿后删除（695 纪律收官）；M4 schema 参数面
> 对齐 TS OutlineParameters/ShortRevisionParams + agent_loop 工具注册表评估。
> testgate 2039+584、clippy 双 0、gate:ts 八步全绿、交替互证 33/33、U+FFFD 净。

## 一、旧链退役（live duel 证据轮）

### 消费方普查（删除前）

- `pipeline/short_fiction_runner.rs`（830 行）：**零外部消费方**（893 切链后
  execute_short_run 已走 `production::short_fiction_runner`；同型符号
  `run_short_fiction_production`/`ShortFictionRunOptions` 全部命中新链命名空间）。
- `agents/short_fiction.rs`（1010 行）：唯一消费方 = 旧 runner。
- `prompts/short_fiction.rs`（760 行）：唯一消费方 = 旧 agents。
- cover.rs 旧入口**非死代码**：`resolve_cover_generation_request`/
  `generate_image_from_prompt`/`build_cover_image_prompt`/
  `generate_short_fiction_cover` 经 `generate_cover` 工具
  （execute_generate_cover）+ play/interactive-film/film-authoring 三路活体——
  不删；旧链特有的封面胶水随 runner 内联消失，无级联。
- TOOL_LABELS 无残留（`short_fiction_run` 是活链工具名）。

### live duel（INKOS_DUEL=1 真跑对照——duel-requires-inkos-duel-env 纪律）

证据器械 `tests/short_fiction_retirement_duel.rs`（**退役提交中与旧链同删**——
旧链 API 不复存在即不可编译；新链常驻覆盖 = runner 单测 + e2e）：
同一 mock LLM 双形态分流（tools 在场 = 新链 result-tool 脚本非流式；
stream+无 tools = 旧链按五 system 提示词逐字标记路由 + 消息数区分 base(2)/
revise(4) 轮，SSE 线格式同 strangler_duel 548 号先例）。两侧同走真实 LLM 栈
（旧链 AgentRouter.chat 默认流式 tagged 协议；新链经 agent 路由结构化 worker）。

**绿跑输出（逐字）**：

```
running 1 test
duel 绿：旧链 7 调用/12 章/修订稿终稿/重放短路；新链 7 轮/2 章/work 域/重放零调用。
test short_fiction_old_new_chain_retirement_duel ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.01s
```

**负控（未设 env，不计证据）**：

```
INKOS_DUEL 未设——跳过（退役对照属证据轮，不进基线）
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 语义不变量对照（duel 裁决面）

① 同 direction 双链各自成稿：标题一致性（标题→final 头→包装 title）；
② 章节标题渲染恒等 `## 第N章 <题>` 双链同形；③ 包装四字段（intro/selling
points/cover prompt）sales-package.md 同名同结构；④ 审稿工件 reviews/
draft-v001.md 双链落盘非空；⑤ cover=false → coverError=disabled 双链同语义
不 fail；⑥ 幂等：旧 already-complete 短路（重跑零调用）/ 新 production-state
重放（零调用）；⑦ 数据流：旧链 outline v002 → writer 请求闭环（修订版标记
断言）+ 新链 outline → batch 脚本序闭环。

**契约差异（886 备案的刻意升级，非 duel 失败项）**：章数旧钳 12-18 vs 新
min(1) 按请求；产物旧裸文件目录 shorts/\<id\>/ vs 新 work 实体 + episodes +
交付合同；修订旧恒一次修订失败降级 vs 新 checkpoint 编排 + scoped 执行分支 +
author 权限面。

### 删除面

三件整删（2630 行）：pipeline/short_fiction_runner.rs、agents/short_fiction.rs
（含旧六代理+旧测试）、prompts/short_fiction.rs（含 760 行提示词构造）+ 三处
mod 声明 + duel 器械同删；注释四处更新为退役收官形态
（production/short_fiction.rs、short_fiction_prompts.rs、production/mod.rs、
agent_production.rs execute_short_run 头）。删除后全仓 grep 零残留引用，
cargo check --all-targets 净。

## 二、M4 收尾：能力动作参数契约对齐（short_capability_route.rs）

勘察实证三处跨引擎漂移，逐一修复：

1. **per-action 参数契约校验器**（TS harness/tools/short-production.ts 四 schema
   + agent-tools.ts ShortRevisionParams 的 Value.Decode 语义）：声明键类型+
   minimum 强制、未声明键透传（TypeBox additionalProperties 缺省开放）——
   Outline=无 maxChaptersPerCall/reviewScope 声明；Draft=无 reviewScope；
   Review=仅 reviewScope；Package=仅 direction；revise=instruction/
   resumeOperationId(minLength 1)/restartPendingRevision/chapterCount/
   charsPerChapter/maxChapterLength/chapterNumbers(int≥1 数组)。违约 →
   `CAPABILITY_PARAMS_INVALID`，文案镜像 capability-registry.ts decode 失败
   形态（`Capability short-fiction.<action> params decode failed: /path: msg`）。
   `Type.Union([Integer(minimum:1),Null])` 三 length 键 null 合法（继承面）。
2. **direction 回退链 + outline 空方向守卫**（TS L45 逐字）：params.direction
   在场即用（含空串——`??` 不穿透）→ 缺席时 production-state intent（含空串）
   → work.metadata.intent → ""；outline 全链空 = TS 逐字
   `"An outline needs a creative direction"`（新码 SHORT_OUTLINE_DIRECTION_
   REQUIRED）。此前 Rust 无 metadata.intent 层、无空方向守卫。
3. **production 门**（TS `stage==='outline'||stage==='draft' ? params : {}`）：
   chapters/charsPerChapter/min/max/openingHook/maxChaptersPerCall/coverRequired
   仅 outline/draft 阶段读取（review/package 不再透传产出参数）；reviewScope
   全阶段透传（TS `'reviewScope' in params` 语义）；coverRequired 接通
   （此前 Rust 面完全丢失该键）。
4. **revise 面双修**：①键名 `chapters` → `chapterCount`（TS ShortRevisionParams
   逐字——旧键从未被 TS 声明，Rust 误读致入参静默丢失）；②revise runtimes
   writer 上下文 `short-writer` → `short-reviser`（TS createShortFictionReviseTool
   L1836 逐字——模型/温度解析面分链；e2e 单端点 mock 不区分 context 故 894
   未见）。

### 测试（capability_params_contract_895，e2e 六组全绿）

direction 非串/chapters=0+null 继承/outline 空方向守卫（TS 文案逐字）/
metadata.intent 回退成稿（production-state intent 断言）/review+package 声明键/
revise chapterNumbers+chapterCount——违约全程零额外 LLM 调用（校验先于工件门）。
既有 capability_stage_actions_episode_flow + short78 三组回归绿（happy path
行为不变）。

## 三、agent_loop 工具注册表评估（备案，无需接线）

TS 侧四 stage 工具与 revise 工具注册于 **harness capability registry**
（production-capabilities.ts L300-301），由 Studio work-inspector 代理循环经
capability-registry.ts invoke 消费；artifact-edit-policy 的 revise 意图映射为
Studio 侧策略面。引擎聊天循环（agent_route/propose_action）无 capability 工具
注册表结构——引擎对等物 = capability HTTP 路由本身（894 落位，本轮契约补齐）；
用户路径由 propose_action 确认流覆盖（893 切链）。**结论：engine 侧无需在
agent_loop 注册**；若未来引擎侧引入能力工具循环，capability 路由 +
本轮参数契约即注册面。

## 四、验证清单

testgate 2039+584（engine-rs 50 目标全绿；旧链测试删除净降 25）/ clippy 双 0
（doc list 项缩进新坑一处即修）/ gate:ts 八步全绿（含 engine-contract-diff）/
交替互证 33/33 / e2e_write_next_contract 212（211+1）/ U+FFFD 净 /
live duel INKOS_DUEL=1 真跑绿（证据逐字存上）+ 负控确认。

## 五、备案（886 设计 §三 全线收官）

- **M1-M4 全线收官**：短篇生产 2.0 链自 887 M3a 起六轮（887/889/890/891/892/
  893/894/895）完成双栈对齐 + 旧链退役，无并链残留。
- Studio 前端 propose_action 确认卡 → capability 动作路由（确认卡面）未动——
  桌面端路径走 agent_route（893 已切新链），不阻塞。
- 引擎侧 agent_loop 能力工具注册表 = 评估备案（上文三），无行动项。
- 出口不变：上游协议扩展或外部事件或新真实需求或下一巡检周期。

**下一号自 896 起；出口不变；用户侧待办=Fork 推送 29 个提交。**
