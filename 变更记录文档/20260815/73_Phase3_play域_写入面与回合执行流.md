# 73 号：play 域 —— 写入面与回合执行流

**日期**：2026-08-15
**阶段**：Phase3 深度收尾（72 号"下一步"首选：play 写入面与 runner——解锁互动世界完整切流）
**契约源**：`packages/core/src/play/play-store.ts`（写方法）、`play/play-reducer.ts`（320 行全量）、`play/play-agents.ts`（三必需代理）、`play/play-runner.ts`（seedOpening/step 主链）、`agent/agent-tools.ts` L1944-L2061（createPlayStartTool）

---

## 一、背景

71 号交付了 play 读取面，但互动世界在 Rust 侧无法启动与推进：写入面（建世界/存状态/记事件）、图谱变更 reducer、回合 LLM 执行流（interpret→mutate→render）与 play_start 确认意图执行器均缺。本号补齐写入链全环——互动世界从启动到回合推进可在 Rust 端闭环。

## 二、交付

### 1. Store 写入面（`src/play.rs` 追加）

`ensure_run`（run 目录骨架 state/projections/summaries/checkpoints）/ `create_world`（safeSegment + 必填 title + 世界 JSON pretty 落盘）/ `save_current_state` / `append_transcript_turn` / `append_event` / `read_events` / `write_projection`（safe_run_child_path 防逃逸）。行追加统一显式 flush（68 号 tokio 缓冲教训固化）。

### 2. 图谱写后端 + reducer（`src/play_graph.rs` 新，~700 行）

- **PlayGraphDb 双后端**：`play.db` 存在 → **sqlite 读写**（四表 CREATE IF NOT EXISTS + INSERT OR REPLACE + expire UPDATE + 事务 BEGIN/COMMIT）；否则 **文件后端**（play-graph.json 四 record，内存事务=克隆回滚）——与 71 号读取面的后端选择完全一致，读写不割裂。
- **reducer 全量**（play-reducer.ts 逐字）：
  - `apply_play_mutation`：玩家 id 规范化（legacy `player` → `actor_player`）→ 实体别名解析（id+label 映射表，歧义别名剔除；边端点 label 自动解析到既有实体 id）→ 事件构造（summary/blockedReason 二选一）→ 校验 → 事务内（记事件 + 非 blocked 图变更）；
  - 校验：状态槽 owner 存在性 + 证据链（实体必须为 evidence/clue、`from` 与当前态一致、**不回退**）；
  - 图变更：**关系边 fail-open**（端点缺失跳过该边而非掀翻整回合）+ **holding 归一**（目标非 item 且无 physical/portable 标记 → 降为 observed）+ **数值槽 min-max 夹取** + 证据状态槽推进（`evidence:{id}:status` 记 previous/status/reason）；
  - `seed_play_graph`：仅图变更不记事件（开场播种）。

### 3. 回合执行流（`src/play_runner.rs` 新，~700 行）

- **三代理 trait**（Send+Sync，注入式）+ `PlayAgents`（AgentRouter 适配，端口名 play-action-interpreter/play-world-mutator/play-scene-renderer）：
  - **interpreter**（0.15/1024）：动作五类归一；fail-open——瞬时错误/不可解析降级为玩家原话作 do；
  - **mutator**（0.25/4096）：状态草案；fail-open——降级为 blocked 无操作回合（带中文原因）；eventId 缺省补 `evt-{turn}`；**重试**（50x/429/超时等可重试错误 ×2 次退避 400ms）；
  - **renderer**（0.45/4096）：场景正文；**永不抛错**——重试 + 一次严格 JSON 追问 + prose 兜底（剥 fence）+ 终态占位"（这一拍悬着，没有落定。）"；suggestedActions ≤4；
  - 提示词 zh 全量逐字（mutator 的品类中立结构/持有物规则/关系边规范/时间同步轴约 20 段；renderer 的接住动作/世界自转/Time 权威等约 12 段 + open/guided 模式差异）。
- **PlayRunner**：
  - `seed_opening`：已有实体/槽 → None；开场播种（turn 0 look 动作 + 专用播种指令——"只播种已成立状态/实物持有必须建实体"三段约束）+ state 投影；
  - `step`：**render 先行、提交在后**（场景到手前不落任何盘——回合 all-or-nothing）；提交链 = 图变更 → events.jsonl → state 投影 → current.json（turn/lastEventId/lastAction/blocked/世界契约）→ scene 投影 → transcript 双轮（user+assistant）。
- `normalize_action_intent`（play_parser 追加）：PlayActionIntentSchema 宽松归一（actionKind 非法回退 do、目标标签 trim 空删、自由文本强转、secondaryActions 过滤）。

