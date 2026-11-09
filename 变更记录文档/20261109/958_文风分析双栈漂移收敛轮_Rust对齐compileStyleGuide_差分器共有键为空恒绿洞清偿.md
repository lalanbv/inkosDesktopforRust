# 958 号 · 文风分析双栈漂移收敛轮：Rust /style/analyze 对齐 2.0 compileStyleGuide + 差分器共有键为空恒绿洞清偿

> 957 B 面备案勘察（写法档案池的写入来源=文风页）时顺藤摸出的真实用户面缺陷：
> 默认 Rust 后端下「文风」页点「分析」，结果区**恒空**——Rust `POST
> /style/analyze` 仍返回 55 号移植时的统计指纹 StyleProfile，而上游 2.0 harness
> 迁移（7aed0d82）已把该端点换成 LLM 编译文风指南 `{guide}`、文风页同提交改读
> `guide`。本轮把 Rust 端按 TS 收敛，并清偿差分器「超集键剥离」恒绿洞（新探针
> 必红自检时当场抓获）。
> 验证：Rust style_guide 单测 3/3（新）+e2e style55 7/7（+3 替 1）+273 大载荷
> 用例改形绿+testgate engine-rs 2087（+5）/src-tauri 584+audit:rust 双 0（自检
> 活体）+clippy 双 0+gate:ts 十步全绿（engine-contract-diff 共享面 76 对比 0
> 分歧）+U+FFFD 净。

## 一、缺陷定型（双栈收敛方向判定法三步）

- **移植时形态**：8263e192（55 号）Rust `style_analyze` = TS 当时的
  `analyzeStyle(text, sourceName ?? "unknown")` 纯统计 → 直接返回 StyleProfile。
- **上游后改**：7aed0d82「complete creative harness v2 migration」把 TS 端点换成
  `compileStyleGuide`（必需技能 inkos-long-story-analysis + inkos-imitation-writing
  经 runWorkerAgent 编译）→ `{ guide }`；StyleManager.tsx 同提交改渲染
  `profile.guide`。
- **判定**：忠实移植+上游后改=漂移 rust→TS，按 TS 收敛（非刻意增强）。
- **活体取证（before，8958 真引擎+IAB GUI）**：curl 返回九键 StyleProfile 无
  `guide`；文风页点「分析」后 `<article>` 文本为空串、高 32px（仅内边距），导入区
  照常出现——用户看到空白分析结果。
- **为何一直没被拦**：TS 侧该端点要 LLM，差分器共享面此前未收录；TS 交互测试
  （922 号）mock `{guide}`，只验 UI 不验 Rust 端点形态。

## 二、Rust 收敛实现（TS 逐字）

- 新模块 `engine-rs/src/agents/style_guide.rs`（对位 TS `agents/style-guide.ts`）：
  - `style_guide_messages`：system 双语逐字（「把参考文本编译为有证据、可执行的
    文风指南，只返回 Markdown。」/ "Compile the reference into an evidence-backed
    operational style guide. Return Markdown only."）；user = `来源：{源}\n\n{样本}`
    / `Source: {源}\n\n{样本}`，源 = sourceName trim 非空，否则「参考文本」/
    "reference"。
  - `resolve_style_guide_skills`：按声明序逐个取可用集**首个**同 id 技能（TS
    `available.skills.find`——builtin 在前），缺任一 →
    `Style extraction requires unavailable skill: {id}`（LLM 不被触达）。
  - `compile_style_guide`：样本 trim 空 → `Reference text is required for style
    extraction.`；激活集 = 外层已激活 ∪ 两技能（merge 后写胜）经
    `scope_operation_skills` 让 AgentRouter::chat 出口注入 system（139 号既有
    注入点，与 TS appendActivatedSkillGuidance 逐字同形）；响应 trim 为空 →
    `Style extraction returned an empty guide.`。
- `style_analyze` 接线：取 `State<BooksRuntime>`（路由补 `.with_state`）；语言 =
  请求 `language`（"en" → en，其余任意值 → zh——TS 只判 `=== "en"`）缺省/null 回
  项目配置；来源缺省 "unknown"（TS `sourceName ?? "unknown"` 先于 trim 兜底）；
  成功 200 `{guide}`，失败 500 平铺 `{error}`（同文件既有错误协议）。
- **备案**：①TS `hydrateActivatedSkillGuidance` 引用资源装载（linked/检索）Rust
  路由出口未移植——139 号既有备案（resources 恒空、全 body 注入），本轮不扩面；
  ②TS `runWorkerAgent` 不传 temperature，Rust 路由面必填——沿用同名 agent
  "style-guide" 既有 0.3（分析型抽取）；③TS 加载项目配置失败即 500，Rust 回 zh
  继续（effective_router 回退面），更宽容的边缘差异；④`/books/:id/style/import`
  Rust 仍为 2.0 前形态（统计指纹+定性拆解+方法论 → books/{id}/story/style_guide.md，
  Rust 写作链自洽读同处；TS 2.0 写 works/{id}/source/story 经 source sync），
  非用户面断裂，留后续 2.0 能力执行面收敛。

