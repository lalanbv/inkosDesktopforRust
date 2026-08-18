# 107 号变更记录：prompt-pack 消费面收口（play/film）+ env 族对账勘误

## 一、背景

106 号候选：env 余量小件批。勘测发现两项**对账勘误**与一项真缺口：

1. **103 号审计开放清单 #1（prompt-pack 体系）实为已闭合**——Rust `prompts/prompt_pack` 模块（三级加载链 + builtin 12 件 + append 段逐字）与 longform 三消费点（writer.rs:1603 / reviser.rs:584 / continuity.rs:1101）及 REST 域（60 号 skill_routes 的 prompt-packs 三端点）均已移植；74/76/77/82 号"未接"备案为陈旧记录。**真缺口是 play 面与 film 面的消费点**（TS play-agents.ts mutator/renderer + film-authoring-tools 的 draft_structure）。
2. **INKOS_SKILL_DIRS 已接**（60 号 `env_skill_dirs`，`std::env::split_paths` 语义同 TS delimiter 分隔）——106 号对账表误标"未接"。
3. **INKOS_USER_AGENT 两侧均为常量**（TS `const INKOS_USER_AGENT = "InkOS/1.3.5"` 无 env 读）——106 号对账表"TS env 可覆盖"表述勘误。
4. **INKOS_FILM_IMAGE_SIZE**：TS `node-image.ts` `params.size ?? env ?? "1024x1536"`——Rust film 面未读 env（唯一真 env 缺件）。**74 号备案 #3 勘误**：play 面 TS 同为固定 `1024x1024`（play-image.ts:194 无 env）——非差异。
5. INKOS_DEFAULT_LANGUAGE 属 56 号请求级 env 合并族（`resolveEffectiveLLMConfig` 内），维持备案。

## 二、交付

### 1. play 面附加段（`play_runner.rs`）

- `PlayAgents` 增 `root: &Path`（三处构造点同步：agent_production play 执行器 + play_tools 两处）。
- `mutator_system_prompt` / `renderer_system_prompt` 助手：基础提示 + `append_prompt_pack_guidance`（TS play.mutator / play.renderer；加载异常防御回退基础提示）——**项目覆盖 `prompt/play/mutator.md` 即可定制世界规则指引**。
- 单测：项目覆盖 → `(play.mutator, source: project)` 段 + 覆盖内容；无覆盖 → builtin 段。

### 2. film 面附加段（`agent_production.rs`）

- `execute_draft_structure` 系统提示经 `append_prompt_pack_guidance(interactive-film.story-graph)`（TS draft_structure 工具同款）。TS 确认式 script/storyboard/interactive-film runner 与 Rust 同样不附加（无缺口）。

### 3. env 缺件（`interactive_film_routes.rs`）

- film 生图尺寸链补全：`body.size ?? env INKOS_FILM_IMAGE_SIZE ?? "1024x1536"`（TS node-image 逐字）。

### 4. 对账勘误（见第一节）——103 号审计 #1 与 #8 族勘正

prompt-pack 体系状态从"开放"改为"已闭合（本轮补齐 play/film 消费点后全覆盖：longform 三件 + play 两件 + film 一件 + REST 域）"。

## 三、parity 要点

- 附加段头 `## Prompt Pack Guidance ({id}, source: {source})` 与三级加载链（project `prompt/<id 路径>.md` → user → builtin）为既有逐字实现；本轮仅补消费点对齐 TS 消费矩阵。
- film 尺寸链 env 位次逐字；play 面 1024x1024 固定（勘误后确认两侧一致）。

## 四、偏差备案

1. **play 面附加段仅 mutator/renderer**：TS 同（play.start/reconciler/image 三 builtin id 无核心消费点——pack 清单展示用）。
2. **film-authoring 聊天工具族（fill-node 等）**：Rust 无该聊天工具面（会话工具矩阵既有状态，94/103 号矩阵备案），附加段随工具面若后续接入再接。
3. logDroppedMutationItems（TS mutator 可观测性日志）维持 82 号备案。

## 五、暂缓件（滚动，勘误后）

103 号开放清单更新：#1 prompt-pack **闭合**；#8 已闭合（105 号）；env 族仅余 INKOS_DEFAULT_LANGUAGE（随 56 号合并族）。余：provider 特判族（#5，95 号架构备案）、pi-ai 模型卡（#6 维持）、Scheduler 余量（#7）、聊天卡 details/SSE 结构化（#9，前端契约前提）、同步钩子族（#10）、回放治理输入（#11）、评审轮数配置位（#12）、authoring 边角（#14）、56 号 env 请求级合并族。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1152** 过（+1：play 附加段） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | 172 过（PlayAgents 五处构造随字段更新） |
| `cargo test --features export-bindings --lib` | **1311** 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（108 号候选）

1. **首选：strangler 实切演练**（98 号 runbook 只读面起跑）——功能面协议/提示词/工具矩阵对账后无已知缺口；需真实运行环境与流量（请求用户提供：Rust engine 起动 + 真实 LLM 端点配置）。
2. 其次：56 号 env 请求级合并族（resolveEffectiveLLMConfig 全量——INKOS_LLM_PROVIDER/SERVICE/STREAM/TEMPERATURE/THINKING_BUDGET/PROXY_URL/HEADERS/EXTRA_*，多环境部署前的最后 env 面）。
3. 或：聊天卡 details 外露 + SSE tool:end 结构化 result（#9，前端契约确认前提）。
