# 256 号：film authoring 第 2 批——authoring 会话工具面全量接线（聊天循环）

- 日期：2026-09-09
- 分支：develop
- 关联：253/254 号（第 1 批 delta builders + 上下文纯函数）、232 号（移植评估）、107 号（draft_structure 生产链先例）
- 推送核验：origin/develop 仍停 6bc65564——**201–254 共 56 提交待推送**；本批随 256 后本地领先 58。两项默认值无新答复。

## 一、移植内容

### 新模块 `engine-rs/src/interaction/film_authoring_tools.rs`（约 900 行含测试）

- **直写四件**（TS film-authoring-tools.ts 逐字描述/返回文本）：`set_world_anchor`（phase→world）、`upsert_characters`（含 TS writeCharacterFacts 移植——motivation/voice 事实写 MemoryDB，story/ 目录 open 前创建）、`add_variable`、`define_ending`。
- **LLM 两件**：`fill_node` / `revise_node`——`build_film_authoring_context`（254 号）上下文 + 双语节点系统提示词逐字 + prompt-pack 附加段（`interactive-film.script`，107 号出口同款）+ `extract_json_object` 复用（agent_production 改 pub(crate)）+ 宿主强制 node id + phase→workshop；details 带 `promptPacks`/`skillIds`（TS graphUpdatedDetails 形态）。LLM 走 `FilmAuthoringLLM` trait（生产实现 = AgentRouter "film-authoring" 角色，temperature 0.6/maxTokens 4000 对齐 TS submitNode）。
- **generate_node_image**：254 号遗留的依赖面评估结论——**可移植**（74 号已接通 cover 生图基建）。prompt 解析（imageSlot.prompt→sceneDesc）、尺寸链（size ?? env ?? **1536x1024**）、落盘 `node_image_rel_path`、set-imageRef delta 全链复刻。
- **context-transform**：`film_graph_context_message`（TS createInteractiveFilmContextTransform 注入文案逐字），图谱缺失不注入（TS null 分支）。
- 确认类三件（draft_structure/connect_choice/remove_node）不移植聊天面——Rust 走 agent_production 确认链（chat_prompts 同一备案），与 TS confirmedIntent 单工具面等价。

### 接线 `agent_route.rs` / `chat_prompts.rs`

- 注册矩阵：authoring 会话 = 七件作者工具 + propose_action + use_skill（free-text 时）；**不注册** material/文件/research/import/sub_agent/编辑族（TS createFilmAuthoringTools 先返回语义）。propose_registered 门控纳入 authoring。
- `build_interactive_film_authoring_prompt`（双语逐字，TS agent-system-prompt.ts）+ build_system_prompt 分派臂（无 bookId 兜底 chat——TS 同落空分支）。
- bookId 缺失 → 400 BOOK_ID_REQUIRED（TS createFilmAuthoringTools 抛错对齐）。
- run_agent_loop 前 history 前插图谱上下文消息（TS `[injected, ...messages]` 对应位）。

### 顺手修复的两处双端偏差

1. **BOOK_NOT_FOUND 误拒 authoring 会话**（冒烟抓真 bug）：TS 对 `interactive-film-authoring` 显式跳过 books/{id}/book.json 存在性校验（studio/api/server.ts L4962——bookId 是影游项目 id），Rust 无条件校验导致该会话 404。已按 TS 作用域修复。
2. **生图默认尺寸**：上游 d1d6d8ec（2026-08-16）把 TS 默认从竖版 1024x1536 翻转为横版 1536x1024，Rust 74 号移植对的是旧值。post_node_image 已对齐并注明。
3. **applyGraphDelta phase/phaseRevs**：TS 的 phase 参数（world/structure/workshop 推进）与 phaseRevs 保留语义在 Rust 缺失（恒写旧 phase、重写时丢 phaseRevs）。`apply_graph_delta` 增 `phase: Option<&'static str>` + phaseRevs 透传；draft_structure 生产链补 phase=structure；POST delta/node-image 端点传 None。

## 二、验证

- engine lib **1336**（+7：直写工具 apply/rev/phase、memory facts、fill/revise 双语提示词与错误链、生图三错误路径、context 消息形态、schema 面）、集成 **197**、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**（41.8s，非早退）。
- **真实引擎冒烟**（mock LLM 全链，生产形态 bin + HTTP）：
  - authoring 无 bookId → 400 BOOK_ID_REQUIRED ✓
  - 带 bookId=p1 指令「把故事核心改成孤儿觉醒」→ LLM 第 1 轮 tool_calls(set_world_anchor) **真实执行落盘**（rev 1、storyCore=孤儿觉醒、phase=world）、第 2 轮收尾文本、HTTP 200 ✓
  - mock 请求体核验：消息序 [system(authoring 提示词+「p1」) → 注入图谱 user → 指令 user]、工具表 = 7 作者件 + propose_action + use_skill、无 material/文件/research/import ✓

## 三、film authoring 剩余

零——TS authoring 全部 12 工具面在双端各有归宿（聊天面 7 件 + propose/use_skill；确认链 3 件 + 提交工具 2 件在生产链）。后续可选增强：studio 端 authoring 会话入口 e2e（Playwright）与 generate_node_image 真实生图冒烟（需真实生图端点配置）。
