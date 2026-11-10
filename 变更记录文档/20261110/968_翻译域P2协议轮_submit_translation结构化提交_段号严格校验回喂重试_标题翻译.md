# 968 号 · 翻译域 P2 协议轮：submit_translation 结构化工具提交 + 段号严格校验回喂重试 + 章节标题翻译

> 966 号施工图 P2（G4/G5/G6）。此前 Rust 翻译为单轮 JSON 文本协议：坏段（未知/
> 重复/空 target）被**静默丢弃**、缺段不报错——部分漏译的运行仍回执「成功」；章节
> 标题从不翻译（译后章节仍为原文标题）。本轮按 TS 2.0 `createLLMTranslationModel`
> 收敛。
> 验证：Rust llm_model 单测 2/2（新）+e2e translations70 7/7（+2 新、mock 换工具
> 协议）+translation lib 套件全绿+testgate engine-rs 2099（+4）/src-tauri 584+
> audit:rust 双 0（自检活体）+clippy 双 0+gate:ts 十步全绿+U+FFFD 净。

## 一、协议（G4）

- `translate_segments` 改走 `run_structured_tool`（TS `runWorkerAgentTool` 等价环，
  888 号基座）：`submit_translation` 工具（TS `TranslationResultToolSchema` 同形
  JSON Schema：chapterTitle?/segments[{index≥0,target minLength1,notes?}]/glossary?）、
  `tool_choice:"required"`、`stream:false`、环预算 3 轮。
- system 逐字换 TS 2.0：「Translate the chapter title and all segments with the
  activated translation Skill. / Submit the complete translation through the
  translation result tool.」+基座追加 finish-by-calling 句；user 为同键 JSON。
- **技能注入保持**：structured 环经 `router_chat_turn_fn` 直呼 streaming_client，**旁路**
  `AgentRouter::chat` 的 OPERATION_SKILLS 出口——构造消息时显式读取当前激活集
  `append_activated_skill_guidance`（TS prepareWorkerMessages 同位），960 号技能块不丢
  （活体日志 translate 调用 skillBlock:true 实证）。
- `LlmTranslationModel` 持 `Arc<AgentRouter>`（环需所有权）；审校仍为单轮 JSON（P3）。

## 二、严格校验 + 回喂重试（G6）

- validate 闭包=schema 面（TypeBox 等价，码 `WORKER_SCHEMA_INVALID`）+ TS
  `validateTranslatedSegments` 语义面逐字：`Translation returned an empty target for
  segment N.`（码 `TRANSLATION_TARGET_EMPTY`）/`…unknown segment index N.`/
  `…duplicate segment index N.`/`Translation omitted segment index(es): a, b.`；按源段序
  归一、target/notes trim。
- 失败经工具结果**回喂模型重试**（TS 语义）；预算耗尽 → 末次校验错误为 run 失败消息
  （500 `INKOS_TRANSLATION_ACTION_FAILED`），坏批不落盘。runner 侧未知段由 `continue`
  改硬失败（TS 同）。

## 三、标题翻译（G5）+ 两处 TS 对齐

- `TranslateSegmentsOutput.chapter_title`；runner 跟踪 translatedTitle（首个非空结果采用），
  译文文件 title 与 manifest 章节 title=译后标题，审校输入/报告标题同用译后标题（TS
  updateChapterProgress/`chapterTitle: completedChapter.title`）。
- 未审校章 reviewSummary/observations 清空（TS 以 undefined 覆写同语义，967 版为保留）。
- 回执 observations=manifest 全章 flatMap（TS 口径，967 版仅本次运行）+category/
  assessment/scope（`chapter:N`）缺省补齐；manifest/详情为落盘原形不带 scope。

## 四、测试与必红

- 单测：语义五形态（合法归一/未知/重复/缺失/空 target 码面）+schema 八形态。
- e2e：mock 换工具协议（非流式 tool_calls 应答，审校仍 SSE JSON）；新例「首轮漏译末段→
  回喂 `Translation omitted segment index(es): …`→次轮补全成功，tool_choice=required、
  system 含 finish-by-calling、标题 `[译]Chapter 1` 落 manifest 与译文文件」；新例「持续
  漏译→预算耗尽 500+末次校验消息+计数 0 不落盘」；960 例断言改新 system；967 例回执
  observations 补 scope 期望。
- **必红（env 中和两维）**：NEUTER968_VALIDATE（去缺段校验）→ 恰 2 红（重试例/耗尽例）；
  NEUTER968_TITLE（去标题采用）→ 恰 1 红（标题例）；撤除后 7/7，残留 0。

## 五、真引擎 GUI 活体（8968，工具协议 mock v2：首轮故意漏译末段）

- 两章各 2 段项目 → IAB 翻译页「运行翻译」：回执「翻译 4 段，审校 2 章。Skill：
  inkos-translation」，章节卡「2 segments」×3、章节标题「【译】Chapter 1/2」。
- mock 日志逐章：translate(stream:false, toolChoice:required, skillBlock:true, 提交 1 段)
  → translate(feedback=["Translation omitted segment index(es): 2."], 提交 2 段) →
  review(skillBlock:true)——回喂重试与技能注入在生产形态全链实证。
- 环境清偿：引擎/mock 杀净、临时根删除、IAB 标签关闭、stash 栈 0。

## 六、门禁

- clippy 双 0（首跑 nonminimal_bool 2 处=`is_none_or` 化后清零）；testgate engine-rs 2099
  （+4）/src-tauri 584（INKOS_DUEL=1）+audit:rust 双 0+必红自检活体；gate:ts 十步全绿；
  U+FFFD 净。

## 七、备案

- 温度：TS runWorkerAgentTool 走客户端缺省，Rust 沿用翻译面既有 0.2（忠实度优先）。
- TS prepareWorkerMessages 的 authorRequest 前置/上下文编译分支翻译面不移植。

**下一号自 969 起；出口=翻译 P3（审校 2.0：独立源读+成对引证 observations+
reviewSourcePaths）/ P4 存储迁移 / P5 修订；用户侧待办=Fork 推送（含 955-968）。**
