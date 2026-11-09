# 960 号 · 翻译技能激活对齐轮：Rust translation run 注入 inkos-translation 技能 + skillIds 回执

> 959 族扫备案出口①：TS 2.0 translation run 以 profile "translation" 的必需技能
> inkos-translation 包裹翻译/审校链（runWithAgentContext 注入）并在回执附
> `skillIds`；Rust 端翻译链**零技能指导**且回执缺 `skillIds`——默认 Rust 后端
> TranslationManager 回执无「Skill：…」，更实质的是翻译与审校提示里缺少长文翻译
> 方法论（忠实度/术语一致/审校证据要求）。本轮按 TS 收敛这一切片。
> 验证：Rust production_bindings 单测 +1（10/10）+e2e translations70 4/4（+1
> 新、2 例补技能播种与 skillIds 断言）+testgate engine-rs 2090（+2）/src-tauri
> 584+audit:rust 双 0（自检活体）+clippy 双 0+gate:ts 十步全绿+U+FFFD 净。

## 一、漂移定型

- **TS 2.0**（`server.ts` translations/:id/run）：`resolveProfileSkillActivations(
  loadAvailableAgentSkills(root).skills, createBuiltInWorkProfileRegistry(root).
  require("translation"))` → `createTranslationRunTool(…{defaultSkills})` →
  `pipeline.runWithAgentContext({activatedSkills})` 包住翻译模型与审校模型 →
  回执 `{…result, skillIds}`；缺必需技能 → 抛 `Profile "translation" requires
  unavailable skill(s): inkos-translation` → 500。
- **Rust**：`run_translation` 直接跑 `run_translation_project`，无技能解析/注入，
  回执 `{projectId,translatedSegments,reviewedChapters,reportPath}`。
- **定性**：Rust 翻译域整体为 2.0 前移植（`translations/` 存储、JSON 协议提示词、
  无标题翻译/无修订面——886 号起「translation 不在 M3」既有收窄），全量 2.0
  迁移属独立大工程；本轮只收敛**技能激活+回执**这一用户可见且质量相关的切片
  （技能指导块声明「不是输出格式覆盖」，与 Rust JSON 协议提示词兼容）。

## 二、实现

- `skills/production_bindings.rs` 新增 `resolve_profile_skill_activations`（TS
  `skills/activations.ts` 逐字）：可用集按 id 建表**后写胜**（TS `new Map`——
  configured 同名覆盖 builtin）；缺必需即错（消息逐字，多缺以 `, ` 连接）；
  `include_recommended` 追加推荐集；去重保序；resources 恒空。
- `translation_routes.rs::run_translation`：`translation_skill_activations(root)`
  （load_available_agent_skills + create_built_in_work_profile_registry(root).
  require("translation") + 上述解析，失败 → 500 TRANSLATION_RUN_FAILED 带原消息）
  → `scope_operation_skills(Some(activations), run_translation_project(..))`
  （runner 无 spawn，task-local 覆盖翻译与审校每次 AgentRouter::chat 出口注入）
  → 回执插入 `skillIds`。
- **备案**：①错误码沿用 Rust 既有 `TRANSLATION_RUN_FAILED`（TS 2.0 为
  `INKOS_TRANSLATION_ACTION_FAILED`；UI 只显 message）；②桌面壳 489 号保证打包/
  dev 均注入 builtin 技能根，缺技能即部署缺陷——与 TS 同 fail-loud；③全量翻译域
  2.0 迁移（works 存储/工具提交协议/标题翻译/修订面/审校 observations）留独立
  规划。

## 三、测试与必红

- 单测 `profile_skill_activations_mirror_ts`：缺必需消息逐字/后写胜/推荐集按需
  追加/推荐缺失不报错。
- e2e：mock 抽 `translation_mock_reply`+捕获式变体（记录每次 system）；
  `seed_translation_skill`（真内置 SKILL.md 入项目约定 skills 目录）；全链用例补
  `skillIds == ["inkos-translation"]`；502 用例补播种（保持上游失败语义）；新例
  `run_injects_translation_skill_and_requires_it`：缺技能 → 500 消息逐字且 mock
  零请求；播种后 200+skillIds，且翻译与审校两类调用的 system 全部含
  `## Activated professional skills`+`### inkos-translation — …`+技能正文。
- **必红**：stash `translation_routes.rs`（保留测试与解析函数）→ 恰 2 红（全链
  skillIds 缺失/缺技能应 500 得 200），pop 后 4/4。

## 四、真引擎活体 + GUI 真人走查（8961，二进制先核验）

- 翻译专用 mock（JSON 协议应答+逐次记录 system 是否带技能块）；项目经 API
  upload(dataUrl)+create 建立（IAB 不支持文件选择器）。
- IAB 翻译页：选项目→「运行翻译」→ 回执「翻译 2 段，审校 1 章。Skill：
  inkos-translation。报告：translations/…/review-report.md」，段落回填「【译】…」；
  mock 日志 translate/review 两次调用 `skillBlock: true`。
- 环境清偿：引擎/mock 杀净、临时根删除、IAB 标签关闭、stash 栈 0。

## 五、门禁

- clippy 双 0；testgate engine-rs 2090（+2）/src-tauri 584（INKOS_DUEL=1）+
  audit:rust 双 0+必红自检活体；gate:ts 十步全绿；U+FFFD 净。

## 六、结论

默认 Rust 后端翻译链获得与 TS 2.0 相同的长文翻译方法论指导（翻译+审校全调用
注入），回执恢复「Skill：…」；profile 必需技能解析函数作为通用件入 Rust 技能
绑定层，可供后续 2.0 能力执行面复用。

**下一号自 961 起；出口=961 审计响应形态上游不一致定案 / 翻译域 2.0 全量迁移
规划 / 写法绑定 B 面；用户侧待办=Fork 推送（含 955-960）。**
