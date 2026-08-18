# 138 号（Phase3）：生产技能绑定——production-bindings 移植

日期：2026-08-18 · 前置：130 号（builtin 技能装载）/ 137 号备案项

## 背景

TS `skills/production-bindings.ts`（合并 e7c04465 新增）：生产模式与内置
技能包的绑定表 + 激活解析/合并工具；agent-session 以 `workerSkills` 闭包
（architect/writer → longWriting；auditor/reviser → longReview）供 sub_agent
工具合并技能指导。Rust 缺失。

## 实现

### A. 绑定模块（`src/skills/production_bindings.rs`，TS 逐字）

- `ProductionSkillCapability`（8 能力）+ `production_skill_ids` 绑定表
 （longWriting=[inkos-long-writing]；longReview=[long-writing, story-review]；
  short/play/script/storyboard/interactive-film/translation 各一）+
  `NON_LONG_PRODUCTION_CAPABILITIES`。
- `ActivatedSkillResource`/`ActivatedSkillGuidance` 类型
 （skill + 已加载参考资源——TS skill-tool 形态）。
- `resolve_production_skill_activations`：绑定表 ∩ 可用技能（缺失静默
  跳过），resources 恒空（引用按需加载属 use-skill 工具面）。
- `merge_activated_skill_guidance`（按 id 去重、后写胜、保序）/
  `activated_skill_ids`。
- `worker_skills_for_agent`：TS agent-session workerSkills 闭包逐字。

### B. 指导拼接（agents 域，TS `appendActivatedSkillGuidance` 逐字）

`append_activated_skill_guidance`：激活技能 → "## Activated professional
skills" 指导块（技能条目 `### id — name` + body/description + 参考资源
`#### Reference: path:charStart-charEnd · heading`）拼进 system 消息
（无 system 前置一条）。

### C. 范围裁定（备案）

**消息级注入接线**（pipeline agent 调用链深处把 worker 指导注入 LLM
消息）留 139 号与 runner harness 类重构同轮——TS 的注入点就是 harness 的
agent 上下文装配（agentCtxFor → appendActivatedSkillGuidance 调用链），
分开做必然错位。本轮交付绑定/解析/合并/拼接全套积木 + 解析形态验收。

### D. 测试（+5）

- 绑定表逐字（8 能力 + NON_LONG 六项）。
- 绑定 ∩ 可用（缺失静默跳过 / 全缺失空 / resources 恒空）。
- 合并去重（后写胜 + 保序）。
- worker 闭包绑定（architect/writer/reviser/auditor/exporter + 空可用集）。
- 指导拼接（无 system 前置 / 有 system 追加不替换 / 空激活零改动 /
  Reference 行格式）。
（requested/disabled/forced 解析形态既有 registry 测试在册——130 号
last-write-wins 修正后断言齐全。）

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1195** 过（+5） |
| `cargo test --test e2e_write_next_contract` | **191** 过 |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1353** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. runner harness 类重构（agentCtxFor/worker-agent）+ **worker 技能指导的
   消息级注入接线**（本轮积木的消费端）。
2. script-storyboard / translation runner 移植（接入 137 号同事务面）。
