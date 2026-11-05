# 896 号 · 验收轮：895 新增逻辑全量测试 + 代码审查 + 模拟真人走查（一患修复 + 全路由冒烟）

> 验收对象 = 895 号新增面（capability 参数契约 + coverRequired + 旧链退役后新链独活形态）。
> 产出：**coverRequired 优先级真患修复**（TS stage-runner L427 `??` 语义缺失）+
> 模拟真人测试器械 `short_capability_router_smoke`（全路由表 19 轮真跑绿，永久入电池）。
> testgate 2040+584（51 目标）、clippy 双 0、gate:ts 八步、交替 33/33、U+FFFD 净。

## 一、代码审查（发现一处真患，即修）

### coverRequired 优先级缺失（TS stage-runner L427 逐字核验发现）

TS `short-fiction-runner.ts` L427：
`coverRequired: options.coverRequired ?? (!stopAfter && !revisionRequest ? options.cover !== false || undefined : previousState?.target?.coverRequired)`
——**显式 coverRequired 优先于 stopAfter/previous 分支**（false = 有别于缺省的显式形态）。
Rust 此前单字段 `cover`，stage 调用分支（stop_after=Some）完全忽略之；895 路由把
coverRequired 误映射到 `options.cover`（stage 调用恒走 previous 分支 → 显式要求静默丢失）。

**修复**（三处）：
1. `ShortFictionRunOptions` 增 `cover_required: Option<bool>`（TS coverRequired 分字段——
   与全链 legacy `cover` 分立；Some(false) 显式禁用 ≠ None 缺省）。
2. runner stage-init 逐字优先级：`options.cover_required.or_else(|| 全链/previous 分支)`。
3. capability 路由 stage_action：`options.cover_required = params.coverRequired`
   （outline/draft production 门内）；`options.cover` 保持 None（TS stage 工具不传 cover）。
   revise 内部 revision_options 字面量显式 `cover_required: None`。

**回归证据**：e2e 双形态断言——params 契约组 plan `coverRequired:false` →
stage 结果面 `coverRequired:false` + production-state `target.coverRequired:false`；
episode 流 draft `coverRequired:true` → `target.coverRequired:true`（显式覆盖 previous）。
produce 全链（stop_after=None）与 revise 路径行为不变（单测 75 全绿）。

### 其余审查面（无患清单）

校验器零分配 happy path（Vec 惰性）/direction 回退链 `??` 语义（空串不穿透）与
`??` 校验先于工件门的错误优先序/production 门 review-package 面/reviewScope 全阶段
透传/revise instruction+resume minLength 面/recovery payload coverRequired:null 在列
（TS L330 对齐）/stage 结果面 cover_required 透传（L2251 ↔ TS L448）/
agent_loop 注册表评估结论维持（引擎对等物=capability HTTP 路由）。
注：profile 守卫先于参数校验（ring 内序）——非短篇 work 的参数违约返回 profile 错误，
与 TS decode-先于-profile 的极端组合序差异备案不修（语义面无消费方）。

## 二、模拟真人测试（新器械：全路由表冒烟，永久入仓）

`tests/short_capability_router_smoke.rs`（INKOS_DUEL=1 门禁——缺 env 早退不计；
已入 cargo-test-gate 电池 = 每轮真跑）：`router_books` **完整路由表**装配
（spawn_rust_engine 同款；进程内 e2e app78 只注册 3 路由，本器械补全路由注册+
中间件/状态装配面；bin 进程装配层由 strangler_duel 既有 bin 测试覆盖）。

**真跑输出（逐字）**：

```
running 1 test
router 冒烟绿：五动作+错误三面+桌面确认流+产物面+L1 读面，19 轮全按序。
test router_capability_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.59s
```

覆盖面：① capability 五动作全流（plan→draft→review→package→revise scoped 链
author_scope→edit_location→chapter_range_replacements→review×2）；② 错误三面
（空方向守卫 TS 逐字文案/参数违约 CAPABILITY_PARAMS_INVALID/未知动作）零 LLM 轮；
③ 桌面确认流（agent 路由 short_run 全链——propose_action 用户路径）+ 结果文本
TS 逐字；④ 产物面（production-state target.coverRequired:false + delivery
checks_passed + final 章题渲染）；⑤ L1 读面（GET works/:id + works 列表）；
⑥ mock 19 轮按序耗尽 + 总调用数恰 19。负控（未设 env）0.00s 早退确认。

**器械自坑（二条，诊断法实证）**：
- **mock `arguments` 必须字符串编码**：`json!({"arguments": arguments})` 写成 JSON
  对象 → 客户端解析 tool_calls 视空 → worker "You did not call" 补轮误触发
  （逐调用探针 + refeed 原文 dump 定位；OpenAI tool_calls.arguments 恒 JSON
  字符串——895 duel 器械 `arguments.to_string()` 同款）。引擎侧无患。
- **outline 提交 storyTitle 必须与 work.title 一致**：种子 title 与提交 title 不符
  → worker 校验回喂重试环（"书名保持原样"约束）——4 消息形态即校验回喂信号。

## 三、验证清单（896 复跑全绿）

testgate 2040+584（engine-rs **51 目标**全绿——router smoke 新入电池 +1 目标）/
clippy 双 0（新器械两处 unused import 即修）/ gate:ts 八步全绿（engine-contract-
diff + export-epub-smoke 并行争用慢化无碍）/ 交替互证 33/33 / e2e_write_next_
contract 212 全绿 / production 单测 75 / U+FFFD 净（四改动文件 0 命中）。

## 四、备案

- 895 交付面经本轮验收成立：参数契约校验器/direction 回退链/outline 空方向守卫/
  production 门/revise chapterCount+short-reviser 双修——模拟真人流全过。
- 模拟真人分层定案：进程内 e2e（handler 契约）+ 全路由表冒烟（路由注册/装配）
  + strangler bin 测试（bin 装配层）三层互补；capability 面的 bin 进程级冒烟因
  门禁对 `Command::new` 新文件的启发式拦截改以全路由表形态承载（覆盖差=进程
  生命周期/env 解析，strangler bin 测试已有同类覆盖）。
- 出口不变：上游协议扩展或外部事件或新真实需求或下一巡检周期。

**下一号自 897 起；出口不变；用户侧待办=Fork 推送 30 个提交。**
