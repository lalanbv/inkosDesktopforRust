# 623 号：全量测试 + 代码审查 + 真人式 GUI 走查——hasStream 毫秒撞表 P0 缺陷修复

> 目标（用户）：全量测试、代码审查、模拟真人测试，确保功能正常没有 BUG。
> 本号 = 全量门禁矩阵复跑 + 621/622 提交逐行复审 + 浏览器自动化真人式走查（walkthrough-env 活体环境）+ 走查抓获 1 个 P0 前端缺陷的确诊/修复/三重验证闭环。

## ① 全量测试矩阵（全部绿）

| 门禁 | 结果 |
|---|---|
| gate:ts 七步（build/typecheck/test/audit:npm/node-fallback-smoke/engine-contract-diff/export-epub-smoke） | ✓（修复后复跑：20.7s/20.1s/84.4s/2.4s/34.6s/40.6s/34.1s） |
| cargo:testgate（INKOS_DUEL=1 内置） | ✓ engine-rs 45 目标 1897 passed + src-tauri 25 目标 584 passed（较 621 号时 1803/578 增长来自并行会话 R38–R46 新面） |
| duel 真跑核实 | ✓ 日志落盘复读：strangler_duel 8+ 用例逐名 `ok`（非早退假绿） |
| clippy:gate | ✓ 双 crate 0 告警 |
| audit:rust（cargo:testgate 内置） | ✓ 双 lock 0 漏洞 |
| bench:gate | ✓ 12 基准漂移 -13.2%~+7.5%（阈值 +30%，零回退；首轮被机器负载拦截 [loadavg 34%/核>20%]，ps 辨源=ZCode 宿主渲染+Spotlight 索引，低谷重试通过——257/529 号教训复用） |
| audit:semantic-patterns | ✓ 0 候选 |

## ② 代码审查（621/622 提交逐行复审）

- 622 `pi-stream.ts` start 事件补推：空文本/多文本块/纯 tool_calls/错误路径边界均安全；`contentIndex:0` 与 agent-loop 聚合语义经差分器活体逐字节验证。
- 621 `server.ts` 四层 resolvedServiceId 记录点（层1 reqService / 层2 serviceConfigKey(firstService) / 层3 svcName / 层4 undefined→恒流式）与 `resolveConfiguredServiceEntry` 键格式逐一对表无误；层2 语义修正（偏好跟随实际解析服务）正确。
- 621 `agent-session.ts` CachedAgent streamPreference tracker 三触点（接口/失效判定/缓存写入）齐全。
- `audit:semantic-patterns` 0 候选。

## ③ 真人式 GUI 走查（browser-use IAB 黑盒驱动，环境=walkthrough-env 一键编排 mock+Rust 引擎+fixture）

**环境准备**（非测试态）：`node scripts/walkthrough-env.mjs --root /tmp/inkos-walk-623`（fixture 自断言全过：resync 基线 3、run-log 11、promises 4 条）。测试态全程只做真实用户操作（点击/输入/滚动），观察=DOM 快照+截图双验证（证据存 `gui-test-screenshots/`，不入库）。

| 点 | 结果 | 证据 |
|---|---|---|
| T0 首页（写作数据 123 字/2 章、运行遥测「累计 11 · 失败 0」与 fixture 对账、SSE 实时绿点、书卡） | PASS | t0_home.png |
| T1 聊天链（新建会话→发消息→回复渲染） | **FAIL→修复→复验 PASS** | t1_chat_no_assistant_reply.png → t1_chat_reply_fixed.png |
| T2 书详情伏笔池：5 分类 chips+4 条目，「悬念」筛选 4→1 | PASS | t2_foreshadow_filter.png |
| T2.5 数据分析页（2 章/123 字/状态分布与 fixture 一致） | PASS | t3_bookcard_state.png |
| T3 书籍设置承诺账本：kind 徽标+open 置顶+「悬念」筛选 4→1 | PASS | t3_promise_ledger_filter.png |
| T5 市场雷达：历史条目回放→市场概要+2 推荐卡→「从该选题开书」→ #/book/new 输入框完整预填 | PASS | t5_radar_open_book_prefill.png |
| T6 项目设置：对话偏好 checkbox、Agent Skills 注册表展示、提示词分组 | PASS | t6_settings_skills.png |
| Skill 文件夹导入 | 运行时不支持（IAB 无 file chooser），标注跳过 | — |
| 插件管理窗/托盘 | 原生面，属 DESKTOP_WALKTHROUGH.md 人工清单 | — |

**非缺陷备案**：① 模型下拉菜单的模态遮罩在菜单关闭动画期短暂残留（`fixed inset-0 z-50 bg-black/10`，外部点击即正常消散，无残留）；② 会话级 SSE 在 mock 秒回（6ms）时于事件广播后才连上、`finally` 即关——事件全错过但 POST 响应兜底渲染，生产形态（LLM 秒级耗时）无此窗口；③ 走查期间侧栏某次点击创建了空会话（预期功能行为，一次性 fixture 环境）。

