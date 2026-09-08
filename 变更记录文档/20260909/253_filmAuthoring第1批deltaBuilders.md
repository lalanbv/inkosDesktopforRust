# 253 号：film authoring 移植第 1 批（前半）——六类 delta builder 纯函数补齐

- 日期：2026-09-09
- 分支：develop
- 关联：232 号（评估拆 3 批——本批启动第 1 批）、128/129 号（interactive_film 既有移植：StoryGraphDelta/apply_graph_delta 已在位）
- 推送核验：origin/develop 仍停 6bc65564——**201–251 共 52 提交待推送**；本批随 252 后本地领先 55。两项默认值无新答复。

## 一、评估修正（232 号范围收窄）

深查发现 232 号评估高估了缺口：`StoryGraphDelta` 结构、`apply_upsert_remove` 归约、`apply_graph_delta` 端点、authoring-state 存储层**均已在位**。第 1 批实际缺口仅 **6 个 delta builder 纯函数**（TS authoring-tools.ts 仅 26 行）——本批补齐：
- build_world_anchor_delta（partial WorldAnchor）
- build_add_variable_delta / build_define_ending_delta（单元素 upsert）
- build_remove_node_delta（remove 语义）
- build_connect_choice_delta / build_upsert_characters_delta（upsert 语义）

单测：单段 delta 断言（每 builder 只填自己的段、其余 None、notes 空）。

## 二、验证

- engine：lib **1327**（+1 delta builders）、集成 **197**、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**。

## 三、film authoring 剩余路线（第 1 批后半 + 第 2 批）

1. 第 1 批后半：fill_node/revise_node/generate_node_image 三个 LLM 工具 schema + 执行器（依赖 authoring-state 图谱结构，已移植）；
2. 第 2 批：聊天循环注册（ChatToolRouter + 注册矩阵 film-authoring 分支）+ context-transform（每轮注入剧情图谱）+ 双语提示词（231 号备案段）。
