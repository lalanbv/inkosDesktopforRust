# 626 号：stream 偏好面真人 GUI 走查——非流式路径 + 非流式轮中止/换向双活体验证（零缺陷，零代码）

> 目标（用户）：全量测试、代码审查、模拟真人测试，确保功能正常没有 BUG。
> 本号 = 质量目标新一轮增量的**未覆盖面补全**：621/622 修复的 stream 偏好面此前只经差分器（服务端 SSE 序列）验证，**客户端渲染层从未走过真人 GUI**；624/625 的中止语义也只在流式轮活体验证过——非流式轮的停止/换向是全新交叉面。静态复审 + 活体走查 T1-T4 全过，零功能缺陷，零代码改动。

## ① 静态复审（非流式轮中止/渲染双端路径，零缺陷）

- **引擎侧**：624 号 `tokio::select!` 在 `agent_loop.rs:186-197` 包裹 `chat.chat(&messages, tools)`，位于 `run_agent_loop` **共享循环**内——非流式只是 LLM 传输形态不同（`agent_route.rs:606` `.with_stream(ov.stream)`），在飞中止对非流式轮同样生效；中止后走 aborted 分支 = `fail_chat_turn` + `agent:error{error:"aborted"}` + 500 AGENT_ERROR，不落盘。
- **客户端成功路径**（`action.ts`）：623 号 role 收窄后的 `hasStream` 双分支对非流式轮成立——SSE 单 draft:delta 先到则 `finalizeStream` 更新流气泡，晚到/错过则静态追加，二者均恰好一次渲染。
- **客户端中止路径**：`abortSession`（action.ts:387-423）设 `chatAbortedAt` + POST abort 端点 + 关 SSE；catch 路径（724-738 行）`chatAbortedAt >= streamTs` → `discardAbortedStream`（非流式无流气泡 = no-op）→ 零痕迹。
- **通知面**：625 号 `buildTaskReport` 过滤 `agent:error+error==="aborted"`，与流式形态无关，非流式中止轮同样不产生「聊天任务出错」噪音。

## ② 活体走查 T1-T4 全过（walkthrough-env + SIGSTOP 冻结法，截图 9 张存 gui-test-screenshots/626_*，不入库）

**环境准备**：`node scripts/walkthrough-env.mjs --root /tmp/inkos-walk-626`（fixture 自断言全过：resync 基线 3、run-log 11）；浏览器新标签页 bundle 指纹 `index-CS8lxCQ5.js` 与 dist 逐一核对一致（625 号陈旧 bundle 教训纪律化执行，本轮零伪象）。

| 点 | 内容 | 结果 | 证据 |
|---|---|---|---|
| T1 | 设置 UI（服务商管理→Mock 详情）切「流式响应」关→保存→**inkos.json `stream:false` 落盘**→聊天页发送→回复（markdown 表格）**恰好一次渲染**→transcript 恰 2 条 0 失败 | PASS | t0/t1 |
| T2 | SIGSTOP 冻结 mock→发送挂起（「思考中…」+停止按钮激活）→点停止→**零痕迹**（无✗无重试无回复）→**通知徽章 0**（625 修复面在非流式轮同样成立）→SIGCONT 解冻→迟到 500 无新痕迹→磁盘 `[user, assistant, user]` + request_failed 无 assistant 回复 = **217 号契约** | PASS | t2/t3 |
| T3 | 冻结下换向（发 A 挂起后发 B）→冻结窗双用户气泡+仅一轮「思考中」→解冻→**仅 B 轮回复落地**（带 usage ↑100↓50，首包 9.6s 如实计入冻结窗时长）→A 轮零痕迹→磁盘 6 消息 + 2 request_failed（T2/T3 两中止轮） | PASS | t5/t6 |
| T4 | UI 切回流式→保存→`stream:true` 落盘→发送→回复落地→transcript 8 条收尾（两中止轮 request_failed、三轮对话 user/assistant 成对） | PASS | t7/t8 |

**流式旗标双向硬证据（零注入）**：622 号 mock 协议保真按 `body.stream` 旗标返回 JSON/SSE 双形态——引擎若误发 `stream:true` 则非流式 JSON 解析必败、误发 `stream:false` 则流式 SSE 解析必败；T1b 与 T4 的成功落地即双向反证引擎确实按解析服务的偏好发送旗标（621 号「偏好跟随实际解析服务」的用户可见层实证，619 号用户文档条目行为兑现）。

## ③ 非缺陷备案两条

1. **Field 字段标题 label 与 checkbox 无关联 = 标准形态非缺陷**：`ServiceDetailPage.tsx:371-380` 外层 `Field` 渲染的 `<label>流式响应</label>` 是字段标题（htmlFor=null、非包裹），真正开关是**内层包裹 label**（input 在 label 内，点击「开启」行即切换，坐标点击实证 `checked` 翻转成功）。走查驱动 T1 的翻转系点击点恰命中开关本体——初判「label 点击不切换」为**测试驱动瞄准伪象**（点到了字段标题而非控件行），非产品缺陷。
2. **「晚到 draft:delta 重复渲染」假设被 T1 证伪**：静态复审提出的竞态假设（POST 静态追加后 SSE 单 delta 晚到再建第二个气泡）在活体未发生（恰一次渲染 + transcript 2 条）。

## ④ 门禁策略（零代码改动的裁剪备案）

本号**零代码改动**（零缺陷无需修复）。625 号已在同 HEAD（58c65c1a）落成全量门禁矩阵全绿（gate:ts 七步 + cargo:testgate 1898+584 含 duel 8/8 真跑逐名核实 + clippy 双 0 + 双 audit 0 + smoke 双腿 + 差分器 48 对照 0 分歧 + bench:gate 12 基准零回退），且此后无任何代码落库——照 535/621/622/625 零改动裁剪先例，不重复复跑（较 620 号选择性复跑进一步：连选择性复跑的增量价值亦为零）。

## ⑤ 教训

1. **mock 协议保真使黑盒发送成为流式旗标的双向硬证据**：协议按旗标分形的 mock 下，成功/失败本身就是引擎请求形态的判定依据，无需注入观测。
2. **dom_cua 点击目标要核几何**：ref 命中的是「字段标题」还是「控件本体」须以 boundingClientRect 定位为准（本轮外层标题 label 宽 280px 横贯整行，与其下方的开关控件是两个元素）——盲目重试同一 ref 只会重复落空。
3. **625 号 bundle 指纹纪律化复用零伪象**：新标签页 + 资源条目指纹比对前置，本轮全部观测建立在正确构建上。
4. **未覆盖面驱动的新一轮**：全量矩阵复跑会重复 625 的劳动；真正增量在「已修面的用户可见层补全」（差分器覆盖服务端 SSE、活体覆盖客户端渲染，两层互补不可互替）。
