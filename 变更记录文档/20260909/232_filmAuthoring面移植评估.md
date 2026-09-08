# 232 号：interactive-film-authoring 面移植评估——评估报告（不实施）

- 日期：2026-09-09
- 分支：develop
- 关联：231 号（本批为其遗留项的评估结论）、128/129 号（film 相关既有移植）
- 推送核验：origin/develop 仍停 6bc65564——**201–231 共 31 提交待推送**；本批次后本地领先 32。两项默认值无新答复。

## 一、现状盘点

| 面 | TS | Rust |
|---|---|---|
| authoring 工具族（12 件） | film-authoring-tools.ts（513 行）：set_world_anchor / add_variable / define_ending / upsert_characters / submit_story_node / submit_story_structure / fill_node / revise_node / draft_structure / connect_choice / remove_node / generate_node_image | **零移植**（engine-rs 全仓无命中） |
| context-transform | createInteractiveFilmContextTransform——每轮从磁盘注入完整剧情图谱（节点 id/选项/变量/条件/效果/结局的唯一权威） | 无 |
| authoring-state 存储 | authoring-store.ts | ✓ 已移植（interactive_film_routes：authoring-state.json + per-project 串行锁 + pre-rev 快照） |
| propose_action 映射 | draft_structure/connect_choice/remove_node → interactive-film-authoring 会话 | ✓ 基建在位（propose_action_tool 映射） |
| SessionKind | interactive-film-authoring | ✓ 枚举在位 |
| 聊天循环注册矩阵 | 12 工具 + propose_action | **无 authoring 工具注册——该会话兜底 chat 提示词，LLM 无法操作剧情树** |

## 二、影响面

互动影游**创建**链完整可用（interactive_film_create 确认链走 agent_production ✓）；创建后的 **authoring 逐节点编辑**（对话式改剧情树）在默认 Rust 引擎下不可用——用户切 Node 回退端可恢复完整功能。属「高级功能入口缺失」，非数据/正确性缺陷。

## 三、工作量评估（如移植）

- 12 工具 schema + 执行器：每个工具操作 authoring-state 的剧情图谱 JSON（节点/变量/结局增改查），估 800-1200 行 + 测试。
- context-transform：每轮注入图谱（token 预算管理），估 150 行。
- 系统提示词双语（231 号备案段）+ 注册矩阵接入：估 100 行。
- propose_action 三个结构动作的确认执行链对接：估 200 行。
- **合计估 1500+ 行**——大型移植项目，非单批「轻量」体量。

## 四、建议

1. **优先级判定待产品输入**：互动影游 authoring 是否为核心使用路径？若 film 创建（已可用）满足主要需求，authoring 移植可延后。
2. 若决定移植，建议拆 3 批：①存储层图谱操作纯函数（含 golden 向量）；②工具族 schema+执行器+注册；③context-transform + 提示词 + 确认链对接。
3. 短期缓解：authoring 会话在 Rust 引擎下的兜底 chat 提示词可引导用户切 Node 回退端（如需）。

## 五、验证

纯评估批次（零代码改动）：全部门禁于 231 号已绿。
