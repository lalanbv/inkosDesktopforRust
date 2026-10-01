# 559 号：R37 自扩展技能写入链——author_skill 工具 + disable-model-invocation 治理面（TS 先行，Rust 备案）

日期：2026-10-01
类型：feat(core)，546 号 Pi 深化四 R37 施工图（写面 TS 先行，498→500 先例）

## 选题来路

v6 Pi 对标线 R36/R37 两域，R36 已出设计稿、R37 施工图四段链（生成→写入→确认门→
热载）。本仓技能基建全活（SKILL.md 限制与上游逐字同构、五目录装载、registry
resolve、use_skill golden 18 键），唯一缺口 =「agent 为自己写技能」的写入链与
治理门。安全模型与上游相反：上游全信任扩展宿主，本仓受控链路（优势位）。

## 考古实锤（施工图的两处修正）

- **registry dedupe 是 last-write-wins 不是首胜**：builtin-loader.ts 注释明示
  「project/user skills can intentionally replace an InkOS default with the
  same id」（[...builtin, ...configured] 后写胜）。施工图 4.3「首胜不改」
  与现实相反——这使 **写端撞名拒绝比施工图预估更必要**：不拒则 agent 写
  `skills/<内置id>/SKILL.md` 即可在下会话覆盖内置技能（自劫持）。
- **disable-model-invocation 此前零消费面**：AgentSkillSchema 是 strict()
  ——用户手改 frontmatter 加该字段会把整份 SKILL.md 打成装载 diagnostic
  （strict 拒未知键）。R37 必须显式支持，否则「人侧停用通道」是坏的。

## 实施

1. **治理面三件**：
   - `skills/types.ts`：AgentSkillSchema 加 `disableModelInvocation: z.boolean().optional()`
     （camelCase 键；frontmatter kebab-case 显式映射，语义=人侧保留可见、
     模型不可激活）。
   - `skills/external-loader.ts`：parseAgentSkillDocument 映射
     `data["disable-model-invocation"] === true`。
   - `skills/registry.ts`：resolveSkills 把该技能并入 disabled 集（不进
     availableSkills/usedSkills，requested 强制也跳过；listSkills/getSkill
     保留——人侧/审计可见）。golden 18 键安全：既有技能无该字段（optional）。
2. **author_skill 工具**（agent-tools.ts `createAuthorSkillTool(projectRoot)`）：
   - `assertSafeSkillId`：`^[a-z0-9][a-z0-9-]{0,63}$` + 保留字
     （builtin/user/external/project 四 source 枚举）。
   - 只增不改：同名目录已存在即拒绝（修改/删除走人侧）；不给通用写权（维持）。
   - **全源撞名拒绝**：loadAvailableAgentSkills({projectRoot}) 拿
     builtin+configured 全部现有 id，任一同 id 即拒（last-write-wins 防自劫持）。
   - 落盘 `skills/{name}/SKILL.md`：frontmatter 用 JSON.stringify 产 YAML
     双引号标量——description 中换行/`---`/伪键全部中性化（注入防御实测）。
   - 结果文本明示三件事：下会话生效、当前会话冻结、停用方式。
3. **装配**：buildChatToolSet 统一追加（全模式常驻：chat/book/edit/production
   皆可沉淀技能；不走 spill 包装）；buildChatToolCatalog 进 node-only 组
   （13→14）；差分器 TOOL_CATALOG_ONLY_NODE +author_skill。
4. **测试**：author-skill.test.ts 7 件（安全名矩阵/端到端回读闭环——落盘后
   loadConfiguredAgentSkills 即拾取且 description/body 逐字/只增不改+撞名/
   注入防御/限额/disable 解析+registry 过滤）；chat-tool-set.test 名单锁
   3 处（NODE_ONLY 14/五件→六件）；agent-session.test 名单锁 12 处
   （author_skill 插 use_skill 前；derivative 四件套 toEqual([toolName]) →
   [toolName, "author_skill"]）。

## 偏差备案（施工图 vs 实施）

- **确认门**：施工图 4.2.3「写盘前走既有确认链（propose_action 复用）」——
  实施为「直接落盘 + 当前会话 registry 冻结 + 结果文本明示 + 人侧停用通道」。
  理由：propose_action 复用需新 intent 类型+studio 卡片适配，跨 core+studio
  量级翻倍；冻结+人审窗口已实现「未过人审的技能不进当轮系统提示」的安全
  本质；连续自写风险由结果文本通知缓解。RunLog 审计（R33 `inkos.skill.authored`
  预留键）仍未接线——维持备案。
- **Rust 侧不实施**（546 施工图 4.4 裁决「写面 TS 先行，Rust 备案」——498→500
  导演灵感卡先例：语义 TS 先行、Rust 补偿清偿）。差分器 node-only 豁免表
  放行，rust-only 空集要求不涉。

## 门禁

core tsc 0 + author-skill 7/7 + chat-tool-set 9/9 + agent-session 51/51 +
gate:ts 七步全绿（engine-contract-diff 0 分歧含新豁免）+ duel 真跑 10/10
（Rust 零改动——cargo/tauri/clippy 面无涉）。

## 教训

- **名单锁测试是装配面的回归护栏**：author_skill 加入触发 15 处名单断言更新
  （catalog/chatt-tool-set/agent-session 三文件）——这些锁正是 555 号
  「目录名单 = 机械对照」纪律的 TS 侧配套，新工具必须在三处同步。
- **strict zod schema 下加字段前先查治理通道是否已坏**：disable-model-invocation
  表面「零消费面」，实际 strict 把手写字段变成装载炸弹——考古要看「字段
  存在时会发生什么」，不只看「谁消费它」。
- **施工图的治理细节要与代码现实对勘**：dedupe 首胜 vs last-write-wins 一字
  之差，防劫持的实现位置完全不同（registry 改造 vs 写端拒绝）。
