# 240 号：use_skill 运行时工具移植——skills 从管理面接入 agent 运行时

- 日期：2026-09-09
- 分支：develop
- 关联：130 号（skills 域移植）、230/231 号（skill catalog 提示段——本批落地其运行时依赖）
- 推送核验：origin/develop 仍停 6bc65564——**201–239 共 39 提交待推送**；本批次后本地领先 40。两项默认值无新答复。

## 一、缺口

skills 域在 Rust 只有管理端点（GET /skills 等）与加载/解析基建（load_available_agent_skills / SkillRegistry 均已在位），但 **agent 运行时未接入**——TS 聊天循环的 `use_skill` 工具（intentSkillTool）与 system prompt 的 skill catalog 提示段在 Rust 缺失，用户安装的专业技能在默认引擎的对话中无法被激活。

## 二、移植内容

1. **`interaction/skill_tool.rs`**：use_skill schema（UseSkillParams 逐字）+ 执行器——skillId 规范化（normalize_skill_id_strict）、disabled 拒绝（`Skill is disabled`）、registry 查找（`Skill is not available`）、body 返回（空 body 回退 description）+ `Skill activated` 文本面 + `{kind:"skill_activated", skillId}` details；resourcePath 资源分支（safeChildPath + is_file + 512KB + UTF-8 NUL 校验）。
2. **catalog 提示段**：`serialize_skill_catalog`（id/name/description 三元 JSON，`<`/`>` → `\u003c`/`\u003e` 转义防提示注入，对齐 TS JSON.stringify 转义层级）；`<skill_catalog_data>` 包裹 + zh/en 使用纪律段逐字，追加到 system prompt。
3. **注册条件**：`action_source == free-text && requested_skills 为空`（TS allowIntentSkillSelection 同语义），对所有会话面统一追加（TS createAgentToolsForMode 同构）；`disabledSkills` 请求参数启用（此前被 `_` 吞弃）。

## 三、备案

1. **query 语义检索分支**：依赖 BM25 索引（LocalSearchIndex 未移植）——schema 保留、执行忽略（返回主体即可用）；待检索架构重构（233 号 memory 语义层同族）一并评估。
2. **onActivate 激活回调**（TS skillTurnActive → sanitize 链）不移植——Rust loop 为 OpenAI 文本形态，toolResult body 已承担指令注入。

## 四、验证

- engine：lib **1319**（+4：激活文本/details、disabled/missing/缺参、catalog 转义）、集成 **196**、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**。
- TS 零改动；core 1918 / studio 793 / 双 typecheck 已绿。