## 三、差分器：探针 + 恒绿洞清偿

- **探针**：walkthrough-mock 分派链首新增 compileStyleGuide 回显分支（system
  首段+`### inkos-*` 技能标题行+user 全文；置链首防技能正文含下游分支识别词误
  分派）；共享面加 `POST /style/analyze` 探针并新增 `fullBody` 标记=非 GET 探针
  **严格**全 body 对比（不做超集键剥离）。双端回显逐字相等=锁死提示词/技能注入
  序/响应形态同形。
- **恒绿洞（必红自检当场抓获）**：首版探针在旧 Rust 二进制上仍判 ✓——698 号
  超集键剥离对「共有键为空」的两侧（TS `{guide}` vs Rust 九键）各自剥成 `{}`
  后恒等。清偿：剥离逻辑抽 `stripDeclaredSupersetKeys`，**共有键为空=形状全
  分叉判分歧**（超集关系以共有核为前提；846 号「解析型门禁正向确认」同族）；
  启动自检三夹具（全分叉/一侧空/真超集），自检失败 exit 2 拒绝运行。现网共享面
  超集声明仅 books/doctor 两条且均有共有核——零误伤。
- **必红三证**：①旧 Rust 二进制+严格探针 → `✗ /api/v1/style/analyze 首个分叉
  /guide`（76 对比 1 分歧）；②新二进制 → 76 对比 0 分歧；③自检注入（
  shapeDisjoint 恒 false 的临时副本）→ 「超集键剥离自检失败」exit 2——自检非恒绿。

## 四、测试与必红

- Rust 单测 +3（style_guide.rs）：消息逐字+来源兜底三形/技能声明序+首个命中
  不被后列同名覆盖/缺失技能报首个缺失 id。
- e2e style55 +3 替 1（`style_analyze_returns_profile_and_validates` 退役）：
  捕获式 mock 断言 `{guide}` trim、system 前缀逐字+两技能标题按序、user 逐字；
  语言/来源解析四形（请求 en/缺省 unknown/配置 en+空白源 reference/请求 zh 压过
  配置）；错误面（text 空/缺 400 先于一切、缺技能 500 且 mock 零请求、空白回复
  500 空指南）。真内置 SKILL.md 拷入项目约定 skills 目录（测试进程 builtin 根
  缺失；configured 层同 id 同正文）。
- 273 大载荷用例改形：analyze 改前导空白垫片 >2MB + `{"text":"  "}` → 400（证
  body 完整读取，不依赖 LLM——与 import/chapters、fanfic/init 同形）。
- **必红**：stash 三实现文件（保留测试）→ style55 恰 3 红（新 3 例：`{guide}`
  形态断言红/LLM 未被调用/缺技能应 500 得 200——均为行为红），既有 4 例绿；
  pop 后 7/7。

## 五、真引擎活体 + GUI 真人走查（8958，二进制 23:30 先核验新鲜）

- curl：`{guide}` 含两技能标题行（真内置技能目录 env 注入）+「来源：斗破」。
- IAB 文风页：填来源/样本→点分析→结果区渲染指南全文（article 334px）；选书→
  「导入文风指南」→ 绿色成功回执，盘面 books/b1/story/style_guide.md+
  style_profile.json 落盘；停 mock 再点分析→旧结果清空+红色错误提示「HTTP 错误:
  …」（非空白结果）。
- 走查插曲：Playwright 定位器 click 在该环境超时（可操作性检查不过），按 IAB
  惯用形改 DOM 级 `el.click()`；React 受控输入经 `fill` 后 state 已同步（按钮
  enabled 核验）。
- 环境清偿：引擎/mock 杀净、临时根删除、IAB 标签关闭。

## 六、门禁

- clippy 双 0；testgate engine-rs 2087（+5）/src-tauri 584（含 INKOS_DUEL=1）+
  audit:rust 双 0+必红自检活体；gate:ts 十步全绿（build/typecheck/test/audit:npm/
  rust-bin/node-fallback-smoke/engine-contract-diff 76/0/export-epub-smoke/
  work-alternation/check-routes）；U+FFFD 净。

## 七、结论

默认 Rust 后端下文风页「分析」从恒空恢复为 2.0 语义（LLM 编译、技能注入、错误
可见），双端提示词与技能注入序由差分器回显探针逐字锁死；差分器顺带清偿「共有键
为空」恒绿洞并以启动自检机制化。957 B 面（写法绑定读树/匹配键/档案池无 UI 写入
口）仍待 959 定案。

**下一号自 959 起；出口清空回归常备（B 面=写法绑定特征树备案；style/import 2.0
能力执行面收敛备案）；用户侧待办=Fork 推送（含 955-958）。**