### 4. play_start 确认意图执行器（agent_production.rs 接线）

- 装配分支：payload.playStart 必填 title（缺 → 统一 502 错误面中文文案）；params 透传 premise/worldContract/visualContract/mode/initialScene/suggestedActions；**tool 名 `play_start`**（非 sub_agent，无 stages）。
- `execute_play_start`（createPlayStartTool 语义）：**worldId 即会话 id**（1:1 绑定防串台；trim+80 上限+危险字符拒绝）+ runId 固定 main → create_world（zh）→ ensure_run → 首开（transcript 空）：默认开场文本（「标题」+ premise）/payload initialScene → scene 投影 + current state + assistant transcript → **seedOpening fail-open**（失败仅无图谱，不阻断启动）→ details `{kind:"play_world_started", worldId, runId, title, mode, premise, worldContract, visualContract, sceneText, suggestedActions, seedMutation?, graph?}`。
- play_start 在 suppressManualTextForTool 表 → 响应 response 空（前端自绘开场卡）。

### 测试

- **lib 单测（5 个）**：reducer 全量（player 规范化 + label 别名解析 + holding 保持/blocked 事件记录）、fail-open 悬空边 + 数值槽夹取、证据生命周期（推进/previous/回退拒绝/非证据实体拒绝）、**sqlite 后端 roundtrip**（写入后经 71 号读取面读回）、blocked 变体（不落图但记事件带 blockedReason）。
- **E2E `play73_e2e`（3 个）**：**play_start 确认全链**（button 意图 → mock 三代理按系统提示词分流 → 建世界/开场播种/holding 边/完整磁盘形态 + 71 号 GET run 读回同一世界 + response 空断言）；缺 title → 502 AGENT_ACTION_FAILED + 中文文案；**step 全落盘**（evt-1 事件/current turn=1/scene 投影/transcript 双轮/file 图谱实体 + 空输入拒绝）。

## 三、parity 要点

1. **回合 all-or-nothing**：render 成功前零落盘（TS 注释语义——"state advanced but tool failed"半态不可能出现）。
2. **三代理全部 fail-open**：interpreter 降级通用动作、mutator 降级 blocked 回合、renderer 降级 prose/占位——单代理漂移不掀翻回合。
3. **图谱读写后端一致**：71 号读取（play.db 优先）与 73 号写入同序——读写不割裂。
4. **worldId 即 sessionId**：两个 play 会话永不互相推进对方世界。
5. **开场播种是 HUD 增强而非启动前提**：seed 失败世界照常启动。

## 四、偏差备案

1. **sceneReconciler 对账代理暂缓**：TS 的 render 后二次对账（mergePlayMutations 补充合并）；Rust step 直接用 mutator 产物——多数回合行为一致，对账增益场景（场景正文与图谱不一致的修正）后续号接。
2. **regenerateLastTurn 变体面暂缓**：checkpoint（before-turn 快照）/variant 保存恢复链未移植——step 内不做回合前 checkpoint。
3. **en 提示词暂缓**：三代理 zh 全量；en 世界当前回退 zh 提示（模型可处理，非理想）。
4. **play_language 固定 zh**：TS inferLanguage 按内容判 zh/en；Rust create_world 固定 zh（utils 语言判定随后续号复用）。
5. **play_step/play_revise/play_edit 聊天工具面**：回合推进入口当前为程序调用（agent 确认意图 play_start 已接）；聊天内 play_step 工具随 agent 工具面扩展接线。
6. **step 的实体名册**：经 71 号 play_graph_snapshot 快照读取（≤40 条 + 120 字符截断）——与 TS readGraphSnapshot 等价。

## 五、暂缓件（沿 72 号清单滚动）

- 生图链（cover 基础设施——解锁 69 号 node-image 与 71 号 play generate-image 两处 503）
- draft_structure/connect_choice/remove_node 三执行器（域本体就绪，纯接线）
- sceneReconciler + regenerate 变体/checkpoint 面、play en 提示词、play_step 聊天工具
- 单章写作中途截断、actionPayload strict、/agent model 校验、resumeFrom、fetchWithProxy、attachments、模型四层解析

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **1027** 通过（+5） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **119** 通过（+3） |
| `cargo test --features export-bindings --lib` | 1186 通过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：互动世界完整闭环（启动→开场→回合推进→71 号游玩面读取）Rust 端全链可用；POST /agent 的 play_start 确认意图（11 个确认意图累计接通 3 个：write_next/create_book/play_start）。
- **下一步（74 号候选）**：②生图链（cover 基础设施——resolveCoverGenerationRequest + generateImageFromPrompt，一次解锁两处 503）；或 ③draft_structure/connect_choice/remove_node 三执行器纯接线（域本体 69 号已交付，半天量级）。
