# 648 号：ChatPage 就地停止——QuickActions「写下一章」chip 翻转停止态（644 备案清偿）

日期：2026-10-06。类型：feat(studio UI)。前置：644 号备案「ChatPage 就地停止归 UI 需求」。

## 一、缺口精确化（勘察推翻表面定性）

644 备案定性「ChatPage 无停止翻转语义属缺口」——勘察后精确化：**后端停止链早已全通**（quick-action 发起的 write-next 生产任务 controller 注册进 activeConfirmedTasks，`reservedProductionSessions` 按 bookSession.sessionId 预留映射，`POST /sessions/:id/abort` → `findRunningTaskController` 命中 → 中止；且 ChatPage「停止当前回复」按钮调的 `abortSession(activeSessionId)` 默认 scope=all 已会命中该控制器）。真正缺口是**前端 UI**：QuickActions 的「写下一章」chip（聊天指令 chip，`requestedIntent:"write_next"` 走聊天轮）在任务执行中保持 disabled、不翻转，用户不知道停止能力存在——就地停止缺的是可发现性，不是通路。

## 二、实现（前端三文件，零后端改动）

1. **QuickActions**：新增 `writeNextRunning`/`onStopWriteNext` props——write_next chip 执行中翻「停止写作」（destructive 形态 + Square 图标 + `data-slot="quick-action-stop"`），点击走 `onStopWriteNext`；执行中即使全局面 disabled 停止 chip 仍可点（停止不许被 disabled 吞掉）；`key` 从 `label` 改为 `chip.labelEn`（翻转不重挂载）。
2. **chat/selectors**：新增 `activeSessionWriteNextRunning`——活跃会话消息里 `direct-write_next-*` 工具卡处于 running/processing。
3. **ChatPage**：派生该 selector + `handleStopWriteNext`（与「停止当前回复」同一 `abortSession` 链，scope=all 中止聊天轮与轮内任务，640 受理边界语义）+ 传 props。

## 三、红绿

- **jsdom 四断言**（QuickActions.interaction.test.tsx）：默认态旧行为不变/执行中翻停止态+点击走停止回调/全局面 disabled 时停止仍可点/审计 chip 不受影响。stash 旧码红 2/4（停止态两断言）→ 绿 4/4。
- **真浏览器活体**（walkthrough-env node 腿 + 慢速流 60ms）：点「写下一章」→ 3s 后 chip 翻「停止写作」destructive（任务卡执行中同步可见）→ 点击 → 工具卡「已由用户停止」收敛、chip 回「写下一章」。
- **门禁**：gate:ts 八步全绿（首跑 typecheck 红=测试文件 HTMLElement 缺 disabled 属性断言，as HTMLButtonElement 修复后绿）。Rust 零触碰裁剪备案（647 号刚整体复验过 Rust 面，本号纯 studio UI）。

## 四、备案

- 停止走 scope=all：聊天轮与轮内 write-next 一并中止（640 受理边界语义，轮内任务本就同生共死）；若未来需要「停聊天留任务」的分离语义另议。
- `direct-write_next-` 前缀派生与 server 端 `direct-${confirmedIntent}-${uuid}` 命名耦合——命名变更会静默失效；既有 server.test/交互测试锚定该前缀（action.test 855 行），风险已被测试面锁定。
- 侧栏 book 项的快捷「写下一章」按钮（另一入口）未接翻转——它直调 REST 端点语义不同，若需同款翻转另号处理。

## 五、教训

1. **「缺能力」与「缺可发现性」要分开定性**：644 备案说「无停止翻转语义属缺口」，勘察发现通路（abort→controller 映射）早已全通、缺的只是 UI 翻转——备案措辞凭走查表象下结论，清偿前先重新勘察缺口边界。
2. **活体探针的页面状态前提要先验证**：新 root 环境会话列表为空 → activeSessionId 为空 → handleQuickAction 静默 return，点击无任何反应——探针前先确认状态前提（会话存在），否则会把「没发任务」误读为「翻转失败」。
