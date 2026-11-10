# 973 号 · 代理修订工具注册轮：revise_paragraph 入 agent 工具注册表 + debug/tools 目录 +1

> 971 号备案：修订工具未入 agent 工具注册表（TS 为代理域工具
> `createTranslationRevisionTool`，Rust HTTP 面先行）。本轮补注册面：新
> `interaction/translation_tools.rs`（revise_paragraph ToolDef，执行体复用 971 号
> `revise_translation_segment`）+ `ToolCtx.translation_deps`（runtime+活动书）+
> agent_route 装配 + debug/tools 目录计数 32→33。
> 验证：Rust lib translation_tools 3/3（新）+registry 11/11（计数/唯一名/生产变更集
> 三例随迁）+debug/tools 33 断言+testgate engine-rs 2108（+3）/src-tauri 584+audit:rust
> 双 0（自检活体）+clippy 双 0+gate:ts 十步全绿+U+FFFD 净。

## 一、注册面

- `TranslationToolDeps{runtime, active_book_id}`（agent_route book 会话装配——与
  import_registered 同面：chat|book）；`ToolCtx.translation_deps` 字段（root_only None）。
- `revise_paragraph` ToolDef（tool_def! 宏）：ProductionMutation（suppress_production
  集中拒绝面自动覆盖——后台生产剔除）；available=translation_deps 在场；schema=
  TS TranslationRevisionParams 逐字（workId?/chapterNumber≥1/paragraph anyOf
  [const last, integer≥1]/instruction minLength 1/additionalProperties:false）；描述逐字。
- 执行体：`resolve_work_id_from`（显式 workId 优先、活动书兜底）→ 参数守卫
  （chapterNumber/paragraph/instruction，error_result 透传 TS 码面）→ 971 号执行链
  （技能装配=HTTP revise 同款）→ ToolResult text=TS textResult 逐字
  （`{Revised|Kept} chapter N, paragraph P of C (last paragraph|not the last paragraph).
  Selected source: …`），details=result+`kind:translation_segment_revised`+`workId`。
- 注册表 defs 尾部 extend（声明序=调试投影序）；registry.rs use 表加 translation_tools。

## 二、测试与必红

- lib 新 3 例：注册面（名称/ProductionMutation/schema required+anyOf[const last,
  integer≥1]/additionalProperties:false）；parse_paragraph 六形态（last→None/3→Some/
  None→None/0·-1·字符串数字即错）；textResult 文本四形逐字。
- registry 三例随迁：总件数 35→36（分解注释同步）、debug/tools 32→33、生产变更集九件
  →十件（+revise_paragraph 按字典序插入）。
- **必红**：NEUTER973 中和注册表 extend → 恰 3 红（registry 计数/生产变更集/注册面
  presence），撤除后全绿，残留 0。

## 三、真引擎活体（8973，二进制重建后核验）

- `GET /api/v1/debug/tools`：33 条全唯一、`revise_paragraph` 在册、pipeline=true、
  条目键面 name/description/parametersSha256/pipeline。
- 环境清偿：引擎杀净、临时根删除、stash 栈 0。

## 四、门禁与插曲

- clippy 双 0（首跑 len>=1 比较与 unused import/braces 三处清零）；testgate engine-rs
  2108（+3）/src-tauri 584（INKOS_DUEL=1）+audit:rust 双 0+必红自检活体；gate:ts 十步
  全绿（diff 78/0）；U+FFFD 净。
- 插曲：NEUTER973 注释拼接形态错位（`// NEUTER973：` 直接前缀原行）致恢复脚本首次
  miss——grep 实际形态后精确恢复。

## 五、备案

- 工具参数 schema 面与 TS TypeBox 形态逐键同形（anyOf const-union）；执行面 Work 锁
  语义备案同 971（HTTP revise 同执行体）。
- 代理面 TS `committedAction`（确认卡粒度）在 Rust 聊天分发面由 ProductionMutation+
  suppress_production 承担（既有机制，无新门）。

**下一号自 974 起；出口=epub 生成器字节对齐 / C 类残余备案项 / 翻译 create 的
registered-sources 进代理确认卡；用户侧待办=Fork 推送（含 955-973）。**
