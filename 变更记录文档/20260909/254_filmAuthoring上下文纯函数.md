# 254 号：film authoring 移植第 1 批后半——authoring 上下文纯函数（film-context.ts 逐字）

- 日期：2026-09-09
- 分支：develop
- 关联：253 号（delta builders——第 1 批前半）、TS film-context.ts（32 行）
- 推送核验：origin/develop 仍停 6bc65564——**201–253 共 54 提交待推送**；本批随 254 后本地领先 56。两项默认值无新答复。

## 一、移植内容（interactive_film.rs）

- `summarize_story_graph`（TS summarizeStoryGraph 逐字）：标题/世界锚（核心/主题/题材/规则/时长）/变量清单/节点清单（`- id[type] title -> 选项→目标`）；
- `build_film_authoring_context`（TS buildFilmAuthoringContext 逐字）：摘要 + 角色档案块（`- 名（role）动机：… 口吻：…`，口吻由 speakingRhythm/vocabulary 合成）；
- role 序列化名经 serde 取（protagonist/antagonist/support/other）。

## 二、验证

- engine：lib **1329**（+2：图谱摘要行形态断言〔含 edges/target 渲染〕、角色档案块与 blocks 顺序断言）、集成 **197**、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**。

## 三、film authoring 剩余（第 2 批）

fill_node/revise_node 工具层（NodeSubmitter 端口 + 上下文调用 + apply delta）与聊天循环注册/context-transform/双语提示词。generate_node_image 依赖图像服务依赖面评估。
