# 644 号：642 write-next 停止活体走查——抓获 H1/D2 双 UI 消费面缺陷并修复

日期：2026-10-05。类型：fix(studio UI+core 通知面)+test(chore 走查基建)。前置：642 号（6d680be8）write-next 可停性双端对齐，验收面全是 server 单测——按 639 纪律（活体走查是 mock 测试唯一对偶验证）做真浏览器走查，双缺陷活体实锤后红绿清偿。

## 一、走查基建（本号入库的走查能力扩展）

1. **walkthrough-mock 慢速流**（`WALKTHROUGH_MOCK_STREAM_DELAY_MS`，默认 0=现行为逐字节等价）：正文类响应按 8 字符/块逐块 SSE 发射——write-next 流中点停止需要足够长的执行窗口（delay=40ms 时单链 ~12s）。协议保真保持 548 号（finish chunk+[DONE] 收尾）。
   - **首跑挂死教训**：断流清理挂 `req.on("close")` 会在请求体消费完（end 后消息即 complete）立即触发，把发射 timer 清掉、流永不推进——必须挂 `res.on("close")`（连接提前断开才触发）。fixture 全链在慢速流下复跑通过后环境才可用。
2. **walkthrough-env `--engine node`**：642 修复面在 Node 直连端点，走查须能指定回退端后端（默认 rust 不变）。node 腿先写与 fixture 同形的最小项目配置再起（510 号启动检查要求 root 已有 inkos.json），fixture 随后幂等重写。

## 二、走查路由澄清（非缺陷定性）

`#/book/:id` 渲染的是 **ChatPage（对话创作页）**，其「写下一章」为快捷触发器、无 176 号停止翻转语义；**停止能力在 BookDetail（`#/book/:id/settings`）**。ChatPage 执行中按钮 disabled+任务卡形态属设计内（就地停止归 UI 需求另议）。T1-T2 全链在 BookDetail 验证：发起 600ms 内翻转「停止写作」红色 → 流中保持 → 点击停止后 1.5s 内收敛回「写下一章」（真停，642 API 层活体生效双端确认）。

## 三、H1（UI 提示条）：中性停止文案落红色失败分支——实锤+修复

- **活体铁证**：停止后提示条 = `后台任务失败: 写作已按您的要求停止。`，className 含 `border-destructive`——642 中性文案正确到达 UI，但遗留的 `includes("Operation aborted")` 判定不认它，被红色失败分支包裹（188 号「用户主动停止不以失败呈现」语义回归，642 文案变更的 UI 消费面漂移）。
- **修复**：`use-book-activity.ts` 新增 `WRITE_STOPPED_MESSAGE` + `isWriteStoppedMessage()`（认 642 双端中性文案+遗留 abort 字面）；BookDetail 内联提示条提取为导出组件 `WriteStatusBanner`（jsdom 可直测），判定换 helper。
- **红绿**：`BookDetail.interaction.test.tsx` 五断言（中性文案/遗留字面/真失败/写作中/空态），stash 旧判定红恰 1/5（核心断言：中性文案落 border-destructive）→ 绿 5/5。

## 四、D2（通知中心）：停止落「写章失败 [必须处理]」错误级噪音——实锤+修复

- **活体铁证**：通知中心出现 error 级「写章失败 [必须处理] 写作已按您的要求停止。」——`buildTaskReport`（core）只豁免 `agent:error+aborted`（625 号聊天面先例），write:error 的中性停止文案走失败三分类。625 语义在 write-next 通知面的对偶缺口。
- **修复**：`buildTaskReport` 对全部 `:error` 族事件按 error 载荷精确拦截（`aborted` / `Operation aborted` / 642 中性文案）返 null——中止/停止是作者意图收敛不是任务失败；真实失败不受扰。
- **红绿**：`author-report.test.ts` 新用例（中性文案/遗留字面/aborted 三豁免+真实失败不受扰）stash 旧码红 1/9 → 绿 9/9。

## 五、修复后活体复验（双腿）

- **node 腿**（回退端 8790）：停止→提示条中性（无 border-destructive，文案「写作已按您的要求停止。」）+ 通知中心清空后两轮停止零「写章失败」+ 续写自愈（再发起跑完 write:complete，章节 7→8）。
- **rust 腿**（引擎 8791）：同链全绿——Rust `write_next_route` 同形中性文案被 helper 正确识别（跨端字面同形契约的活体证明）；Rust 端停止传播在阶段边界收敛（~6s，175 号形态）。
- **门禁**：gate:ts 八步全绿（build 19.7s/typecheck 19.1s/test 83.5s/audit 2.4s/rust-bin 0.2s/smoke 34.3s/diff 40.3s/epub 34.3s，61 端点 0 分歧）。Rust 门禁裁剪备案（535 先例）：本号零触碰 engine-rs/src 与 src-tauri，二进制与 HEAD 同源（643 B 场景重建+本号 rust 腿活体）。

## 六、备案

- walkthrough-mock 与写作链解析器的形态差距（走查环境常态，fixture 断言不红）：planner memo parse failed×3 后 fallback、审稿输出解析失败跳过自动修稿、62 字篇幅不足 mock 文本固有——非本号缺陷；若需「零 fallback 走查形态」另立项对齐 mock 分派关键词与解析器预期。
- ChatPage 快捷入口的就地停止能力（当前须到设置页停止）归 UI 需求另议。
- `WALKTHROUGH_MOCK_STREAM_DELAY_MS` 默认 0：639 等既有走查形态零变化。

## 七、教训

1. **活体走查的断言要下到 className 级**：文案对≠形态对——「写作已按您的要求停止。」字面正确落在红条里，文案层断言发现不了形态层回归（红条 vs 中性条）。
2. **UI 消费面是文案变更的隐含契约方**：642 换停止文案时，grep 「Operation aborted」能找到 BookDetail 的判定——双端行为分歧清单要扩到「同包内字面耦合点」（同端也要查消费面），否则 188 号语义静默回归。
3. IncomingMessage 的 `close` 在请求体消费完即触发，连接断开清理必须挂 `res.on("close")`——mock 流式发射的清理锚点选错会让整条流挂死（首跑 90s 零输出定位）。
4. 走查路由先确认组件归属再断言行为：`#/book/:id`（ChatPage）与 `#/book/:id/settings`（BookDetail）是两个行为面，对着错误组件断言会把设计内形态误判为缺陷。
