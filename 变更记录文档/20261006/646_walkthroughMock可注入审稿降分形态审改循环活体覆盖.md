# 646 号：walkthrough-mock 可注入审稿降分形态——审改循环/降分重写分支活体覆盖（645 备案清偿）

日期：2026-10-06。类型：chore(scripts 走查基建)。前置：645 号备案「审稿 overall_score 恒 88（审改循环/降分重写分支未覆盖，可另号加注入形态）」。

## 一、审改循环语义勘察（core chapter-review-cycle）

通过判定 = `passed && score ≥ 85（PASS_SCORE_THRESHOLD）&& 篇幅带内`。初始审不通过 → 最多 `maxReviewIterations`（默认 1）轮：logStage「修复轮次 N/M（当前 X 分）」→ reviser.reviseChapter(mode="auto") → 复审 → 达标退出 / 净提升 ≥3 继续 / 否则退出。修稿输出契约（auto 模式）= `=== FIXED_ISSUES ===` / `=== REVISED_CONTENT ===` 节；reviser 判 `revisedContent` 与原文全等或为空即「未产出新内容，退出循环」。

## 二、mock 实现（单文件，未设 env 时与 645 形态逐字节等价）

1. **`WALKTHROUGH_MOCK_AUDIT_SCORES`**（如 `"62,91"`）：审稿调用按模块级计数器递进取分（越界取最后一个）；`<85` → `passed:false` + 一条 structural issue（驱动 auto 模式强制 REVISED_CONTENT 路由）；未设 → 恒 88（645 形态，循环不触发）。
2. **「修稿编辑」分派**：返回 `=== FIXED_ISSUES ===` + `=== REVISED_CONTENT ===`（修订正文=原正文+修订尾段，与原文有差异且字数带内 2487）。

## 三、红绿（活体）

- **红 1（注入机制不存在）**：645 green3 日志「修复轮次」计数 0——恒 88 形态下审改循环不可达，`审计草稿` 直落 `落盘最终章节`。
- **红 2（注入后首跑，分派顺序缺陷实录）**：`修复轮次 1 未产出新内容，退出循环`——修稿 persona 是「修稿编辑」但其任务描述含「根据**审稿**意见对章节进行修正」，mock 分派链 `includes("审稿")` 先截胡，修稿调用拿到审稿 JSON → REVISED_CONTENT 为空。
- **绿（修稿分支移到「审稿」之前）**：fixture 轮与 API 轮双实证——`修复轮次 1/1（当前 62 分）` → 修稿 → `修复后达到通过线（91 分），退出循环` → 章节 `ready-for-review`（fixture 轮修订正文 2487 字带内；API 轮序列末值 91 直接通过不入循环=越界语义同时被证）。
- **门禁**：gate:ts 八步全绿（node-fallback-smoke 双腿=未注入形态的活体回归，61 端点 0 分歧）。Rust 零触碰裁剪备案（535 先例，纯 scripts）。

## 四、备案

- 注入序列按 mock 进程内全局计数——多书/多链并发共享同一序列；走查环境单链串行无碍，若未来并发走查需按会话键隔离。
- 降分 issue 为固定「开篇拖沓/structural」形态；「未净提升退出」「多轮修复」分支需序列如 `"62,65,70,91"` + 提高 maxReviewIterations（管线侧默认 1，属配置面非 mock 面）方可覆盖，按需另议。
- 修稿输出 FIXED_ISSUES 为固定一句；PATCHES 路由（local-only issue 形态）未覆盖。

## 五、教训

1. **mock 分派链是前缀匹配优先级问题**：`includes` 分派链里，宽关键词（「审稿」）必须排在窄关键词（「修稿编辑」）之后——persona 词与任务描述词分离时（persona 含窄词、任务描述含宽词），按 persona 排序不够，要按「最具体的身份词最先匹配」排全链。活体日志「未产出新内容」表象在 reviser 侧，根因在 mock 分派序——跨文件排查先看分派链全序。
2. **注入型 mock 的越界语义要显式设计**：取「最后一个值」让多轮 write-next 的行为可预期（首轮触发循环、后续直通），序列即场景脚本。
3. 修稿腿的「有差异」是硬约束（全等=未产出新内容）——mock 的修订正文用「原正文+修订尾段」最小差异形态即可满足，不必生成第二套正文。