## ④ P0 缺陷：hasStream 毫秒撞表→成功回复被无声丢弃（确诊+修复+三重验证）

**现象**：聊天页发送消息，引擎 6-10ms 完成轮次并落盘（messageCount=2，curl 直证响应契约完整 `{response,details,session,usage,timings}`），UI 却只渲染用户气泡——无回复、无错误提示、无失败标记；切走再切回（3 次重拉服务端 1300B 全量消息）仍不渲染。

**根因**（`packages/studio/src/store/chat/slices/message/action.ts`）：`sendMessage` 先取 `streamTs = Date.now()+1`，随后 `addUserMessage` 的 `Date.now()` 落在下一毫秒 → **用户消息 timestamp === streamTs**（两处 Date.now 间隔≈0.5-1ms，毫秒边界落在其中的概率≈每次发送掷硬币级，真机 2/2 复现）。`hasStream` 判 `messages.some(m => m.timestamp === streamTs)` **不带 role 检查**→被用户消息误判 true→走 `finalizeStream`，而 finalizeStream 只更新 `timestamp===streamTs && role==="assistant"` 的消息→**静默 no-op**。同族函数（appendStreamChunk/getOrCreateStream/replaceStreamWithError/finalizeStream）全带 role 检查，唯 hasStream 漏——不对称即缺陷。成功路径四分支（error/finalContent/toolExecutions/else）全无痕迹的唯一交集。

**修复**：两处 hasStream（成功路径+catch 路径）补 `&& message.role === "assistant"`，与全族判定口径对齐。修复后撞表场景 hasStream=false → 静态兜底分支追加助手消息。

**三重验证**：
1. **可证伪单测**（621 号协议）：新增「appends the success reply when the user bubble timestamp collides with streamTs (623)」——vi.spyOn(Date,"now") 脚本化撞表（首调 base→streamTs=base+1，后续恒 base+1→用户消息撞表）；旧代码预测转红 `expected undefined to be '设定文档全文。'` 实测转红（35 旧用例不受扰），修复后 36/36 绿。
2. **活体复验**：重建 studio dist → 浏览器硬刷新 → 发「测试三」→ 新回复+历史回复完整渲染（markdown 表格），计量徽章 1920 tok。
3. **gate:ts 七步全量复跑绿**（含修复代码；studio-only 改动，Rust 零改动照 535/621 先例裁剪 Rust 复跑——本日 Rust 全目标已在此前全绿）。

**关联缺陷备案（不改语义，留观察）**：`loadSessionMessages`（207 行 `messages.length > 0` 即丢弃）与 `loadSessionDetail`（416 行本地非空优先）两条陈旧守卫使「活体丢消息后应用内切回」无法从服务端恢复（F5 整页重载可恢复）。修复④后健康链路无用户可见影响；其语义（本地优先 vs 服务端真源）涉乐观更新取舍，留产品级裁决，不在本号扩大战场。

## ⑤ 教训

1. **「毫秒撞表」类竞态的测试不可达性**：`Date.now()+1` 与紧邻 `Date.now()` 的碰撞概率≈每次发送 δ ms（δ≈两次取时之间的工作量），组件测试的 jsdom 时序几乎不可能命中——活体浏览器走查才是这类缺陷的唯一抓手，再次印证 574/591 号真机走查路线的价值。
2. **「四分支全无痕迹」是强定位信号**：无回复+无错误+无失败标记+finally 已跑 → 逐一排除后只剩「静默 no-op 型更新」，role 口径不对称是第一嫌疑。
3. **ARIA 快照会包含折叠容器内内容**（grid overflow-hidden 裁剪不改变可达树），折叠态误判曾把走查带偏一轮——视觉截图与 ARIA 双验证缺一不可（web-gui-tester 技能纪律的实证）。
4. **走查驱动的诊断分层**：UI 观测（截图/ARIA）→ 性能资源条目（POST 6ms/ES 迟到）→ store fiber 只读探针（msgN=1/零错误零失败标记）→ curl 引擎契约复现 → 代码定位，每层只前进一小步，避免跳到结论。

## 验证清单

- [x] gate:ts 七步全绿（修复后复跑）
- [x] cargo:testgate 双 crate 1897+584 全绿（duel 真跑日志核实）
- [x] clippy 双 crate 0 告警；audit:rust 双 lock 0 漏洞；audit:npm 0 advisory
- [x] bench:gate 12 基准零回退（低谷复跑）
- [x] audit:semantic-patterns 0 候选
- [x] GUI 走查 7 点双验证（截图 7 张）+ 1 P0 缺陷修复闭环
- [x] 可证伪回归测试红→绿 36/36
