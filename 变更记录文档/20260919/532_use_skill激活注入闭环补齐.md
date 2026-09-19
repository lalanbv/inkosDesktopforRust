# 532 号：use_skill 工具面双端对照走查——激活注入闭环补齐（回合技能集）

日期：2026-09-19
提交：本文件同批路径限定提交
类型：fix(engine)——240 号移植欠账 + 532 走查发现

## 走查背景与双候选结论

531 号备案双候选，走查结论：

1. **writer prompt pack 层 golden 评估 → 排除**：`withPromptPackGuidance`/
   `append_prompt_pack_guidance` 双端均已移植且接线等价（Rust writer.rs:1671 对
   system prompt 包 pack 层，与 TS writer.ts:208 同构），无漂移、无 golden 需求；
2. **use_skill 工具面走查 → 发现真漂移（本轮修复）**。

## 走查发现：激活注入闭环断裂

use_skill 工具本体（240 号移植）执行面逐字对齐 ✓：disabled/registry 校验、
resourcePath 安全面（safe_child_path + 512KB 上限 + UTF-8 空字节检查）、query 分支
内存 BM25 检索、返回文本格式（Skill activated/Purpose/Static resource/This skill
provides instructions only）。

但 TS 侧 use_skill 不是孤立工具——它有**激活闭环**：

- `onActivate: (activation) => turnSkills.set(activation.skill.id, activation)`
  （agent-session.ts:1166）——激活写回**回合技能集**（每轮由
  skillResolution.usedSkills 重建、轮末随会话缓存重置）；
- 同轮后续 `sub_agent` 调用时
  `mergeActivatedSkillGuidance(workerSkills(agent), activeSkills())`——动态激活与
  worker 绑定合并（后写胜）注入 worker system。

Rust 侧 sub_agent_tool.rs 自 139 号起留有注释承认缺口："Rust 会话面无 use-skill
工具（激活集空），合并结果即 worker 绑定"——**240 号把工具本体移植进来后没回头
接这个合并位**：用户在聊天轮里 use_skill 激活技能后，同轮 sub_agent 的注入集不变，
TS 有而 Rust 无。

TS activeSkills 的其余消费面（fanfic/continuation/spinoff/imitation/shortFiction/
script/storyboard 等建书工具构造参数）在 Rust 侧无对应工具，本轮不涉及。

## 修复（回合技能集四件套）

- `skills/production_bindings.rs`：+`TURN_SKILLS` task-local（Arc<Mutex<HashMap>>，
  值内可变支持工具运行时写回）+`scope_turn_skills`（包整轮工具循环）+
  `activate_turn_skill`（TS turnSkills.set 对应物；无 scope 静默）+
  `turn_skill_activations`（TS activeSkills() 快照；无 scope → 空）；+回合语义单测
  （无 scope 写读均空 / scope 内写后读可见、同 id 后写胜 / 轮末丢弃）；
- `interaction/skill_tool.rs`：tool_use_skill 成功路径写回回合集，resources 语义
  对齐 TS onActivate（resourcePath → 全文单段 char 0..len；query → 检索段；
  无 → 空）；+scope 内外集成测试；
- `interaction/sub_agent_tool.rs`：worker 注入改为
  `merge_activated_skill_guidance(&[&worker_binding, &turn_skill_activations()])`
  （合并序与 TS 一致：worker 绑定在前、动态激活后写胜），139 号陈旧注释更新；
- `server/agent_route.rs` post_agent：`run_agent_loop` 外包 `scope_turn_skills`
  （135 号回合轨迹作用域同款嵌套模式）——工具循环内 use_skill 写、sub_agent 读、
  轮末随 scope 丢弃。

## 门禁插曲：管道统计伪象（第二次）

`cargo test 2>&1 | grep "test result" | awk` 管道在 cargo 增量输出时截断，报出
"engine 1386/failed 1、src-tauri 536/failed 1"的假象（failed 计数来自截断行碎片）；
落盘复读求和实为 **engine 1806 / src-tauri 578，全部 0 failed**。与 527 号"输出
截断漏主 lib result 行"同源第二次——**cargo 门禁统计一律落盘再解析**，自此为硬惯例。

## 门禁（8 项全绿）

| 门禁 | 结果 |
| --- | --- |
| cargo test engine-rs | 1806 passed / 0 failed（+3 新测试，落盘求和） |
| cargo test src-tauri | 578 passed / 0 failed（落盘求和） |
| clippy:gate（--all-targets 双 crate） | 0 告警 |
| gate:ts:fast | typecheck 17.5s + test 75.8s + audit:npm 全绿（本轮零 TS 改动） |
| 契约差分器（活体） | 41 对照 0 分歧 |
| node-fallback-smoke | 双引擎一致性全部通过 |
| export-epub-smoke | EPUB 结构校验通过 |
| INKOS_DUEL=1 strangler_duel | 10/10 真跑 |
| bench:gate | 负载窗口轮询通过（用户应用高峰 37%/核多轮拦截后低谷执行），9 基准零回退 |

## 教训

- **移植"工具本体"≠移植"工具的会话语义"**：240 号对照面是 use_skill 的执行契约，
  而 TS 里它经 onActivate 参与会话状态闭环——对照审计的边界应该是"行为闭环"而非
  "函数签名"。139 号预留注释（"合并结果即 worker 绑定"+激活集空前提）是现成的
  追踪锚：**注释里写明的临时前提，后续号动到相关面时必须复核**；
- task-local 值内可变（Arc<Mutex<Map>>）是 Rust 侧"轮级可变会话状态"的低成本
  形态：scope 定生命周期、try_with 静默降级，不侵入既有请求结构体；
- cargo 管道统计伪象第二次：`| grep | awk` 在增量编译输出时截断 → 假 failed 计数，
  一律 `> file` 落盘再解析。

## 下一号

自 533 起（开号前双查当日目录最大号）。备案候选：聊天轮 use_skill 激活的
RunLog/tool_executions 留痕面核对（details.kind=skill_activated 的前端消费）；
en-prompt-sections 残余面复核（529 号清偿后 grep 复查）。
